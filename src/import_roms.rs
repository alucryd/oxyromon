use super::chdman::*;
use super::checksum::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
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
        progress_bar.println("");
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

    let mut transaction = begin_transaction(connection).await;

    // abort if the romfile is already in the database
    if find_romfile_by_path(
        &mut transaction,
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
            &mut transaction,
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
            &mut transaction,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    } else if CSO_EXTENSION == romfile_extension {
        import_cso(
            &mut transaction,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    } else {
        import_other(
            &mut transaction,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
        )
        .await?;
    }

    // mark games and system as complete if they are
    update_games_by_system_id_mark_complete(&mut transaction, system.id).await;
    update_system_mark_complete(&mut transaction, system.id).await;

    commit_transaction(transaction).await;

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

    let mut roms_sevenzip_infos: Vec<(Rom, &ArchiveInfo, Option<PathBuf>)> = Vec::new();
    let mut game_ids: HashSet<i64> = HashSet::new();

    for sevenzip_info in &sevenzip_infos {
        let size: u64;
        let crc: String;
        let mut extracted_path = None;

        // system has a header or crc is absent
        if header.is_some() || sevenzip_info.crc.is_empty() {
            extracted_path = Some(
                extract_files_from_archive(
                    &progress_bar,
                    romfile_path,
                    &[&sevenzip_info.path],
                    &tmp_directory.path(),
                )?
                .remove(0),
            );

            let size_crc = get_file_size_and_crc(
                progress_bar,
                extracted_path.as_ref().unwrap(),
                &header,
                1,
                1,
            )
            .await?;
            size = size_crc.0;
            crc = size_crc.1;
        } else {
            size = sevenzip_info.size;
            crc = sevenzip_info.crc.clone();
        }

        match find_rom(connection, size, &crc, &system, &progress_bar).await? {
            Some(rom) => {
                game_ids.insert(rom.game_id);
                roms_sevenzip_infos.push((rom, sevenzip_info, extracted_path));
            }
            None => {
                if sevenzip_infos.len() == 1 {
                    if let Some(extracted_path) = extracted_path {
                        remove_file(&extracted_path).await?;
                    }
                    move_to_trash(connection, progress_bar, system, romfile_path).await?;
                } else {
                    if let Some(extracted_path) = extracted_path {
                        move_to_trash(connection, progress_bar, system, &extracted_path).await?;
                    }
                    delete_files_from_archive(
                        progress_bar,
                        romfile_path,
                        &vec![sevenzip_info.path.as_str()],
                    )?;
                }
            }
        }
    }

    // archive contains a single valid file
    if roms_sevenzip_infos.len() == 1 {
        let (rom, sevenzip_info, _) = roms_sevenzip_infos.remove(0);

        let new_path = system_directory
            .as_ref()
            .join(format!("{}.{}", &rom.name, &romfile_extension));

        // move file inside archive if needed
        if sevenzip_info.path != rom.name {
            rename_file_in_archive(progress_bar, romfile_path, &sevenzip_info.path, &rom.name)?;
        }

        // move archive if needed
        rename_file(progress_bar, romfile_path, &new_path).await?;

        // persist in database
        create_or_update_romfile(connection, &new_path, &vec![rom]).await;

    // archive contains multiple valid files
    } else if roms_sevenzip_infos.len() > 1 {
        // archive contains a full single game
        if game_ids.len() == 1 {
            let game_id = game_ids.drain().last().unwrap();
            let rom_ids: HashSet<i64> = find_roms_by_game_id(connection, game_id)
                .await
                .into_par_iter()
                .map(|rom| rom.id)
                .collect();
            // archive contains a full game
            if rom_ids
                .difference(
                    &roms_sevenzip_infos
                        .par_iter()
                        .map(|(rom, _, _)| rom.id)
                        .collect(),
                )
                .collect::<Vec<&i64>>()
                .len()
                == 0
            {
                let game = find_game_by_id(connection, game_id).await;
                for (rom, sevenzip_info, _) in &roms_sevenzip_infos {
                    if &sevenzip_info.path != &rom.name {
                        rename_file_in_archive(
                            progress_bar,
                            romfile_path,
                            &sevenzip_info.path,
                            &rom.name,
                        )?;
                    }
                }

                let new_path = system_directory
                    .as_ref()
                    .join(format!("{}.{}", &game.name, &romfile_extension));

                // move file
                rename_file(progress_bar, romfile_path, &new_path).await?;

                // persist in database
                create_or_update_romfile(
                    connection,
                    &new_path,
                    &roms_sevenzip_infos
                        .into_iter()
                        .map(|(rom, _, _)| rom)
                        .collect::<Vec<Rom>>(),
                )
                .await;

                return Ok(());
            }
        }

        // archive contains a partial game or multiple games
        for (rom, sevenzip_info, extracted_path) in roms_sevenzip_infos {
            let extracted_path = match extracted_path {
                Some(extracted_path) => extracted_path,
                None => extract_files_from_archive(
                    &progress_bar,
                    romfile_path,
                    &[&sevenzip_info.path],
                    &tmp_directory.path(),
                )?
                .remove(0),
            };

            let new_path = system_directory.as_ref().join(&rom.name);

            // move file
            rename_file(progress_bar, &extracted_path, &new_path).await?;

            // persist in database
            create_or_update_romfile(connection, &new_path, &vec![rom]).await;
        }

        // delete archive
        remove_file(romfile_path).await?;
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

    if cue_path.is_file().await {
        progress_bar.println("CUE file found, using multiple tracks mode");
        let (size, crc) = get_file_size_and_crc(progress_bar, &cue_path, &header, 1, 1).await?;
        let cue_rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
            Some(rom) => rom,
            None => {
                move_to_trash(connection, progress_bar, system, &cue_path).await?;
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
            progress_bar.println("CRC mismatch");
            move_to_trash(connection, progress_bar, system, romfile_path).await?;
            return Ok(());
        }

        let new_cue_path = system_directory.as_ref().join(&cue_rom.name);
        let mut new_chd_path = new_cue_path.clone();
        new_chd_path.set_extension(CHD_EXTENSION);

        // move cue and chd if needed
        rename_file(progress_bar, &cue_path, &new_cue_path).await?;
        rename_file(progress_bar, romfile_path, &new_chd_path).await?;

        // persist in database
        create_or_update_romfile(connection, &new_cue_path, &vec![cue_rom]).await;
        create_or_update_romfile(connection, &new_chd_path, &roms).await;

        Ok(())
    } else {
        progress_bar.println("CUE file not found, using single track mode");
        let bin_path =
            extract_chd_to_single_track(progress_bar, romfile_path, &tmp_directory.path()).await?;
        let (size, crc) = get_file_size_and_crc(progress_bar, &bin_path, &header, 1, 1).await?;
        remove_file(&bin_path).await?;
        let rom = match find_rom(connection, size, &crc, &system, &progress_bar).await? {
            Some(rom) => rom,
            None => {
                move_to_trash(connection, progress_bar, system, romfile_path).await?;
                return Ok(());
            }
        };

        let mut new_chd_path = system_directory.as_ref().join(&rom.name);
        new_chd_path.set_extension(CHD_EXTENSION);

        // move CHD if needed
        rename_file(progress_bar, romfile_path, &new_chd_path).await?;

        // persist in database
        create_or_update_romfile(connection, &new_chd_path, &vec![rom]).await;

        Ok(())
    }
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
            move_to_trash(connection, progress_bar, system, romfile_path).await?;
            return Ok(());
        }
    };

    let mut new_cso_path = system_directory.as_ref().join(&rom.name);
    new_cso_path.set_extension(CSO_EXTENSION);

    // move CSO if needed
    rename_file(progress_bar, romfile_path, &new_cso_path).await?;

    // persist in database
    create_or_update_romfile(connection, &new_cso_path, &vec![rom]).await;

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
            move_to_trash(connection, progress_bar, system, romfile_path).await?;
            return Ok(());
        }
    };

    let new_path = system_directory.as_ref().join(&rom.name);

    // move file if needed
    rename_file(progress_bar, romfile_path, &new_path).await?;

    // persist in database
    create_or_update_romfile(connection, &new_path, &vec![rom]).await;

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
    romfile_path: &P,
    roms: &[Rom],
) {
    let romfile = find_romfile_by_path(
        connection,
        romfile_path.as_ref().as_os_str().to_str().unwrap(),
    )
    .await;
    let romfile_id = match romfile {
        Some(romfile) => {
            update_romfile(
                connection,
                romfile.id,
                &romfile.path,
                romfile_path.as_ref().metadata().await.unwrap().len(),
            )
            .await;
            romfile.id
        }
        None => {
            create_romfile(
                connection,
                romfile_path.as_ref().as_os_str().to_str().unwrap(),
                romfile_path.as_ref().metadata().await.unwrap().len(),
            )
            .await
        }
    };
    for rom in roms {
        update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
    }
}

async fn move_to_trash<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    romfile_path: &P,
) -> SimpleResult<()> {
    let new_path = get_trash_directory(connection, system)
        .await?
        .join(romfile_path.as_ref().file_name().unwrap());
    rename_file(progress_bar, romfile_path, &new_path).await?;
    create_romfile(
        connection,
        new_path.as_os_str().to_str().unwrap(),
        new_path.metadata().await.unwrap().len(),
    )
    .await;
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
    use std::env;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_import_sevenzip_single_file() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
    async fn test_import_sevenzip_single_file_with_header() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom.7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Headered).rom.7z"),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);
        let header = find_header_by_system_id(&mut connection, system.id).await;

        // when
        import_archive(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 1);
        assert_eq!(
            sevenzip_infos.get(0).unwrap().path,
            "Test Game (USA, Europe).rom"
        );
    }

    #[async_std::test]
    async fn test_import_sevenzip_multiple_files_full_game() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Full).7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Full).7z"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
        assert_eq!(game.system_id, system.id);

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe) (CUE BIN).7z")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.get(2).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_sevenzip_multiple_files_partial_game() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Partial).7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Partial).7z"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 2);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
        assert_eq!(game.system_id, system.id);

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe) (Track 01).bin")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let romfile = romfiles.get(1).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).cue")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_sevenzip_multiple_files_mixed_games() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Mixed).7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Mixed).7z"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 2);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 2);

        let game = games.get(1).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (Beta)");
        assert_eq!(game.system_id, system.id);

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe) (Beta).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Beta).rom");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let romfile = romfiles.get(1).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_import_sevenzip_multiple_files_mixed_games_with_header() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Mixed) (Headered).7z");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Mixed) (Headered).7z"),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);
        let header = find_header_by_system_id(&mut connection, system.id).await;

        // when
        import_archive(
            &mut connection,
            &progress_bar,
            &system_directory,
            &system,
            &header,
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).rom.7z")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 1);
        assert_eq!(
            sevenzip_infos.get(0).unwrap().path,
            "Test Game (USA, Europe).rom"
        );

        let romfile = romfiles.get(1).unwrap();
        assert!(romfile.path.contains("/Trash/"));
    }

    #[async_std::test]
    async fn test_import_zip_single_file() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
            &romfile_path.extension().unwrap().to_str().unwrap(),
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
    async fn test_import_chd_single_track() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (ISO)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
    }

    #[async_std::test]
    async fn test_import_chd_multiple_tracks() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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

        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.game_id, game.id);

        let rom = roms.get(2).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(1).unwrap();
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
    async fn test_import_chd_multiple_tracks_without_cue_should_fail() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert!(roms.is_empty());
    }

    #[async_std::test]
    async fn test_import_cso() {
        // given
        let _guard = MUTEX.lock().await;

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
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe) (ISO)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Headered).rom"),
            &romfile_path.as_os_str().to_str().unwrap(),
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
            &romfile_path,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);
        let games = find_games_by_ids(
            &mut connection,
            &roms
                .iter()
                .map(|rom| rom.game_id)
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;
        assert_eq!(games.len(), 1);

        let game = games.get(0).unwrap();
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(game.system_id, system.id);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.get(0).unwrap();
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
}
