use super::chdman::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use rayon::prelude::*;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::ffi::OsString;
use std::mem::drop;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("convert-roms")
        .about("Converts ROM files between common formats")
        .arg(
            Arg::with_name("FORMAT")
                .short("f")
                .long("format")
                .help("Sets the destination format")
                .required(false)
                .takes_value(true)
                .possible_values(&["7Z", "CHD", "CSO", "ORIGINAL", "ZIP"]),
        )
        .arg(
            Arg::with_name("NAME")
                .short("n")
                .long("name")
                .help("Selects ROMs by name")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Converts all systems/all ROMs")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, matches.is_present("ALL"), &progress_bar).await;
    let format = matches.value_of("FORMAT");
    let rom_name = matches.value_of("NAME");

    for system in systems {
        progress_bar.println(&format!("Processing \"{}\"", system.name));

        let roms = match rom_name {
            Some(rom_name) => {
                let roms =
                    find_roms_with_romfile_by_system_id_and_name(connection, system.id, rom_name)
                        .await;
                prompt_for_roms(roms, matches.is_present("ALL"), &progress_bar).await
            }
            None => find_roms_with_romfile_by_system_id(connection, system.id).await,
        };

        if roms.is_empty() {
            if matches.is_present("NAME") {
                progress_bar.println(&format!("No ROM matching \"{}\"", rom_name.unwrap()));
            }
            continue;
        }

        let romfiles = find_romfiles_by_ids(
            connection,
            &roms
                .iter()
                .map(|rom| rom.romfile_id.unwrap())
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms.into_iter().for_each(|rom| {
            let group = roms_by_game_id.entry(rom.game_id).or_insert(vec![]);
            group.push(rom);
        });
        let romfiles_by_id: HashMap<i64, Romfile> = romfiles
            .into_iter()
            .map(|romfile| (romfile.id, romfile))
            .collect();

        match format {
            Some("7Z") => {
                to_archive(
                    connection,
                    &progress_bar,
                    ArchiveType::SEVENZIP,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            Some("CHD") => {
                to_chd(connection, &progress_bar, roms_by_game_id, romfiles_by_id).await?
            }
            Some("CSO") => {
                to_cso(connection, &progress_bar, roms_by_game_id, romfiles_by_id).await?
            }
            Some("ORIGINAL") => {
                to_original(connection, &progress_bar, roms_by_game_id, romfiles_by_id).await?
            }
            Some("ZIP") => {
                to_archive(
                    connection,
                    &progress_bar,
                    ArchiveType::ZIP,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            Some(_) => bail!("Not implemented"),
            None => bail!("Not possible"),
        }
    }

    Ok(())
}

async fn to_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    archive_type: ArchiveType,
    mut roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let tmp_path = PathBuf::from(&tmp_directory.path());

    // remove same type archives, CHDs and CSOs
    roms_by_game_id.retain(|_, roms| {
        roms.par_iter().any(|rom| {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            !(romfile.path.ends_with(match archive_type {
                ArchiveType::SEVENZIP => ".7z",
                ArchiveType::ZIP => ".zip",
            }) || romfile.path.ends_with(".chd")
                || romfile.path.ends_with(".cso"))
        })
    });

    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(match archive_type {
                        ArchiveType::SEVENZIP => ".zip",
                        ArchiveType::ZIP => ".7z",
                    })
            })
        });

    // convert archives
    for roms in archives.values() {
        if roms.len() == 1 {
            let rom = roms.get(0).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(&archive_path, &[&rom.name], &tmp_path, &progress_bar)?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(&archive_path, &[&rom.name], &tmp_path, &progress_bar)?;
            update_romfile(
                connection,
                romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&tmp_path.join(&rom.name)).await?;
        } else {
            let mut romfiles: Vec<&Romfile> = roms
                .par_iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect();
            romfiles.dedup();

            if romfiles.len() > 1 {
                bail!("Multiple archives found");
            }

            let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let romfile = romfiles.get(0).unwrap();
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(&archive_path, &file_names, &tmp_path, &progress_bar)?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(&archive_path, &file_names, &tmp_path, &progress_bar)?;
            for file_name in file_names {
                update_romfile(
                    connection,
                    romfile.id,
                    archive_path.as_os_str().to_str().unwrap(),
                )
                .await;
                remove_file(&tmp_path.join(file_name)).await?;
            }
        }
    }

    // convert others
    let games = find_games_by_ids(
        connection,
        &others.keys().copied().collect::<Vec<i64>>().as_slice(),
    )
    .await;
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();

    for (game_id, roms) in others {
        if roms.len() == 1 {
            let rom = roms.get(0).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let directory = Path::new(&romfile.path).parent().unwrap().to_path_buf();
            let mut archive_name = OsString::from(&rom.name);
            archive_name.push(match archive_type {
                ArchiveType::SEVENZIP => ".7z",
                ArchiveType::ZIP => ".zip",
            });
            let archive_path = directory.join(archive_name);

            add_files_to_archive(&archive_path, &[&rom.name], &directory, &progress_bar)?;
            update_romfile(
                connection,
                romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&directory.join(&rom.name)).await?;
        } else {
            let game = games_by_id.get(&game_id).unwrap();
            let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let directory = Path::new(
                &romfiles_by_id
                    .get(&roms.get(0).unwrap().romfile_id.unwrap())
                    .unwrap()
                    .path,
            )
            .parent()
            .unwrap()
            .to_path_buf();
            let mut archive_name = OsString::from(&game.name);
            archive_name.push(".");
            archive_name.push(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            let archive_path = directory.join(archive_name);

            add_files_to_archive(&archive_path, &file_names, &directory, progress_bar)?;
            let archive_romfile_id =
                create_romfile(connection, archive_path.as_os_str().to_str().unwrap()).await;
            for rom in &roms {
                delete_romfile_by_id(connection, rom.romfile_id.unwrap()).await;
                update_rom_romfile(connection, rom.id, archive_romfile_id).await;
            }
            for file_name in file_names {
                remove_file(&directory.join(file_name)).await?;
            }
        }
    }

    Ok(())
}

async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    mut roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
) -> SimpleResult<()> {
    // keep CUE/BIN only
    roms_by_game_id.retain(|_, roms| {
        roms.par_iter().any(|rom| {
            romfiles_by_id
                .get(&rom.romfile_id.unwrap())
                .unwrap()
                .path
                .ends_with(".cue")
        }) && roms.par_iter().any(|rom| {
            romfiles_by_id
                .get(&rom.romfile_id.unwrap())
                .unwrap()
                .path
                .ends_with(".bin")
        })
    });

    for (_, roms) in roms_by_game_id {
        let (cue_roms, bin_roms): (Vec<Rom>, Vec<Rom>) = roms
            .into_par_iter()
            .partition(|rom| rom.name.ends_with(".cue"));
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.get(0).unwrap().romfile_id.unwrap())
            .unwrap();
        let cue_path = Path::new(&cue_romfile.path).to_path_buf();
        let chd_path = create_chd(&cue_path, &progress_bar)?;
        let chd_romfile_id =
            create_romfile(connection, chd_path.as_os_str().to_str().unwrap()).await;
        for bin_rom in bin_roms {
            let bin_romfile = romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap();
            update_rom_romfile(connection, bin_rom.id, chd_romfile_id).await;
            delete_romfile_by_id(connection, bin_romfile.id).await;
            remove_file(&Path::new(&bin_romfile.path).to_path_buf()).await?;
        }
    }

    Ok(())
}

async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    mut roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
) -> SimpleResult<()> {
    // keep ISO only
    roms_by_game_id.retain(|_, roms| {
        roms.par_iter().any(|rom| {
            romfiles_by_id
                .get(&rom.romfile_id.unwrap())
                .unwrap()
                .path
                .ends_with(".iso")
        })
    });

    for (_, roms) in roms_by_game_id {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_path = Path::new(&romfile.path).to_path_buf();
            let directory = iso_path.parent().unwrap();
            let cso_path = create_cso(&iso_path, &directory, progress_bar)?;
            update_romfile(
                connection,
                romfile.id,
                cso_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&iso_path).await?;
        }
    }

    Ok(())
}

async fn to_original(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(".zip") || romfile.path.ends_with(".7z")
            })
        });

    // partition CHDs
    let (chds, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(".chd")
            })
        });

    // partition CSOs
    let (csos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(".cso")
            })
        });

    // drop originals
    drop(others);

    // convert archives
    for (_, roms) in archives {
        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
        let romfile = romfiles.get(0).unwrap();
        let archive_path = Path::new(&romfile.path).to_path_buf();
        let directory = archive_path.parent().unwrap();

        let extracted_paths =
            extract_files_from_archive(&archive_path, &file_names, &directory, &progress_bar)?;
        let roms_extracted_paths: Vec<(Rom, PathBuf)> =
            roms.into_iter().zip(extracted_paths).collect();

        for (rom, extracted_path) in roms_extracted_paths {
            let romfile_id =
                create_romfile(connection, extracted_path.as_os_str().to_str().unwrap()).await;
            update_rom_romfile(connection, rom.id, romfile_id).await;
        }
        delete_romfile_by_id(connection, romfile.id).await;
        remove_file(&archive_path).await?;
    }

    // convert CHDs
    for (_, mut roms) in chds {
        // we don't need the cue sheet
        roms.retain(|rom| rom.name.ends_with(".bin"));

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple CHDs found");
        }

        let chd_romfile = romfiles.get(0).unwrap();
        let chd_path = Path::new(&chd_romfile.path).to_path_buf();
        let directory = chd_path.parent().unwrap();
        let file_names_sizes: Vec<(&str, u64)> = roms
            .iter()
            .map(|rom| (rom.name.as_str(), rom.size as u64))
            .collect();

        extract_chd(&chd_path, &directory, &file_names_sizes, &progress_bar).await?;

        for rom in roms {
            let romfile_id = create_romfile(
                connection,
                directory.join(&rom.name).as_os_str().to_str().unwrap(),
            )
            .await;
            update_rom_romfile(connection, rom.id, romfile_id).await;
        }
        delete_romfile_by_id(connection, chd_romfile.id).await;
        remove_file(&chd_path).await?;
    }

    // convert CSOs
    for roms in csos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let cso_path = Path::new(&romfile.path).to_path_buf();
            let directory = cso_path.parent().unwrap();
            let iso_path = extract_cso(
                &cso_path,
                &directory,
                &get_progress_bar(0, get_bytes_progress_style()),
            )?;
            update_romfile(
                connection,
                romfile.id,
                iso_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&cso_path).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::embedded;
    use super::super::import_dats::import_dat;
    use super::super::import_roms::import_rom;
    use super::*;
    use async_std::fs;
    use async_std::path::Path;
    use async_std::sync::Mutex;
    use refinery::config::{Config, ConfigDbType};
    use std::env;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_original_to_zip() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_archive(
            &mut connection,
            &progress_bar,
            ArchiveType::ZIP,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_original_to_zip_with_correct_name() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        let matches = subcommand().get_matches_from(vec![
            "convert-roms",
            "-f",
            "ZIP",
            "-n",
            "test game",
            "-a",
        ]);

        // when
        main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_original_to_zip_with_incorrect_name() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        let matches = subcommand().get_matches_from(vec![
            "convert-roms",
            "-f",
            "ZIP",
            "-n",
            "test gqme",
            "-a",
        ]);

        // when
        main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    #[async_std::test]
    async fn test_original_to_sevenzip() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_archive(
            &mut connection,
            &progress_bar,
            ArchiveType::SEVENZIP,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_sevenzip_to_original() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_original(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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

    #[async_std::test]
    async fn test_zip_to_original() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_original(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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

    #[async_std::test]
    async fn test_sevenzip_to_zip() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_archive(
            &mut connection,
            &progress_bar,
            ArchiveType::ZIP,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_zip_to_sevenzip() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_archive(
            &mut connection,
            &progress_bar,
            ArchiveType::SEVENZIP,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_iso_to_cso() {
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
        let rom_path = tmp_path.join("Test Game (USA, Europe).iso");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).iso"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_cso(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

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
    async fn test_cso_to_iso() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap())
            .await
            .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_original(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_path
                .join("Test Game (USA, Europe).iso")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_cue_bin_to_chd() {
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

        let mut rom_paths: Vec<PathBuf> = Vec::new();
        let rom_path = tmp_path.join("Test Game (USA, Europe).cue");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).cue"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        rom_paths.push(rom_path);
        let rom_path = tmp_path.join("Test Game (USA, Europe) (Track 01).bin");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Track 01).bin"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        rom_paths.push(rom_path);
        let rom_path = tmp_path.join("Test Game (USA, Europe) (Track 02).bin");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Track 02).bin"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        rom_paths.push(rom_path);

        let system = find_systems(&mut connection).await.remove(0);

        for rom_path in rom_paths {
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &rom_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap())
                .await
                .unwrap();
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(roms[0].game_id, roms);

        // when
        to_chd(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);

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

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));
        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");

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
    async fn test_chd_to_cue_bin() {
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

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap())
                .await
                .unwrap();
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(roms[0].game_id, roms);

        // when
        to_original(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
        )
        .await
        .unwrap();

        // then
        let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let mut romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 3);

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_path
                .join("Test Game (USA, Europe) (Track 01).bin")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");

        let romfile = romfiles.remove(0);
        assert_eq!(
            romfile.path,
            system_path
                .join("Test Game (USA, Europe) (Track 02).bin")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");

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
}
