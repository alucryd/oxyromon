use super::check_roms;
use super::common::{CommonRomfile, FromPath};
use super::compact_roms;
use super::config;
use super::config::{
    TestDatabase, add_to_list, set_bool, set_rom_directory, set_string, set_tmp_directory,
};
use super::convert_roms;
use super::create_dats;
use super::database::{
    establish_connection, find_romfiles, find_romfiles_by_system_id,
    find_roms_with_romfile_by_system_id, find_systems,
};
use super::download_dats;
use super::export_roms;
use super::generate_playlists;
use super::import_clone_lists;
use super::import_dats;
use super::import_irds;
use super::import_keys;
use super::import_patches;
use super::import_roms;
use super::model::{Romfile, System};
use super::purge_games;
use super::purge_irds;
use super::purge_roms;
use super::purge_systems;
use super::rebuild_roms;
use super::sort_roms;
use super::util::get_system_directory;
use anyhow::Result;
use indicatif::ProgressBar;
#[cfg(feature = "server")]
use sqlx::SqlitePool;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::Sqlite;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;

/// The dat nearly every matrix cell imports.
pub const DAT: &str = "Test System (20200721).dat";
/// The dat for the GameCube/Wii cells.
pub const RVZ_DAT: &str = "Test System (20230618) (RVZ).dat";
/// The dat for the Wii U cells.
pub const WUD_DAT: &str = "Test System (20261025) (WUD).dat";

/// One `#[tokio::test]` per matrix cell, each handing its case to `$runner`.
macro_rules! test_cases {
    ($runner:ident; $($name:ident : $case:expr),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                $runner(&$case).await;
            }
        )+
    };
}
pub(crate) use test_cases;

/// The fixtures' directory: the checked-in files every test reads from,
/// resolved from the crate root so a test never depends on the cwd.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn copy_tree_owned(from: PathBuf, to: PathBuf) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        if tokio::fs::symlink_metadata(&from).await.unwrap().is_dir() {
            tokio::fs::create_dir_all(&to).await.unwrap();
            let mut entries = tokio::fs::read_dir(&from).await.unwrap();
            while let Some(entry) = entries.next_entry().await.unwrap() {
                copy_tree_owned(entry.path(), to.join(entry.file_name())).await;
            }
        } else {
            tokio::fs::copy(&from, &to).await.unwrap();
        }
    })
}

/// A test's whole world: its own database, its own ROM and temporary
/// directories under one scratch root outside the fixtures, its hidden
/// progress bar, and the settings pointing at them.
///
/// The scratch root lives under the system temp directory, never inside
/// `tests/`, so a killed run leaves no junk beside the fixtures.
pub struct TestEnv {
    pub connection: PoolConnection<Sqlite>,
    /// A handle to the same pool the connection came from: for the rare
    /// test that hands one to a `main` of its own, such as the server's.
    #[cfg(feature = "server")]
    pub pool: SqlitePool,
    pub rom_directory: PathBuf,
    pub tmp_directory: PathBuf,
    pub progress_bar: ProgressBar,
    _root: TempDir,
    // Dropped last, after the connection closed on its database.
    _db: TestDatabase,
}

/// Runs one subcommand's `main` on the args, `subcommand()` included.
macro_rules! run_subcommand {
    ($env:expr, $module:ident, $args:expr) => {{
        let env: &mut TestEnv = $env;
        let matches = $module::subcommand().get_matches_from($args);
        $module::main(&mut env.connection, &matches, &env.progress_bar).await
    }};
}

impl TestEnv {
    /// A bare environment: database, directories and settings, no dat.
    pub async fn new() -> Self {
        let root = TempDir::new().unwrap();
        let rom_directory = root.path().join("roms");
        let tmp_directory = root.path().join("tmp");
        std::fs::create_dir(&rom_directory).unwrap();
        std::fs::create_dir(&tmp_directory).unwrap();

        let db = TestDatabase::new();
        let pool = establish_connection(db.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();
        let rom_directory = set_rom_directory(&mut connection, rom_directory).await;
        let tmp_directory = set_tmp_directory(&mut connection, tmp_directory).await;

        Self {
            connection,
            #[cfg(feature = "server")]
            pool,
            rom_directory,
            tmp_directory,
            progress_bar: ProgressBar::hidden(),
            _root: root,
            _db: db,
        }
    }

    /// An environment with one dat imported: the setup nearly every test
    /// starts with.
    pub async fn with_dat(dat: &str) -> Self {
        let mut env = Self::new().await;
        env.import_dats(&[dat]).await;
        env
    }

    /// Imports each named fixture dat into the database.
    pub async fn import_dats(&mut self, dats: &[&str]) {
        for dat in dats {
            let path = fixtures().join(dat);
            self.run(&["import-dats", path.to_str().unwrap()])
                .await
                .unwrap();
        }
    }

    /// The path of a fixture in `tests/`, read in place: for the rare
    /// test that feeds one to a library call instead of importing it.
    pub fn fixture_source(&self, name: &str) -> PathBuf {
        fixtures().join(name)
    }

    /// Copies a fixture file, or a fixture directory with its whole tree,
    /// into the temporary directory under the same name; returns the copy.
    pub async fn fixture(&self, name: &str) -> PathBuf {
        self.fixture_into(&self.tmp_directory, name).await
    }

    /// Copies a fixture into an arbitrary directory under the same name:
    /// the rare test that plants a file straight into the ROM directory
    /// or another corner of its world instead of the temporary one.
    pub async fn fixture_into(&self, directory: &Path, name: &str) -> PathBuf {
        let target = directory.join(name);
        copy_tree_owned(fixtures().join(name), target.clone()).await;
        target
    }

    /// Imports paths with the `import-roms` subcommand.
    pub async fn import_roms(&mut self, paths: &[&Path]) {
        let mut argv: Vec<&str> = vec!["import-roms"];
        argv.extend(paths.iter().map(|path| path.to_str().unwrap()));
        self.run(&argv).await.unwrap();
    }

    /// Copies a fixture into the temporary directory under another name,
    /// then imports it into `system` with `import_other`, asserting a match.
    pub async fn import_other(&mut self, system: &System, fixture: &str, name: &str) {
        let path = self.tmp_directory.join(name);
        tokio::fs::copy(fixtures().join(fixture), &path)
            .await
            .unwrap();
        let result = import_roms::import_other(
            &mut self.connection,
            &self.progress_bar,
            &Some(system),
            &None,
            &HashSet::new(),
            CommonRomfile::from_path(&path).unwrap(),
            true,
            false,
            import_roms::UnattendedMode::Skip,
        )
        .await
        .unwrap();
        assert!(result.is_some(), "Failed to match \"{}\"", name);
    }

    /// The system the database holds: every test's dat names one.
    pub async fn system(&mut self) -> System {
        find_systems(&mut self.connection).await.remove(0)
    }

    /// Every romfile in the database.
    pub async fn romfiles(&mut self) -> Vec<Romfile> {
        find_romfiles(&mut self.connection).await
    }

    /// The one romfile the test is about, asserting it is alone.
    pub async fn single_romfile(&mut self) -> Romfile {
        let mut romfiles = self.romfiles().await;
        assert_eq!(romfiles.len(), 1);
        romfiles.remove(0)
    }

    /// The file behind a romfile's path, in the ROM directory.
    pub fn romfile_path(&self, romfile: &Romfile) -> PathBuf {
        self.rom_directory.join(&romfile.path)
    }

    /// Overwrites a file's first bytes, leaving its size alone: what a
    /// CRC mismatch test does to an imported ROM.
    pub async fn corrupt(&self, path: &Path) {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await
            .unwrap();
        file.write_all(b"00000000").await.unwrap();
        file.sync_all().await.unwrap();
    }

    /// Cuts a file to `size` bytes: what a size mismatch test does.
    pub async fn truncate(&self, path: &Path, size: u64) {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await
            .unwrap();
        file.set_len(size).await.unwrap();
    }

    /// The directory a system's romfiles live under, Trash included.
    pub async fn system_directory(&mut self, system: &System) -> PathBuf {
        get_system_directory(&mut self.connection, system)
            .await
            .unwrap()
    }

    /// The roms of a system that have a romfile: what the convert and
    /// export matrices assert against.
    pub async fn roms_with_romfiles(&mut self, system: &System) -> Vec<super::model::Rom> {
        find_roms_with_romfile_by_system_id(&mut self.connection, system.id).await
    }

    /// Every romfile of a system.
    pub async fn romfiles_of(&mut self, system: &System) -> Vec<Romfile> {
        find_romfiles_by_system_id(&mut self.connection, system.id).await
    }

    /// A system-scoped boolean setting: the knobs `sort-roms` reads back
    /// from the database.
    pub async fn set_bool(&mut self, key: &str, value: bool, system_id: i64) {
        set_bool(&mut self.connection, key, value, Some(system_id)).await;
    }

    /// A system-scoped string setting.
    pub async fn set_string(&mut self, key: &str, value: &str, system_id: i64) {
        set_string(&mut self.connection, key, value, Some(system_id)).await;
    }

    /// One value added to a system-scoped list setting.
    pub async fn add_to_list(&mut self, key: &str, value: &str, system_id: i64) {
        add_to_list(
            &mut self.connection,
            &self.progress_bar,
            key,
            value,
            Some(system_id),
        )
        .await;
    }

    /// Import the fixtures `disks`, an ORE each of `platform`, as earlier
    /// versions wrote a game of several: the OREs, staged.
    pub async fn import_disk_ores(
        &mut self,
        platform: &'static str,
        disks: &[&str],
    ) -> Vec<PathBuf> {
        let staging = self.tmp_directory.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let options = ore_rs::Options {
            platform: Some(platform),
            ..Default::default()
        };
        let ores: Vec<PathBuf> = disks
            .iter()
            .map(|disk| {
                let ore = staging.join(disk).with_extension("ore");
                ore_rs::compress(&self.fixture_source(disk), &ore, &options, &[], &mut |_| {})
                    .unwrap();
                ore
            })
            .collect();
        let paths: Vec<&Path> = ores.iter().map(PathBuf::as_path).collect();
        self.import_roms(&paths).await;
        ores
    }

    /// Assert the ORE at `ore` is one game's ROMs, the fixtures `names`, in
    /// that order, each under its name and byte for byte, of `platform`.
    pub fn assert_set(&self, ore: &Path, platform: &str, names: &[&str]) {
        let reader = ore_rs::Reader::open(ore).unwrap();
        assert!(reader.is_set(), "{} is no set", ore.display());
        assert_eq!(reader.platform(), platform);
        let files = reader.files().unwrap();
        assert_eq!(
            files
                .iter()
                .map(|(name, ..)| name.as_str())
                .collect::<Vec<_>>(),
            names
        );
        for (name, start, len) in files {
            let rom = std::fs::read(self.fixture_source(&name)).unwrap();
            let mut bytes = vec![0; len as usize];
            ore_rs::Reader::open(ore)
                .unwrap()
                .read_at(start, &mut bytes)
                .unwrap();
            assert_eq!(bytes, rom, "{name}");
        }
    }

    /// The relative path a romfile under a system directory is stored as:
    /// the path's tail after the ROM directory, exactly as the database
    /// keeps it.
    pub fn stored_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.rom_directory)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    /// Runs a subcommand's `main` on these args, its first word naming
    /// the dispatch: the real CLI path, prompts included, so a test's
    /// `when` goes through the same doors as a user's command.
    pub async fn run(&mut self, args: &[&str]) -> Result<()> {
        match args.first().copied().unwrap_or_default() {
            "check-roms" => run_subcommand!(self, check_roms, args),
            "compact-roms" => run_subcommand!(self, compact_roms, args),
            "config" => run_subcommand!(self, config, args),
            "convert-roms" => run_subcommand!(self, convert_roms, args),
            "create-dats" => run_subcommand!(self, create_dats, args),
            "download-dats" => run_subcommand!(self, download_dats, args),
            "export-roms" => run_subcommand!(self, export_roms, args),
            "generate-playlists" => run_subcommand!(self, generate_playlists, args),
            "import-clone-lists" => run_subcommand!(self, import_clone_lists, args),
            "import-dats" => run_subcommand!(self, import_dats, args),
            "import-irds" => run_subcommand!(self, import_irds, args),
            "import-keys" => run_subcommand!(self, import_keys, args),
            "import-patches" => run_subcommand!(self, import_patches, args),
            "import-roms" => run_subcommand!(self, import_roms, args),
            "purge-games" => run_subcommand!(self, purge_games, args),
            "purge-irds" => run_subcommand!(self, purge_irds, args),
            "purge-roms" => run_subcommand!(self, purge_roms, args),
            "purge-systems" => run_subcommand!(self, purge_systems, args),
            "rebuild-roms" => run_subcommand!(self, rebuild_roms, args),
            "sort-roms" => run_subcommand!(self, sort_roms, args),
            other => unimplemented!("TestEnv::run does not dispatch \"{other}\" yet"),
        }
    }
}
