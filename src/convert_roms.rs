use super::chdman::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::{HumanBytes, ProgressBar};
use rayon::prelude::*;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::mem::drop;

const FORMATS: &[&str] = &["7Z", "CHD", "CSO", "ORIGINAL", "ZIP"];

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
                .possible_values(FORMATS),
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
        .arg(
            Arg::with_name("STATISTICS")
                .short("s")
                .long("statistics")
                .help("Prints statistics for each conversion")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, matches.is_present("ALL")).await?;
    let rom_name = matches.value_of("NAME");
    let format = match matches.value_of("FORMAT") {
        Some(format) => format,
        None => FORMATS.get(select(FORMATS, None)?).unwrap(),
    };
    let statistics = matches.is_present("STATISTICS");

    for system in systems {
        progress_bar.println(&format!("Processing \"{}\"", system.name));

        let roms = match rom_name {
            Some(rom_name) => {
                let roms =
                    find_roms_with_romfile_by_system_id_and_name(connection, system.id, rom_name)
                        .await;
                prompt_for_roms(roms, matches.is_present("ALL"))?
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
            "7Z" => {
                to_archive(
                    connection,
                    progress_bar,
                    ArchiveType::SEVENZIP,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            "CHD" => {
                to_chd(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    statistics,
                )
                .await?
            }
            "CSO" => {
                to_cso(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    statistics,
                )
                .await?
            }
            "ORIGINAL" => {
                to_original(connection, progress_bar, roms_by_game_id, romfiles_by_id).await?
            }
            "ZIP" => {
                to_archive(
                    connection,
                    progress_bar,
                    ArchiveType::ZIP,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            _ => bail!("Not supported"),
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

    // remove same type archives
    roms_by_game_id.retain(|_, roms| {
        roms.par_iter().any(|rom| {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            !(romfile.path.ends_with(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            }))
        })
    });

    // partition CHDs
    let (chds, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
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
                    .ends_with(CSO_EXTENSION)
            })
        });

    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(match archive_type {
                        ArchiveType::SEVENZIP => ZIP_EXTENSION,
                        ArchiveType::ZIP => SEVENZIP_EXTENSION,
                    })
            })
        });

    // convert CHDs
    for roms in chds.values() {
        if roms.len() == 1 {
            let rom = roms.get(0).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let bin_path =
                extract_chd_to_single_track(progress_bar, &romfile.path, &tmp_directory.path())
                    .await?;
            let archive_path = Path::new(&romfile.path).parent().unwrap().join(format!(
                "{}.{}",
                &rom.name,
                match archive_type {
                    ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                    ArchiveType::ZIP => ZIP_EXTENSION,
                }
            ));
            add_files_to_archive(
                progress_bar,
                &archive_path,
                &[&bin_path.file_name().unwrap().to_str().unwrap()],
                &tmp_directory.path(),
            )?;
            update_romfile(
                connection,
                romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&bin_path).await?;
            remove_file(&romfile.path).await?;
        } else {
            let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                .into_par_iter()
                .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
            let cue_rom = cue_roms.get(0).unwrap();
            let cue_romfile = romfiles_by_id.get(&cue_rom.romfile_id.unwrap()).unwrap();
            let chd_romfile = romfiles_by_id
                .get(&bin_roms.get(0).unwrap().romfile_id.unwrap())
                .unwrap();

            let mut archive_path = Path::new(&chd_romfile.path).to_path_buf();
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });

            let bin_names_sizes: Vec<(&str, u64)> = bin_roms
                .iter()
                .map(|rom| (rom.name.as_str(), rom.size as u64))
                .collect();
            let bin_paths = extract_chd_to_multiple_tracks(
                progress_bar,
                &chd_romfile.path,
                &tmp_directory.path(),
                &bin_names_sizes,
            )
            .await?;

            add_files_to_archive(
                progress_bar,
                &archive_path,
                &[&cue_rom.name],
                &archive_path.parent().unwrap(),
            )?;
            let bin_names: Vec<&str> = bin_paths
                .iter()
                .map(|p| p.file_name().unwrap().to_str().unwrap())
                .collect();
            add_files_to_archive(
                progress_bar,
                &archive_path,
                &bin_names,
                &tmp_directory.path(),
            )?;
            update_romfile(
                connection,
                chd_romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            update_rom_romfile(connection, cue_rom.id, Some(chd_romfile.id)).await;
            delete_romfile_by_id(connection, cue_romfile.id).await;

            for bin_path in bin_paths {
                remove_file(&bin_path).await?;
            }
            remove_file(&cue_romfile.path).await?;
            remove_file(&chd_romfile.path).await?;
        }
    }

    // convert CSOs
    for roms in csos.values() {
        let rom = roms.get(0).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let iso_path = extract_cso(progress_bar, &romfile.path, &tmp_directory.path())?;
        let archive_path = Path::new(&romfile.path).parent().unwrap().join(format!(
            "{}.{}",
            &rom.name,
            match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            }
        ));

        add_files_to_archive(
            progress_bar,
            &archive_path,
            &[&iso_path.file_name().unwrap().to_str().unwrap()],
            &tmp_directory.path(),
        )?;
        update_romfile(
            connection,
            romfile.id,
            archive_path.as_os_str().to_str().unwrap(),
        )
        .await;
        remove_file(&iso_path).await?;
        remove_file(&romfile.path).await?;
    }

    // convert archives
    for roms in archives.values() {
        if roms.len() == 1 {
            let rom = roms.get(0).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(
                progress_bar,
                &archive_path,
                &[&rom.name],
                &tmp_directory.path(),
            )?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(
                progress_bar,
                &archive_path,
                &[&rom.name],
                &tmp_directory.path(),
            )?;
            update_romfile(
                connection,
                romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&tmp_directory.path().join(&rom.name)).await?;
        } else {
            let mut romfiles: Vec<&Romfile> = roms
                .par_iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect();
            romfiles.dedup();

            if romfiles.len() > 1 {
                bail!("Multiple archives found");
            }

            let rom_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let romfile = romfiles.get(0).unwrap();
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(
                progress_bar,
                &archive_path,
                &rom_names,
                &tmp_directory.path(),
            )?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(
                progress_bar,
                &archive_path,
                &rom_names,
                &tmp_directory.path(),
            )?;
            for rom_name in rom_names {
                update_romfile(
                    connection,
                    romfile.id,
                    archive_path.as_os_str().to_str().unwrap(),
                )
                .await;
                remove_file(&tmp_directory.path().join(rom_name)).await?;
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
            let directory = Path::new(&romfile.path).parent().unwrap();
            let archive_path = directory.join(format!(
                "{}.{}",
                &rom.name,
                match archive_type {
                    ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                    ArchiveType::ZIP => ZIP_EXTENSION,
                }
            ));

            add_files_to_archive(progress_bar, &archive_path, &[&rom.name], &directory)?;
            update_romfile(
                connection,
                romfile.id,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&romfile.path).await?;
        } else {
            let game = games_by_id.get(&game_id).unwrap();
            let rom_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let directory = Path::new(
                &romfiles_by_id
                    .get(&roms.get(0).unwrap().romfile_id.unwrap())
                    .unwrap()
                    .path,
            )
            .parent()
            .unwrap();
            let archive_path = directory.join(format!(
                "{}.{}",
                &game.name,
                match archive_type {
                    ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                    ArchiveType::ZIP => ZIP_EXTENSION,
                }
            ));

            add_files_to_archive(progress_bar, &archive_path, &rom_names, &directory)?;
            let archive_romfile_id =
                create_romfile(connection, archive_path.as_os_str().to_str().unwrap()).await;
            for rom in &roms {
                delete_romfile_by_id(connection, rom.romfile_id.unwrap()).await;
                update_rom_romfile(connection, rom.id, Some(archive_romfile_id)).await;
            }
            for rom_name in rom_names {
                remove_file(&directory.join(rom_name)).await?;
            }
        }
    }

    Ok(())
}

async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    statistics: bool,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;

    // partition CUE/BINs
    let (cue_bins, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CUE_EXTENSION)
            }) && roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(BIN_EXTENSION)
            })
        });

    // partition ISOs
    let (isos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ISO_EXTENSION)
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
                    .ends_with(CSO_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // convert CUE/BIN
    for roms in cue_bins.values() {
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .into_par_iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.get(0).unwrap().romfile_id.unwrap())
            .unwrap();
        let chd_path = create_chd(
            progress_bar,
            &cue_romfile.path,
            &Path::new(&cue_romfile.path).parent().unwrap(),
        )?;
        if statistics {
            let roms = [cue_roms.as_slice(), bin_roms.as_slice()].concat();
            let mut romfile_paths = romfiles_by_id
                .iter()
                .filter(|(k, _)| {
                    bin_roms
                        .iter()
                        .map(|r| r.romfile_id.unwrap())
                        .collect::<Vec<i64>>()
                        .contains(k)
                })
                .map(|(_, v)| &v.path)
                .collect::<Vec<&String>>();
            romfile_paths.push(&cue_romfile.path);
            print_statistics(progress_bar, &roms, &romfile_paths, &[&chd_path]).await?;
        }
        let chd_romfile_id =
            create_romfile(connection, chd_path.as_os_str().to_str().unwrap()).await;
        for bin_rom in bin_roms {
            let bin_romfile = romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap();
            update_rom_romfile(connection, bin_rom.id, Some(chd_romfile_id)).await;
            delete_romfile_by_id(connection, bin_romfile.id).await;
            remove_file(&bin_romfile.path).await?;
        }
    }

    // convert ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let chd_path = create_chd(
                progress_bar,
                &romfile.path,
                &Path::new(&romfile.path).parent().unwrap(),
            )?;
            if statistics {
                print_statistics(progress_bar, &[&rom], &[&romfile.path], &[&chd_path]).await?;
            }
            update_romfile(
                connection,
                romfile.id,
                chd_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&romfile.path).await?;
        }
    }

    // convert CSOs
    for roms in csos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_path = extract_cso(progress_bar, &romfile.path, &tmp_directory.path())?;
            let chd_path = create_chd(
                progress_bar,
                &iso_path,
                &Path::new(&romfile.path).parent().unwrap(),
            )?;
            if statistics {
                print_statistics(progress_bar, &[&rom], &[&romfile.path], &[&chd_path]).await?;
            }
            update_romfile(
                connection,
                romfile.id,
                chd_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&iso_path).await?;
            remove_file(&romfile.path).await?;
        }
    }

    Ok(())
}

async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    statistics: bool,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;

    // partition ISOs
    let (isos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ISO_EXTENSION)
            })
        });

    // partition CHDs
    let (chds, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.len() == 1
                && roms.par_iter().any(|rom| {
                    romfiles_by_id
                        .get(&rom.romfile_id.unwrap())
                        .unwrap()
                        .path
                        .ends_with(CHD_EXTENSION)
                })
        });

    // drop others
    drop(others);

    // process ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let cso_path = create_cso(
                progress_bar,
                &romfile.path,
                &Path::new(&romfile.path).parent().unwrap(),
            )?;
            if statistics {
                print_statistics(progress_bar, &[&rom], &[&romfile.path], &[&cso_path]).await?;
            }
            update_romfile(
                connection,
                romfile.id,
                cso_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&romfile.path).await?;
        }
    }

    // process CHDs
    for roms in chds.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_path =
                extract_chd_to_single_track(progress_bar, &romfile.path, &tmp_directory.path())
                    .await?;
            let cso_path = create_cso(
                progress_bar,
                &iso_path,
                &Path::new(&romfile.path).parent().unwrap(),
            )?;
            if statistics {
                print_statistics(progress_bar, &[&rom], &[&romfile.path], &[&cso_path]).await?;
            }
            update_romfile(
                connection,
                romfile.id,
                cso_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&iso_path).await?;
            remove_file(&romfile.path).await?;
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
                    .ends_with(CHD_EXTENSION)
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
    for roms in archives.values() {
        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let romfile = romfiles.get(0).unwrap();
        let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();

        let extracted_paths = extract_files_from_archive(
            progress_bar,
            &romfile.path,
            &file_names,
            &Path::new(&romfile.path).parent().unwrap(),
        )?;
        let roms_extracted_paths: Vec<(&Rom, PathBuf)> =
            roms.into_iter().zip(extracted_paths).collect();

        for (rom, extracted_path) in roms_extracted_paths {
            let romfile_id =
                create_romfile(connection, extracted_path.as_os_str().to_str().unwrap()).await;
            update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
        }
        delete_romfile_by_id(connection, romfile.id).await;
        remove_file(&romfile.path).await?;
    }

    // convert CHDs
    for (_, mut roms) in chds {
        // we don't need the cue sheet
        roms.retain(|rom| rom.name.ends_with(BIN_EXTENSION) || rom.name.ends_with(ISO_EXTENSION));

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple CHDs found");
        }

        let romfile = romfiles.get(0).unwrap();
        let directory = &Path::new(&romfile.path).parent().unwrap();
        let bin_names_sizes: Vec<(&str, u64)> = roms
            .iter()
            .map(|rom| (rom.name.as_str(), rom.size as u64))
            .collect();

        extract_chd_to_multiple_tracks(progress_bar, &romfile.path, &directory, &bin_names_sizes)
            .await?;

        for rom in roms {
            let romfile_id = create_romfile(
                connection,
                directory.join(&rom.name).as_os_str().to_str().unwrap(),
            )
            .await;
            update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
        }
        delete_romfile_by_id(connection, romfile.id).await;
        remove_file(&romfile.path).await?;
    }

    // convert CSOs
    for roms in csos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_path = extract_cso(
                progress_bar,
                &romfile.path,
                &Path::new(&romfile.path).parent().unwrap(),
            )?;
            update_romfile(
                connection,
                romfile.id,
                iso_path.as_os_str().to_str().unwrap(),
            )
            .await;
            remove_file(&romfile.path).await?;
        }
    }

    Ok(())
}

async fn print_statistics<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    roms: &[&Rom],
    old_files: &[&P],
    new_files: &[&Q],
) -> SimpleResult<()> {
    let original_size = roms.iter().map(|&r| r.size as u64).sum();
    let mut old_size = 0u64;
    for &old_file in old_files {
        old_size += try_with!(
            old_file.as_ref().metadata().await,
            "Failed to read file metadata"
        )
        .len();
    }
    let mut new_size = 0u64;
    for &new_file in new_files {
        new_size += try_with!(
            new_file.as_ref().metadata().await,
            "Failed to read file metadata"
        )
        .len();
    }
    progress_bar.println(&format!(
        "Before: {} ({:.1}%); After: {} ({:.1}%); Original: {}",
        HumanBytes(old_size),
        old_size as f64 / original_size as f64 * 100f64,
        HumanBytes(new_size),
        new_size as f64 / original_size as f64 * 100f64,
        HumanBytes(original_size)
    ));
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::*;
    use async_std::fs;
    use std::env;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_original_to_zip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 1);
    }

    #[async_std::test]
    async fn test_original_to_zip_with_correct_name() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        let matches =
            subcommand().get_matches_from(&["convert-roms", "-f", "ZIP", "-n", "test game", "-a"]);

        // when
        main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 1);
    }

    #[async_std::test]
    async fn test_original_to_zip_with_incorrect_name() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        let matches =
            subcommand().get_matches_from(&["convert-roms", "-f", "ZIP", "-n", "test gqme", "-a"]);

        // when
        main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_original_to_sevenzip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    }

    #[async_std::test]
    async fn test_single_track_chd_to_sevenzip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).iso.7z")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 1);
    }

    #[async_std::test]
    async fn test_multiple_tracks_chd_to_sevenzip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(roms[0].game_id, roms);

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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).7z")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));
        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));
        let rom = roms.get(2).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let sevenzip_infos = parse_archive(&progress_bar, &romfile.path).unwrap();
        assert_eq!(sevenzip_infos.len(), 3);
    }

    #[async_std::test]
    async fn test_cso_to_sevenzip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).iso.7z")
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
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_zip_to_original() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_sevenzip_to_zip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_zip_to_sevenzip() {
        // given
        let _guard = MUTEX.lock().await;

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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");

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
    async fn test_iso_to_cso() {
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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).iso");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).iso"),
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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

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
    async fn test_single_track_chd_to_cso() {
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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

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
    async fn test_multiple_tracks_chd_to_cso_should_do_nothing() {
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

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(roms[0].game_id, roms);

        // when
        to_cso(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);

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
    }

    #[async_std::test]
    async fn test_cso_to_iso() {
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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).iso")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }

    #[async_std::test]
    async fn test_cso_to_chd() {
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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_chd(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

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
    async fn test_cue_bin_to_chd() {
        // given
        let _guard = MUTEX.lock().await;

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

        let mut romfile_paths: Vec<PathBuf> = Vec::new();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
            &romfile_path,
        )
        .await
        .unwrap();
        romfile_paths.push(romfile_path);
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Track 01).bin");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Track 01).bin"),
            &romfile_path,
        )
        .await
        .unwrap();
        romfile_paths.push(romfile_path);
        let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Track 02).bin");
        fs::copy(
            test_directory.join("Test Game (USA, Europe) (Track 02).bin"),
            &romfile_path,
        )
        .await
        .unwrap();
        romfile_paths.push(romfile_path);

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_path in romfile_paths {
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(roms[0].game_id, roms);

        // when
        to_chd(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 2);

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

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));
        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.get(2).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");

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
    async fn test_chd_to_cue_bin() {
        // given
        let _guard = MUTEX.lock().await;

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

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 3);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 3);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");

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
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.get(1).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");

        let romfile = romfiles.get(1).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe) (Track 02).bin")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));

        let rom = roms.get(2).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).cue");

        let romfile = romfiles.get(2).unwrap();
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
    async fn test_iso_to_chd() {
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
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).iso");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).iso"),
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

        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);

        // when
        to_chd(
            &mut connection,
            &progress_bar,
            roms_by_game_id,
            romfiles_by_id,
            true,
        )
        .await
        .unwrap();

        // then
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

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
    async fn test_chd_to_iso() {
        // given
        let _guard = MUTEX.lock().await;

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

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
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
        let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
        assert_eq!(roms.len(), 1);
        let romfiles = find_romfiles(&mut connection).await;
        assert_eq!(romfiles.len(), 1);

        let rom = roms.get(0).unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).iso");

        let romfile = romfiles.get(0).unwrap();
        assert_eq!(
            romfile.path,
            system_directory
                .join("Test Game (USA, Europe).iso")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file().await);
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }
}
