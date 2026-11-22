use super::check_roms;
use super::config::{MUTEX, TestDatabase, set_rom_directory, set_tmp_directory};
use super::database::{establish_connection, find_romfiles, find_systems};
use super::import_dats;
use super::import_roms;
use super::model::{Romfile, System};
use super::util::get_system_directory;
use anyhow::Result;
use indicatif::ProgressBar;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::Sqlite;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::sync::MutexGuard;

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
///
/// The global `MUTEX` is held for the env's lifetime, exactly as every
/// test used to hold it itself. Dropping it is step 4 of #334, once no
/// test sets up its world by hand anymore.
pub struct TestEnv {
    pub connection: PoolConnection<Sqlite>,
    pub rom_directory: PathBuf,
    pub tmp_directory: PathBuf,
    pub progress_bar: ProgressBar,
    _guard: MutexGuard<'static, i32>,
    _root: TempDir,
    // Dropped last, after the connection closed on its database.
    _db: TestDatabase,
}

impl TestEnv {
    /// A bare environment: database, directories and settings, no dat.
    pub async fn new() -> Self {
        let _guard = MUTEX.lock().await;

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
            rom_directory,
            tmp_directory,
            progress_bar: ProgressBar::hidden(),
            _guard,
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
        let target = self.tmp_directory.join(name);
        copy_tree_owned(fixtures().join(name), target.clone()).await;
        target
    }

    /// Imports paths with the `import-roms` subcommand.
    pub async fn import_roms(&mut self, paths: &[&Path]) {
        let mut argv: Vec<&str> = vec!["import-roms"];
        argv.extend(paths.iter().map(|path| path.to_str().unwrap()));
        self.run(&argv).await.unwrap();
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

    /// Runs a subcommand's `main` on these args, its first word naming
    /// the dispatch: the real CLI path, prompts included, so a test's
    /// `when` goes through the same doors as a user's command.
    pub async fn run(&mut self, args: &[&str]) -> Result<()> {
        match args.first().copied().unwrap_or_default() {
            "check-roms" => {
                let matches = check_roms::subcommand().get_matches_from(args);
                check_roms::main(&mut self.connection, &matches, &self.progress_bar).await
            }
            "import-dats" => {
                let matches = import_dats::subcommand().get_matches_from(args);
                import_dats::main(&mut self.connection, &matches, &self.progress_bar).await
            }
            "import-roms" => {
                let matches = import_roms::subcommand().get_matches_from(args);
                import_roms::main(&mut self.connection, &matches, &self.progress_bar).await
            }
            other => unimplemented!("TestEnv::run does not dispatch \"{other}\" yet"),
        }
    }
}
