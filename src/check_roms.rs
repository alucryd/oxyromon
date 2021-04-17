use super::chdman::*;
use super::checksum::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::convert::TryFrom;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("check-roms")
        .about("Checks ROM files integrity")
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Checks all systems")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, matches.is_present("ALL")).await?;

    for system in systems {
        check_system(connection, matches, &system, &progress_bar).await?;
    }

    Ok(())
}

async fn check_system(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println(&format!("Processing \"{}\"", system.name));

    let trash_directory = get_trash_directory(connection, system).await?;

    let header = find_header_by_system_id(connection, system.id).await;
    let roms = find_roms_with_romfile_by_system_id(connection, system.id).await;
    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let mut roms_by_romfile_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms.into_iter().for_each(|rom| {
        let group = roms_by_romfile_id
            .entry(rom.romfile_id.unwrap())
            .or_insert(vec![]);
        group.push(rom);
    });

    let mut romfile_moves: Vec<(Romfile, String)> = Vec::new();
    for romfile in romfiles {
        let romfile_path = get_canonicalized_path(&romfile.path).await?;
        let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
        let roms = roms_by_romfile_id.remove(&romfile.id).unwrap();

        progress_bar.println(&format!(
            "Processing {:?}",
            romfile_path.file_name().unwrap()
        ));

        let ok: bool;
        if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
            ok = check_archive(connection, &header, &romfile_path, roms, &progress_bar).await?;
        } else if CHD_EXTENSION == romfile_extension {
            ok = check_chd(connection, progress_bar, &header, &romfile_path, roms).await?;
        } else if CSO_EXTENSION == romfile_extension {
            ok = check_cso(
                connection,
                &progress_bar,
                &header,
                &romfile_path,
                roms.get(0).unwrap(),
            )
            .await?;
        } else {
            ok = check_other(progress_bar, &header, &romfile_path, roms.get(0).unwrap()).await?;
        }

        if !ok {
            romfile_moves.push((
                romfile,
                trash_directory
                    .join(romfile_path.file_name().unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap()
                    .to_owned(),
            ));
        }
    }

    if !romfile_moves.is_empty() {
        // print a summary
        progress_bar.println("Summary:");
        for romfile_move in &romfile_moves {
            progress_bar.println(&format!("{} -> {}", romfile_move.0.path, romfile_move.1));
        }

        // prompt user for confirmation
        if matches.is_present("YES") || confirm(true)? {
            for romfile_move in romfile_moves {
                rename_file(&romfile_move.0.path, &romfile_move.1).await?;
                update_romfile(connection, romfile_move.0.id, &romfile_move.1).await;
            }
        }
    } else {
        progress_bar.println("Nothing to do");
    }

    Ok(())
}

async fn check_archive<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    header: &Option<Header>,
    romfile_path: &P,
    mut roms: Vec<Rom>,
    progress_bar: &ProgressBar,
) -> SimpleResult<bool> {
    let sevenzip_infos = parse_archive(progress_bar, romfile_path)?;

    if sevenzip_infos.len() != roms.len() {
        return Ok(false);
    }

    for sevenzip_info in sevenzip_infos {
        let size: u64;
        let crc: String;
        if header.is_some() || sevenzip_info.crc.is_empty() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let extracted_path = extract_files_from_archive(
                progress_bar,
                romfile_path,
                &[&sevenzip_info.path],
                &tmp_directory.path(),
            )?
            .remove(0);
            let size_crc =
                get_file_size_and_crc(progress_bar, &extracted_path, &header, 1, 1).await?;
            remove_file(&extracted_path).await?;
            size = size_crc.0;
            crc = size_crc.1;
        } else {
            size = sevenzip_info.size;
            crc = sevenzip_info.crc.clone();
        }
        let rom_index = roms
            .iter()
            .position(|rom| rom.name == sevenzip_info.path)
            .unwrap();
        let rom = roms.remove(rom_index);
        if i64::try_from(size).unwrap() != rom.size || crc != rom.crc {
            return Ok(false);
        }
    }

    Ok(true)
}

async fn check_chd<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    roms: Vec<Rom>,
) -> SimpleResult<bool> {
    let tmp_directory = create_tmp_directory(connection).await?;

    let names_sizes: Vec<(&str, u64)> = roms
        .iter()
        .map(|rom| (rom.name.as_str(), rom.size as u64))
        .collect();
    let bin_paths = extract_chd_to_multiple_tracks(
        progress_bar,
        romfile_path,
        &tmp_directory.path(),
        &names_sizes,
    )
    .await?;
    let mut crcs: Vec<String> = Vec::new();
    for (i, bin_path) in bin_paths.iter().enumerate() {
        let (_, crc) =
            get_file_size_and_crc(progress_bar, &bin_path, &header, i, bin_paths.len()).await?;
        crcs.push(crc);
        remove_file(&bin_path).await?;
    }

    if roms.iter().enumerate().any(|(i, rom)| crcs[i] != rom.crc) {
        return Ok(false);
    }

    Ok(true)
}

async fn check_cso<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    rom: &Rom,
) -> SimpleResult<bool> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let iso_path = extract_cso(progress_bar, romfile_path, &tmp_directory.path())?;
    let (size, crc) = get_file_size_and_crc(progress_bar, &iso_path, &header, 1, 1).await?;
    remove_file(&iso_path).await?;
    Ok(i64::try_from(size).unwrap() == rom.size && crc == rom.crc)
}

async fn check_other<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    rom: &Rom,
) -> SimpleResult<bool> {
    let (size, crc) = get_file_size_and_crc(progress_bar, romfile_path, &header, 1, 1).await?;
    Ok(i64::try_from(size).unwrap() == rom.size && crc == rom.crc)
}

#[cfg(test)]
mod test {
    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::*;
    use async_std::fs;
    use async_std::path::PathBuf;
    use async_std::prelude::*;
    use async_std::sync::Mutex;
    use std::env;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_check_sevenzip() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom.7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom.7z"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(!romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_zip() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom.zip");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom.zip"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(!romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_chd_single_track() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Single Track).chd");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Single Track).chd"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(!romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_chd_multiple_tracks() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
            &romfile_path,
        )
        .await
        .unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);

        for romfile in romfiles {
            assert!(!romfile.path.contains("/Trash/"));
            assert!(Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_check_cso() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        env::set_var(
            "PATH",
            format!(
                "{}:{}",
                test_directory.as_os_str().to_str().unwrap(),
                env::var("PATH").unwrap()
            ),
        );
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).cso");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).cso"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(!romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_other() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(!romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_other_size_mismatch() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile = find_romfiles(&mut connection).await.remove(0);
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&romfile.path)
            .await
            .unwrap();
        file.set_len(512).await.unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }

    #[async_std::test]
    async fn test_check_other_crc_mismatch() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile = find_romfiles(&mut connection).await.remove(0);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&romfile.path)
            .await
            .unwrap();
        file.write_all(b"00000000").await.unwrap();
        file.sync_all().await.unwrap();

        // when
        let matches = subcommand().get_matches_from(&["check-roms", "-y"]);

        check_system(&mut connection, &matches, &system, &progress_bar)
            .await
            .unwrap();

        // then
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.remove(0);
        assert!(romfile.path.contains("/Trash/"));
        assert!(Path::new(&romfile.path).is_file().await);
    }
}
