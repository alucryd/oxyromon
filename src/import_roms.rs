use super::chdman::*;
use super::checksum::*;
use super::config::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::PathBuf;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::ffi::OsString;

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
}

pub async fn main<'a>(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'a>,
) -> SimpleResult<()> {
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    let roms: Vec<String> = matches.values_of_lossy("ROMS").unwrap();
    let system = prompt_for_system(connection, &progress_bar).await;

    let header = find_header_by_system_id(connection, system.id).await;

    let system_directory = get_rom_directory(connection).await.join(&system.name);
    create_directory(&system_directory).await?;

    for rom in roms {
        let rom_path = get_canonicalized_path(&rom).await?;
        import_rom(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )
        .await?;
    }

    Ok(())
}

pub async fn import_rom(
    connection: &mut SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let rom_extension = rom_path
        .extension()
        .unwrap()
        .to_str()
        .unwrap()
        .to_lowercase();

    progress_bar.println(&format!("Processing {:?}", rom_path.file_name().unwrap()));

    if ARCHIVE_EXTENSIONS.contains(&rom_extension.as_str()) {
        import_archive(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &rom_extension,
            &progress_bar,
        )
        .await?;
    } else if CHD_EXTENSION == rom_extension {
        import_chd(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )
        .await?;
    } else if CSO_EXTENSION == rom_extension {
        import_cso(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )
        .await?;
    } else {
        import_other(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )
        .await?;
    }

    Ok(())
}

async fn import_archive(
    connection: &mut SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    rom_extension: &str,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let tmp_path = PathBuf::from(&tmp_directory.path());
    let sevenzip_infos = parse_archive(rom_path, &progress_bar)?;

    // archive contains a single file
    if sevenzip_infos.len() == 1 {
        let size: u64;
        let crc: String;
        let sevenzip_info = sevenzip_infos.get(0).unwrap();

        // system has a header or crc is absent
        if header.is_some() || sevenzip_info.crc == "" {
            let extracted_path = extract_files_from_archive(
                rom_path,
                &vec![&sevenzip_info.path],
                &tmp_path,
                &progress_bar,
            )?
            .remove(0);
            let size_crc =
                get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1).await?;
            remove_file(&extracted_path).await?;
            size = size_crc.0;
            crc = size_crc.1;
        } else {
            size = sevenzip_info.size;
            crc = sevenzip_info.crc.clone();
        }

        let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await {
            Some(rom) => rom,
            None => return Ok(()),
        };

        let mut new_name = OsString::from(&rom.name);
        new_name.push(".");
        new_name.push(&rom_extension);
        let new_path = system_directory.join(&new_name);

        // move file inside archive if needed
        if sevenzip_info.path != rom.name {
            rename_file_in_archive(rom_path, &sevenzip_info.path, &rom.name, &progress_bar)?;
        }

        // move archive if needed
        move_file(rom_path, &new_path, &progress_bar).await?;

        // persist in database
        create_or_update_romfile(connection, &new_path, &rom).await;

    // archive contains multiple files
    } else {
        for sevenzip_info in sevenzip_infos {
            let size: u64;
            let crc: String;

            let extracted_path = extract_files_from_archive(
                rom_path,
                &vec![&sevenzip_info.path],
                &tmp_path,
                &progress_bar,
            )?
            .remove(0);

            // system has a header or crc is absent
            if header.is_some() || sevenzip_info.crc == "" {
                let size_crc =
                    get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1).await?;
                size = size_crc.0;
                crc = size_crc.1;
            } else {
                size = sevenzip_info.size;
                crc = sevenzip_info.crc.clone();
            }

            let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await {
                Some(rom) => rom,
                None => {
                    remove_file(&extracted_path).await?;
                    return Ok(());
                }
            };

            let mut new_path = system_directory.join(&rom.name);
            new_path.push(".");
            new_path.push(&rom_extension);

            // move file
            move_file(&extracted_path, &new_path, &progress_bar).await?;

            // persist in database
            create_or_update_romfile(connection, &new_path, &rom).await;
        }

        // delete archive
        remove_file(rom_path).await?;
    }

    Ok(())
}

async fn import_chd(
    connection: &mut SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let tmp_path = PathBuf::from(&tmp_directory.path());
    let mut cue_path = rom_path.clone();
    cue_path.set_extension(CUE_EXTENSION);

    if !cue_path.is_file().await {
        progress_bar.println(&format!("Missing {:?}", cue_path.file_name().unwrap()));
        return Ok(());
    }

    let (size, crc) = get_file_size_and_crc(&cue_path, &header, &progress_bar, 1, 1).await?;
    let cue_rom = match find_rom(connection, size, &crc, &system, &progress_bar).await {
        Some(rom) => rom,
        None => return Ok(()),
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
    let bin_paths = extract_chd(rom_path, &tmp_path, &names_sizes, &progress_bar).await?;
    let mut crcs: Vec<String> = Vec::new();
    for (i, bin_path) in bin_paths.iter().enumerate() {
        let (_, crc) =
            get_file_size_and_crc(&bin_path, &header, &progress_bar, i, bin_paths.len()).await?;
        crcs.push(crc);
        remove_file(&bin_path).await?;
    }

    if roms.iter().enumerate().any(|(i, rom)| crcs[i] != rom.crc) {
        progress_bar.println("CRC mismatch");
        return Ok(());
    }

    let new_meta_path = system_directory.join(&cue_rom.name);
    let mut new_file_path = new_meta_path.clone();
    new_file_path.set_extension(CHD_EXTENSION);

    // move cue and chd if needed
    move_file(&cue_path, &new_meta_path, &progress_bar).await?;
    move_file(rom_path, &new_file_path, &progress_bar).await?;

    // persist in database
    create_or_update_romfile(connection, &new_meta_path, &cue_rom).await;
    for rom in roms {
        create_or_update_romfile(connection, &new_file_path, &rom).await;
    }

    Ok(())
}

async fn import_cso(
    connection: &mut SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let tmp_path = PathBuf::from(&tmp_directory.path());
    let iso_path = extract_cso(rom_path, &tmp_path, &progress_bar)?;
    let (size, crc) = get_file_size_and_crc(&iso_path, &header, &progress_bar, 1, 1).await?;
    remove_file(&iso_path).await?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await {
        Some(rom) => rom,
        None => return Ok(()),
    };

    let mut new_file_path = system_directory.join(&rom.name);
    new_file_path.set_extension(CSO_EXTENSION);

    // move CSO if needed
    move_file(rom_path, &new_file_path, &progress_bar).await?;

    // persist in database
    create_or_update_romfile(connection, &new_file_path, &rom).await;

    Ok(())
}

async fn import_other(
    connection: &mut SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let (size, crc) = get_file_size_and_crc(rom_path, &header, &progress_bar, 1, 1).await?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await {
        Some(rom) => rom,
        None => return Ok(()),
    };

    let new_path = system_directory.join(&rom.name);

    // move file if needed
    move_file(rom_path, &new_path, &progress_bar).await?;

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
) -> Option<Rom> {
    let rom: Rom;
    let mut roms = find_roms_by_size_and_crc_and_system_id(connection, size, crc, system.id).await;

    // abort if no match
    if roms.is_empty() {
        progress_bar.println("No match");
        return None;
    }

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        rom = roms.remove(0);
        progress_bar.println(&format!("Matches \"{}\"", rom.name));
    } else {
        rom = prompt_for_rom(&mut roms, progress_bar).await;
    }

    // abort if rom already has a file
    if rom.romfile_id.is_some() {
        let romfile = find_romfile_by_id(connection, rom.romfile_id.unwrap()).await;
        if romfile.is_some() {
            let romfile = romfile.unwrap();
            progress_bar.println(&format!("Duplicate of \"{}\"", romfile.path));
            return None;
        }
    }

    Some(rom)
}

pub async fn create_or_update_romfile(
    connection: &mut SqliteConnection,
    path: &PathBuf,
    rom: &Rom,
) {
    let romfile_path = path.as_os_str().to_str().unwrap();
    let romfile = find_romfile_by_path(connection, romfile_path).await;
    let romfile_id = match romfile {
        Some(romfile) => {
            update_romfile(connection, romfile.id, romfile_path).await;
            romfile.id
        }
        None => create_romfile(connection, romfile_path).await,
    };
    update_rom_romfile(connection, rom.id, romfile_id).await;
}

async fn move_file(
    old_path: &PathBuf,
    new_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if old_path != new_path {
        progress_bar.println(&format!("Moving to {}", new_path.to_string_lossy()));
        rename_file(old_path, new_path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::MUTEX;
    use super::super::database::*;
    use super::super::embedded;
    use super::super::import_dats::import_dat;
    use super::*;
    use async_std::fs;
    use async_std::path::Path;
    use async_std::sync::Mutex;
    use refinery::config::{Config, ConfigDbType};
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_import_sevenzip_single() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).rom.7z");
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
            &system_path,
            &system,
            &None,
            &rom_path,
            &rom_path.extension().unwrap().to_str().unwrap(),
            &progress_bar,
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
            &roms.iter().map(|rom| rom.game_id).collect(),
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
            system_path
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
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).rom.zip");
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
            &system_path,
            &system,
            &None,
            &rom_path,
            &rom_path.extension().unwrap().to_str().unwrap(),
            &progress_bar,
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
            &roms.iter().map(|rom| rom.game_id).collect(),
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
            system_path
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
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).cue");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).cue"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).chd");
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
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
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
            &roms.iter().map(|rom| rom.game_id).collect(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe) (CUE/BIN)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_path
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
            system_path
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
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).cso");
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
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
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
            &roms.iter().map(|rom| rom.game_id).collect(),
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
            system_path
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
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).rom");
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
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
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
            &roms.iter().map(|rom| rom.game_id).collect(),
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
            system_path
                .join("Test Game (USA, Europe).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }
}
