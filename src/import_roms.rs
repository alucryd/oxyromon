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
use std::ffi::OsString;
use std::str::FromStr;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("import-roms")
        .about("Validates and imports ROM files into oxyromon")
        .arg(
            Arg::with_name("ROMS")
                .help("Sets the ROM files to import")
                .required(true)
                .multiple(true)
                .index(1),
        )
        .arg(
            Arg::with_name("SYSTEM")
                .short("s")
                .long("system")
                .help("Sets the system number to use")
                .required(false)
                .takes_value(true),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let romfile_paths: Vec<String> = matches.values_of_lossy("ROMS").unwrap();
    let system = prompt_for_system(
        connection,
        matches
            .value_of("SYSTEM")
            .map(|s| FromStr::from_str(s).expect("Failed to parse number")),
    )
    .await?;
    let header = find_header_by_system_id(connection, system.id).await;
    let system_directory = get_system_directory(connection, &system).await?;

    for romfile_path in romfile_paths {
        let romfile_path = get_canonicalized_path(&romfile_path).await?;
        import_rom(
            connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    }

    Ok(())
}

pub async fn import_rom<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    system: &System,
    header: &Option<Header>,
    romfile_path: &P,
) -> SimpleResult<()> {
    progress_bar.println(&format!(
        "Processing {:?}",
        romfile_path.as_ref().file_name().unwrap()
    ));

    // abort if the romfile is already in the database
    if find_romfile_by_path(
        connection,
        romfile_path.as_ref().as_os_str().to_str().unwrap(),
    )
    .await
    .is_some()
    {
        progress_bar.println("Already in database");
        return Ok(());
    }

    let romfile_extension = romfile_path
        .as_ref()
        .extension()
        .unwrap()
        .to_str()
        .unwrap()
        .to_lowercase();

    if ARCHIVE_EXTENSIONS.contains(&romfile_extension.as_str()) {
        import_archive(
            connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
            &romfile_extension,
        )
        .await?;
    } else if CHD_EXTENSION == romfile_extension {
        import_chd(
            connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    } else if CSO_EXTENSION == romfile_extension {
        import_cso(
            connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    } else {
        import_other(
            connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    }

    Ok(())
}

async fn import_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    system: &System,
    header: &Option<Header>,
    romfile_path: &P,
    romfile_extension: &str,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let sevenzip_infos = parse_archive(progress_bar, romfile_path)?;

    // archive contains a single file
    if sevenzip_infos.len() == 1 {
        let size: u64;
        let crc: String;
        let sevenzip_info = sevenzip_infos.get(0).unwrap();

        // system has a header or crc is absent
        if header.is_some() || sevenzip_info.crc.is_empty() {
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

        let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
            Some(rom) => rom,
            None => {
                move_to_trash(connection, progress_bar, system_directory, romfile_path).await?;
                return Ok(());
            }
        };

        let mut new_name = OsString::from(&rom.name);
        new_name.push(".");
        new_name.push(&romfile_extension);
        let new_path = system_directory.as_ref().join(&new_name);

        // move file inside archive if needed
        if sevenzip_info.path != rom.name {
            rename_file_in_archive(progress_bar, romfile_path, &sevenzip_info.path, &rom.name)?;
        }

        // move archive if needed
        move_file(progress_bar, romfile_path, &new_path).await?;

        // persist in database
        create_or_update_romfile(connection, &new_path, &rom).await;

    // archive contains multiple files
    } else {
        let delete = true;
        for sevenzip_info in sevenzip_infos {
            let size: u64;
            let crc: String;

            let extracted_path = extract_files_from_archive(
                &progress_bar,
                romfile_path,
                &[&sevenzip_info.path],
                &tmp_directory.path(),
            )?
            .remove(0);

            // system has a header or crc is absent
            if header.is_some() || sevenzip_info.crc.is_empty() {
                let size_crc =
                    get_file_size_and_crc(progress_bar, &extracted_path, &header, 1, 1).await?;
                size = size_crc.0;
                crc = size_crc.1;
            } else {
                size = sevenzip_info.size;
                crc = sevenzip_info.crc.clone();
            }

            let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
                Some(rom) => rom,
                None => {
                    remove_file(&extracted_path).await?;
                    return Ok(());
                }
            };

            let mut new_path = system_directory.as_ref().join(&rom.name);
            new_path.push(".");
            new_path.push(&romfile_extension);

            // move file
            move_file(progress_bar, &extracted_path, &new_path).await?;

            // persist in database
            create_or_update_romfile(connection, &new_path, &rom).await;
        }

        // delete archive or trash archive
        if delete {
            remove_file(romfile_path).await?;
        } else {
            move_to_trash(connection, progress_bar, system_directory, romfile_path).await?;
        }
    }

    Ok(())
}

async fn import_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    system: &System,
    header: &Option<Header>,
    romfile_path: &P,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let mut cue_path = romfile_path.as_ref().to_path_buf();
    cue_path.set_extension(CUE_EXTENSION);

    if !cue_path.is_file().await {
        progress_bar.println(&format!("Missing {:?}", cue_path.file_name().unwrap()));
        return Ok(());
    }

    let (size, crc) = get_file_size_and_crc(progress_bar, &cue_path, &header, 1, 1).await?;
    let cue_rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
        Some(rom) => rom,
        None => {
            move_to_trash(connection, progress_bar, system_directory, &cue_path).await?;
            return Ok(());
        }
    };

    let roms: Vec<Rom> = find_roms_by_game_id(connection, cue_rom.game_id)
        .await
        .into_iter()
        .filter(|rom| rom.id != cue_rom.id)
        .collect();

    let names_sizes: Vec<(&str, u64)> = roms
        .iter()
        .map(|rom| (rom.name.as_str(), rom.size as u64))
        .collect();
    let bin_paths = extract_chd(
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
        progress_bar.println("CRC mismatch");
        move_to_trash(connection, progress_bar, system_directory, romfile_path).await?;
        return Ok(());
    }

    let new_cue_path = system_directory.as_ref().join(&cue_rom.name);
    let mut new_chd_path = new_cue_path.clone();
    new_chd_path.set_extension(CHD_EXTENSION);

    // move cue and chd if needed
    move_file(progress_bar, &cue_path, &new_cue_path).await?;
    move_file(progress_bar, romfile_path, &new_chd_path).await?;

    // persist in database
    create_or_update_romfile(connection, &new_cue_path, &cue_rom).await;
    for rom in roms {
        create_or_update_romfile(connection, &new_chd_path, &rom).await;
    }

    Ok(())
}

async fn import_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    system: &System,
    header: &Option<Header>,
    romfile_path: &P,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let iso_path = extract_cso(progress_bar, romfile_path, &tmp_directory.path())?;
    let (size, crc) = get_file_size_and_crc(progress_bar, &iso_path, &header, 1, 1).await?;
    remove_file(&iso_path).await?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
        Some(rom) => rom,
        None => {
            move_to_trash(connection, progress_bar, system_directory, romfile_path).await?;
            return Ok(());
        }
    };

    let mut new_cso_path = system_directory.as_ref().join(&rom.name);
    new_cso_path.set_extension(CSO_EXTENSION);

    // move CSO if needed
    move_file(progress_bar, romfile_path, &new_cso_path).await?;

    // persist in database
    create_or_update_romfile(connection, &new_cso_path, &rom).await;

    Ok(())
}

async fn import_other<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    system: &System,
    header: &Option<Header>,
    romfile_path: &P,
) -> SimpleResult<()> {
    let (size, crc) = get_file_size_and_crc(progress_bar, romfile_path, &header, 1, 1).await?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
        Some(rom) => rom,
        None => {
            move_to_trash(connection, progress_bar, system_directory, romfile_path).await?;
            return Ok(());
        }
    };

    let new_path = system_directory.as_ref().join(&rom.name);

    // move file if needed
    move_file(progress_bar, romfile_path, &new_path).await?;

    // persist in database
    create_or_update_romfile(connection, &new_path, &rom).await;

    Ok(())
}

async fn find_rom(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<Option<Rom>> {
    let rom: Rom;
    let mut roms = find_roms_by_size_and_crc_and_system_id(connection, size, crc, system.id).await;

    // abort if no match
    if roms.is_empty() {
        progress_bar.println("No match");
        return Ok(None);
    }

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        rom = roms.remove(0);
        progress_bar.println(&format!("Matches \"{}\"", rom.name));
    } else {
        rom = prompt_for_rom(&mut roms)?;
    }

    // abort if rom already has a file
    if rom.romfile_id.is_some() {
        let romfile = find_romfile_by_id(connection, rom.romfile_id.unwrap()).await;
        progress_bar.println(&format!("Duplicate of \"{}\"", romfile.path));
        return Ok(None);
    }

    Ok(Some(rom))
}

async fn create_or_update_romfile<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    path: &P,
    rom: &Rom,
) {
    let romfile_path = path.as_ref().as_os_str().to_str().unwrap();
    let romfile = find_romfile_by_path(connection, romfile_path).await;
    let romfile_id = match romfile {
        Some(romfile) => {
            update_romfile(connection, romfile.id, romfile_path).await;
            romfile.id
        }
        None => create_romfile(connection, romfile_path).await,
    };
    update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
}

async fn move_to_trash<P: AsRef<Path>, Q: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_directory: &Q,
    romfile_path: &P,
) -> SimpleResult<()> {
    let new_path = system_directory
        .as_ref()
        .join("Trash")
        .join(romfile_path.as_ref().file_name().unwrap());
    move_file(progress_bar, romfile_path, &new_path).await?;
    create_romfile(connection, new_path.as_os_str().to_str().unwrap()).await;
    Ok(())
}

async fn move_file<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    old_path: &P,
    new_path: &Q,
) -> SimpleResult<()> {
    if old_path.as_ref() != new_path.as_ref() {
        progress_bar.println(&format!(
            "Moving to {}",
            new_path.as_ref().to_string_lossy()
        ));
        rename_file(old_path, new_path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::*;
    use super::super::database::*;
    use super::super::import_dats;
    use super::*;
    use async_std::fs;
    use async_std::path::PathBuf;
    use async_std::sync::Mutex;
    use std::env;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_import_sevenzip_single() {
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
        let rom_path = tmp_directory.join("Test Game (USA, Europe).rom.7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom.7z"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        import_archive(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &None,
            &rom_path,
            &rom_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).rom.7z")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_zip_single() {
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
        let rom_path = tmp_directory.join("Test Game (USA, Europe).rom.zip");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom.zip"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        import_archive(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &None,
            &rom_path,
            &rom_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).rom.zip")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_chd() {
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
        let rom_path = tmp_directory.join("Test Game (USA, Europe).cue");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).cue"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        let rom_path = tmp_directory.join("Test Game (USA, Europe).chd");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).chd"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        import_chd(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &None,
            &rom_path,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).chd")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.game_id, game.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).cue")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_cso() {
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
        let rom_path = tmp_directory.join("Test Game (USA, Europe).cso");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).cso"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        import_cso(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &None,
            &rom_path,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe) (ISO)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).cso")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_other() {
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
        let rom_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        import_other(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &None,
            &rom_path,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_other_headered() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20210402) (Headered).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let rom_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Headered).rom"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);
        let header = find_header_by_system_id(&mut connection, system.id).await;

        // when
        import_other(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &rom_path,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let mut games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe) (Headered)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Headered).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe) (Headered).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }
}
