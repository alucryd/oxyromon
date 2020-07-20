use super::chdman::*;
use super::crud::*;
use super::maxcso::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use diesel::SqliteConnection;
use indicatif::ProgressBar;
use rayon::prelude::*;
use std::ffi::OsString;
use std::mem::drop;
use std::path::{Path, PathBuf};

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

pub fn main<'a>(
    connection: &SqliteConnection,
    matches: &ArgMatches<'a>,
    tmp_directory: &PathBuf,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(&connection, matches.is_present("ALL"));
    let format = matches.value_of("FORMAT");

    let progress_bar = get_progress_bar(0, get_none_progress_style());

    for system in systems {
        progress_bar.println(&format!("Processing \"{}\"", system.name));

        match format {
            Some("7Z") => to_archive(
                &connection,
                &system,
                &tmp_directory,
                &progress_bar,
                ArchiveType::SEVENZIP,
            )?,
            Some("CHD") => to_chd(&connection, &system, &progress_bar)?,
            Some("CSO") => to_cso(&connection, &system, &progress_bar)?,
            Some("ORIGINAL") => to_original(&connection, &system, &progress_bar)?,
            Some("ZIP") => to_archive(
                &connection,
                &system,
                &tmp_directory,
                &progress_bar,
                ArchiveType::ZIP,
            )?,
            Some(_) => bail!("Not implemented"),
            None => bail!("Not possible"),
        }
    }

    Ok(())
}

fn to_archive(
    connection: &SqliteConnection,
    system: &System,
    tmp_directory: &PathBuf,
    progress_bar: &ProgressBar,
    archive_type: ArchiveType,
) -> SimpleResult<()> {
    let mut games_roms_romfiles: Vec<(Game, Vec<(Rom, Romfile)>)> =
        find_games_roms_romfiles_with_romfile_by_system(connection, &system);

    // remove same type archives, CHDs and CSOs
    games_roms_romfiles.retain(|(_, roms_romfiles)| {
        roms_romfiles.par_iter().any(|(_, romfile)| {
            !(romfile.path.ends_with(match archive_type {
                ArchiveType::SEVENZIP => ".7z",
                ArchiveType::ZIP => ".zip",
            }) || romfile.path.ends_with(".chd")
                || romfile.path.ends_with(".cso"))
        })
    });

    // partition archives
    let (archives, others): (
        Vec<(Game, Vec<(Rom, Romfile)>)>,
        Vec<(Game, Vec<(Rom, Romfile)>)>,
    ) = games_roms_romfiles
        .into_par_iter()
        .partition(|(_, roms_romfiles)| {
            roms_romfiles.par_iter().any(|(_, romfile)| {
                romfile.path.ends_with(match archive_type {
                    ArchiveType::SEVENZIP => ".zip",
                    ArchiveType::ZIP => ".7z",
                })
            })
        });

    // convert archives
    for (_, mut roms_romfiles) in archives {
        if roms_romfiles.len() == 1 {
            let (rom, romfile) = roms_romfiles.remove(0);
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(
                &archive_path,
                &vec![&rom.name],
                tmp_directory,
                &progress_bar,
            )?;
            remove_file(&archive_path)?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });
            add_files_to_archive(
                &archive_path,
                &vec![&rom.name],
                tmp_directory,
                &progress_bar,
            )?;
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &archive_romfile_input);
            remove_file(&tmp_directory.join(&rom.name))?;
        } else {
            let mut roms: Vec<Rom> = Vec::new();
            let mut romfiles: Vec<Romfile> = Vec::new();
            for (rom, romfile) in roms_romfiles {
                roms.push(rom);
                romfiles.push(romfile);
            }
            romfiles.dedup();

            if romfiles.len() > 1 {
                bail!("Multiple archives found");
            }

            let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let archive_romfile = romfiles.remove(0);
            let mut archive_path = Path::new(&archive_romfile.path).to_path_buf();

            extract_files_from_archive(&archive_path, &file_names, tmp_directory, &progress_bar)?;
            remove_file(&archive_path)?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });
            add_files_to_archive(&archive_path, &file_names, tmp_directory, &progress_bar)?;
            for file_name in file_names {
                let archive_romfile_input = RomfileInput {
                    path: &String::from(archive_path.as_os_str().to_str().unwrap()),
                };
                update_romfile(connection, &archive_romfile, &archive_romfile_input);
                remove_file(&tmp_directory.join(file_name))?;
            }
        }
    }

    // convert others
    for (game, mut roms_romfiles) in others {
        if roms_romfiles.len() == 1 {
            let (rom, romfile) = roms_romfiles.remove(0);
            let directory = Path::new(&romfile.path).parent().unwrap().to_path_buf();
            let mut archive_name = OsString::from(&rom.name);
            archive_name.push(match archive_type {
                ArchiveType::SEVENZIP => ".7z",
                ArchiveType::ZIP => ".zip",
            });
            let archive_path = directory.join(archive_name);

            add_files_to_archive(&archive_path, &vec![&rom.name], &directory, &progress_bar)?;
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &archive_romfile_input);
            remove_file(&directory.join(&rom.name))?;
        } else {
            let mut roms: Vec<Rom> = Vec::new();
            let mut romfiles: Vec<Romfile> = Vec::new();
            for (rom, romfile) in roms_romfiles {
                roms.push(rom);
                romfiles.push(romfile);
            }
            let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let directory = Path::new(&romfiles.get(0).unwrap().path)
                .parent()
                .unwrap()
                .to_path_buf();
            let mut archive_name = OsString::from(&game.name);
            archive_name.push(match archive_type {
                ArchiveType::SEVENZIP => ".7z",
                ArchiveType::ZIP => ".zip",
            });
            let archive_path = directory.join(archive_name);

            add_files_to_archive(&archive_path, &file_names, &directory, progress_bar)?;
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            let archive_romfile_id = create_romfile(connection, &archive_romfile_input);
            for rom in &roms {
                update_rom_romfile(connection, &rom, archive_romfile_id);
            }
            for romfile in romfiles {
                delete_romfile_by_id(connection, romfile.id)
            }
            for file_name in file_names {
                remove_file(&directory.join(file_name))?;
            }
        }
    }

    Ok(())
}

fn to_chd(
    connection: &SqliteConnection,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let mut games_roms_romfiles: Vec<(Game, Vec<(Rom, Romfile)>)> =
        find_games_roms_romfiles_with_romfile_by_system(connection, &system);

    // keep CUE/BIN only
    games_roms_romfiles.retain(|(_, roms_romfiles)| {
        roms_romfiles
            .par_iter()
            .any(|(_, romfile)| romfile.path.ends_with(".cue"))
            && roms_romfiles
                .par_iter()
                .any(|(_, romfile)| romfile.path.ends_with(".bin"))
    });

    for (_, roms_romfiles) in games_roms_romfiles {
        let (mut cue_rom_romfile, bin_roms_romfiles): (Vec<(Rom, Romfile)>, Vec<(Rom, Romfile)>) =
            roms_romfiles
                .into_par_iter()
                .partition(|(rom, _)| rom.name.ends_with(".cue"));
        let (_, cue_romfile) = cue_rom_romfile.remove(0);
        let cue_path = Path::new(&cue_romfile.path).to_path_buf();
        let chd_path = create_chd(&cue_path, &progress_bar)?;
        let chd_romfile_input = RomfileInput {
            path: &String::from(chd_path.as_os_str().to_str().unwrap()),
        };
        let chd_romfile_id = create_romfile(connection, &chd_romfile_input);
        for (bin_rom, bin_romfile) in bin_roms_romfiles {
            update_rom_romfile(connection, &bin_rom, chd_romfile_id);
            delete_romfile_by_id(connection, bin_romfile.id);
            remove_file(&Path::new(&bin_romfile.path).to_path_buf())?;
        }
    }

    Ok(())
}

fn to_cso(
    connection: &SqliteConnection,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let mut games_roms_romfiles: Vec<(Game, Vec<(Rom, Romfile)>)> =
        find_games_roms_romfiles_with_romfile_by_system(connection, &system);

    // keep ISO only
    games_roms_romfiles.retain(|(_, roms_romfiles)| {
        roms_romfiles
            .par_iter()
            .any(|(_, romfile)| romfile.path.ends_with(".iso"))
    });

    for (_, roms_romfiles) in games_roms_romfiles {
        for (_, iso_romfile) in roms_romfiles {
            let iso_path = Path::new(&iso_romfile.path).to_path_buf();
            let directory = iso_path.parent().unwrap();
            let cso_path = create_cso(&iso_path, &directory, progress_bar)?;
            let cso_romfile_input = RomfileInput {
                path: &String::from(cso_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &iso_romfile, &cso_romfile_input);
            remove_file(&iso_path)?;
        }
    }

    Ok(())
}

fn to_original(
    connection: &SqliteConnection,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let games_roms_romfiles: Vec<(Game, Vec<(Rom, Romfile)>)> =
        find_games_roms_romfiles_with_romfile_by_system(connection, &system);

    // partition archives
    let (archives, others): (
        Vec<(Game, Vec<(Rom, Romfile)>)>,
        Vec<(Game, Vec<(Rom, Romfile)>)>,
    ) = games_roms_romfiles
        .into_par_iter()
        .partition(|(_, roms_romfiles)| {
            roms_romfiles
                .par_iter()
                .any(|(_, romfile)| romfile.path.ends_with(".zip") || romfile.path.ends_with(".7z"))
        });

    // partition CHDs
    let (chds, others): (
        Vec<(Game, Vec<(Rom, Romfile)>)>,
        Vec<(Game, Vec<(Rom, Romfile)>)>,
    ) = others.into_par_iter().partition(|(_, roms_romfiles)| {
        roms_romfiles
            .par_iter()
            .any(|(_, romfile)| romfile.path.ends_with(".chd"))
    });

    // partition CSOs
    let (csos, others): (
        Vec<(Game, Vec<(Rom, Romfile)>)>,
        Vec<(Game, Vec<(Rom, Romfile)>)>,
    ) = others.into_par_iter().partition(|(_, roms_romfiles)| {
        roms_romfiles
            .par_iter()
            .any(|(_, romfile)| romfile.path.ends_with(".cso"))
    });

    // drop originals
    drop(others);

    // convert archives
    for (_, roms_romfiles) in archives {
        let mut roms: Vec<Rom> = Vec::new();
        let mut romfiles: Vec<Romfile> = Vec::new();
        for (rom, romfile) in roms_romfiles {
            roms.push(rom);
            romfiles.push(romfile);
        }
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let archive_romfile = romfiles.remove(0);
        let archive_path = Path::new(&archive_romfile.path).to_path_buf();
        let directory = archive_path.parent().unwrap();
        let file_names: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
        let extracted_paths =
            extract_files_from_archive(&archive_path, &file_names, &directory, &progress_bar)?;
        let roms_extracted_paths: Vec<(Rom, PathBuf)> =
            roms.into_iter().zip(extracted_paths).collect();
        for (rom, extracted_path) in roms_extracted_paths {
            let romfile_input = RomfileInput {
                path: &String::from(extracted_path.as_os_str().to_str().unwrap()),
            };
            let romfile_id = create_romfile(connection, &romfile_input);
            update_rom_romfile(connection, &rom, romfile_id);
        }
        delete_romfile_by_id(connection, archive_romfile.id);
        remove_file(&archive_path)?;
    }

    // convert CHDs
    for (_, mut roms_romfiles) in chds {
        // we don't need the cue sheet
        roms_romfiles.retain(|(rom, _)| rom.name.ends_with(".chd"));

        let mut roms: Vec<Rom> = Vec::new();
        let mut romfiles: Vec<Romfile> = Vec::new();
        for (rom, romfile) in roms_romfiles {
            roms.push(rom);
            romfiles.push(romfile);
        }
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple CHDs found");
        }

        let chd_romfile = romfiles.remove(0);
        let chd_path = Path::new(&chd_romfile.path).to_path_buf();
        let directory = chd_path.parent().unwrap();
        let file_names_sizes: Vec<(&str, u64)> = roms
            .iter()
            .map(|rom| (rom.name.as_str(), rom.size as u64))
            .collect();
        extract_chd(&chd_path, &directory, &file_names_sizes, &progress_bar)?;
        for rom in roms {
            let romfile_input = RomfileInput {
                path: &String::from(directory.join(&rom.name).as_os_str().to_str().unwrap()),
            };
            let romfile_id = create_romfile(connection, &romfile_input);
            update_rom_romfile(connection, &rom, romfile_id);
        }
        delete_romfile_by_id(connection, chd_romfile.id);
        remove_file(&chd_path)?;
    }

    // convert CSOs
    for (_, roms_romfiles) in csos {
        for (_, cso_romfile) in roms_romfiles {
            let cso_path = Path::new(&cso_romfile.path).to_path_buf();
            let directory = cso_path.parent().unwrap();
            let iso_path = extract_cso(
                &cso_path,
                &directory,
                &get_progress_bar(0, get_bytes_progress_style()),
            )?;
            let iso_romfile_input = RomfileInput {
                path: &String::from(iso_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &cso_romfile, &iso_romfile_input);
            remove_file(&cso_path)?;
        }
    }

    Ok(())
}
