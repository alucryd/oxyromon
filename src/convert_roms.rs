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
}

pub async fn main<'a>(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'a>,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, matches.is_present("ALL")).await;
    let format = matches.value_of("FORMAT");

    let progress_bar = get_progress_bar(0, get_none_progress_style());

    for system in systems {
        progress_bar.println(&format!("Processing \"{}\"", system.name));

        let roms = find_roms_with_romfile_by_system_id(connection, system.id).await;
        let romfiles = find_romfiles_by_ids(
            connection,
            &roms.iter().map(|rom| rom.romfile_id.unwrap()).collect(),
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

            extract_files_from_archive(
                &archive_path,
                &vec![&rom.name],
                &tmp_path,
                &progress_bar,
            )?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(
                &archive_path,
                &vec![&rom.name],
                &tmp_path,
                &progress_bar,
            )?;
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

            extract_files_from_archive(
                &archive_path,
                &file_names,
                &tmp_path,
                &progress_bar,
            )?;
            remove_file(&archive_path).await?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => SEVENZIP_EXTENSION,
                ArchiveType::ZIP => ZIP_EXTENSION,
            });
            add_files_to_archive(
                &archive_path,
                &file_names,
                &tmp_path,
                &progress_bar,
            )?;
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
    let games =
        find_games_by_ids(connection, &others.keys().map(|game_id| *game_id).collect()).await;
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

            add_files_to_archive(&archive_path, &vec![&rom.name], &directory, &progress_bar)?;
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
