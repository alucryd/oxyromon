// A small arcade set for the merged tests: Clone 1 has its own "Test
// Program.rom", whose name its parent uses for other contents, and both clones
// have "Test Clones.rom", which their parent does not.

use super::super::check_roms;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn write_zip(path: &Path, entries: &[(&str, &str)]) {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    for (name, fixture) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&std::fs::read(Path::new("tests").join(fixture)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap();
}

pub fn entry_names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = zip::ZipArchive::new(File::open(path).unwrap())
        .unwrap()
        .file_names()
        .map(String::from)
        .collect();
    names.sort();
    names
}

/// The entries of the set's merged ZIP.
pub const MERGED_ENTRIES: [&str; 4] = [
    "Test Clone 1/Test Program.rom",
    "Test Clones.rom",
    "Test Common.rom",
    "Test Program.rom",
];

/// Imports the set as split ZIPs and returns its system.
pub async fn import_split_set(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    tmp_directory: &Path,
) -> System {
    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (MAME Merged).dat",
    ]);
    import_dats::main(connection, &matches, progress_bar)
        .await
        .unwrap();

    let system = find_arcade_systems(connection).await.remove(0);
    let system_directory = get_system_directory(connection, &system).await.unwrap();

    for (game, entries) in [
        (
            "Test Parent",
            vec![
                ("Test Program.rom", "Test Game (USA, Europe).rom"),
                ("Test Common.rom", "Test Game (Japan).rom"),
            ],
        ),
        (
            "Test Clone 1",
            vec![
                ("Test Program.rom", "Test Game (USA) (Rev 1).rom"),
                ("Test Clones.rom", "Test Game (USA, Europe) (Beta).rom"),
            ],
        ),
        (
            "Test Clone 2",
            vec![("Test Clones.rom", "Test Game (USA, Europe) (Beta).rom")],
        ),
    ] {
        let path = tmp_directory.join(format!("{}.zip", game));
        write_zip(&path, &entries);
        let matches = import_roms::subcommand().get_matches_from([
            "import-roms",
            "-u",
            "skip",
            path.to_str().unwrap(),
        ]);
        import_roms::main(connection, &matches, progress_bar)
            .await
            .unwrap();
        assert!(system_directory.join(format!("{}.zip", game)).is_file());
    }

    let system = find_system_by_id(connection, system.id).await;
    compute_system_completion(connection, progress_bar, &system)
        .await
        .unwrap();
    find_system_by_id(connection, system.id).await
}

pub async fn rebuild(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    merging: &str,
) -> System {
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", merging]);
    main(connection, &matches, progress_bar).await.unwrap();
    find_arcade_systems(connection).await.remove(0)
}

/// Asserts the set is merged: one ZIP, every file in it, every game full.
pub async fn assert_merged(connection: &mut SqliteConnection, system: &System) {
    let system_directory = get_system_directory(connection, system).await.unwrap();
    assert_eq!(system.merging, Merging::Merged as i64);
    assert_eq!(
        entry_names(&system_directory.join("Test Parent.zip")),
        MERGED_ENTRIES
    );
    assert!(!system_directory.join("Test Clone 1.zip").exists());
    assert!(!system_directory.join("Test Clone 2.zip").exists());

    let romfiles = find_romfiles(connection).await;
    assert_eq!(romfiles.len(), 1);
    for game in find_games_by_system_id(connection, system.id).await {
        assert_eq!(game.completion, Completion::Full as i64, "{}", game.name);
        for rom in find_roms_by_game_id_no_parents(connection, game.id).await {
            assert_eq!(rom.romfile_id, Some(romfiles[0].id), "{}", rom.name);
        }
    }
}

/// Runs `check-roms` and asserts no file failed, which would have moved it to
/// the trash.
pub async fn assert_checks(connection: &mut SqliteConnection, progress_bar: &ProgressBar) {
    let paths = |romfiles: Vec<Romfile>| {
        romfiles
            .into_iter()
            .map(|romfile| romfile.path)
            .collect::<Vec<String>>()
    };
    let before = paths(find_romfiles(connection).await);
    let matches = check_roms::subcommand().get_matches_from(["check-roms", "-a"]);
    check_roms::main(connection, &matches, progress_bar)
        .await
        .unwrap();
    assert_eq!(before, paths(find_romfiles(connection).await));
}
