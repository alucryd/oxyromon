use super::chdman::*;
use super::crud::*;
use super::maxcso::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use clap::ArgMatches;
use diesel::pg::PgConnection;
use rayon::prelude::*;
use std::env;
use std::error::Error;
use std::fs;
use std::mem::drop;
use std::path::{Path, PathBuf};

pub fn convert_roms(connection: &PgConnection, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let systems = prompt_for_systems(&connection, matches.is_present("ALL"));
    let format = matches.value_of("FORMAT");
    let tmp_directory = Path::new(&env::var("TMP_DIRECTORY").unwrap()).canonicalize()?;

    for system in systems {
        println!("Processing {}", system.name);
        println!("");

        match format {
            Some("7Z") => to_archive(&connection, &system, &tmp_directory, ArchiveType::SEVENZIP)?,
            Some("CHD") => to_chd(&connection, &system)?,
            Some("CSO") => to_cso(&connection, &system)?,
            Some("ORIGINAL") => to_original(&connection, &system, &tmp_directory)?,
            Some("ZIP") => to_archive(&connection, &system, &tmp_directory, ArchiveType::ZIP)?,
            Some(_) => bail!("Not implemented"),
            None => bail!("Not possible"),
        }
    }

    Ok(())
}

fn to_archive(
    connection: &PgConnection,
    system: &System,
    tmp_directory: &PathBuf,
    archive_type: ArchiveType,
) -> Result<(), Box<dyn Error>> {
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

            extract_files_from_archive(&archive_path, &vec![&rom.name], tmp_directory)?;
            fs::remove_file(&archive_path)?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });
            add_files_to_archive(&archive_path, &vec![&rom.name], tmp_directory)?;
            fs::remove_file(tmp_directory.join(&rom.name))?;
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &archive_romfile_input);
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

            extract_files_from_archive(&archive_path, &file_names, tmp_directory)?;
            fs::remove_file(&archive_path)?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });
            add_files_to_archive(&archive_path, &file_names, tmp_directory)?;
            for file_name in file_names {
                fs::remove_file(tmp_directory.join(file_name))?;
                let archive_romfile_input = RomfileInput {
                    path: &String::from(archive_path.as_os_str().to_str().unwrap()),
                };
                update_romfile(connection, &archive_romfile, &archive_romfile_input);
            }
        }
    }

    // convert others
    for (game, mut roms_romfiles) in others {
        if roms_romfiles.len() == 1 {
            let (rom, romfile) = roms_romfiles.remove(0);
            let directory = Path::new(&romfile.path).parent().unwrap().to_path_buf();
            let mut archive_path = directory.join(&rom.name);
            archive_path.push(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });

            add_files_to_archive(&archive_path, &vec![&rom.name], &directory)?;
            fs::remove_file(directory.join(&rom.name))?;
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &archive_romfile_input);
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
            let mut archive_path = directory.join(&game.name);
            archive_path.push(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });

            add_files_to_archive(&archive_path, &file_names, &directory)?;
            for file_name in file_names {
                fs::remove_file(directory.join(file_name))?;
            }
            let archive_romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            let archive_romfile = create_romfile(connection, &archive_romfile_input);
            for rom in roms {
                update_rom_romfile(connection, &rom, &archive_romfile.id);
            }
            for romfile in romfiles {
                delete_romfile_by_id(connection, &romfile.id)
            }
        }
    }

    Ok(())
}

fn to_chd(connection: &PgConnection, system: &System) -> Result<(), Box<dyn Error>> {
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
        let chd_path = create_chd(&cue_path)?;
        let chd_romfile_input = RomfileInput {
            path: &String::from(chd_path.as_os_str().to_str().unwrap()),
        };
        let chd_romfile = create_romfile(connection, &chd_romfile_input);
        for (rom, romfile) in bin_roms_romfiles {
            update_rom_romfile(connection, &rom, &chd_romfile.id);
            delete_romfile_by_id(connection, &romfile.id);
            fs::remove_file(&romfile.path)?;
        }
    }

    Ok(())
}

fn to_cso(connection: &PgConnection, system: &System) -> Result<(), Box<dyn Error>> {
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
            let cso_path = create_cso(&iso_path, &directory)?;
            let cso_romfile_input = RomfileInput {
                path: &String::from(cso_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &iso_romfile, &cso_romfile_input);
        }
    }

    Ok(())
}

fn to_original(
    connection: &PgConnection,
    system: &System,
    tmp_directory: &PathBuf,
) -> Result<(), Box<dyn Error>> {
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
        extract_files_from_archive(&archive_path, &file_names, &directory)?;
        fs::remove_file(&archive_path)?;
        for rom in roms {
            let romfile_input = RomfileInput {
                path: &String::from(directory.join(&rom.name).as_os_str().to_str().unwrap()),
            };
            let romfile = create_romfile(connection, &romfile_input);
            update_rom_romfile(connection, &rom, &romfile.id);
        }
        delete_romfile_by_id(connection, &archive_romfile.id);
    }

    // convert CHDs
    for (_, roms_romfiles) in chds {
        let (mut cue_rom_romfile, chd_roms_romfiles): (Vec<(Rom, Romfile)>, Vec<(Rom, Romfile)>) =
            roms_romfiles
                .into_par_iter()
                .partition(|(rom, _)| rom.name.ends_with(".cue"));
        let (_, cue_romfile) = cue_rom_romfile.remove(0);
        let cue_path = Path::new(&cue_romfile.path).to_path_buf();

        let mut roms: Vec<Rom> = Vec::new();
        let mut romfiles: Vec<Romfile> = Vec::new();
        for (rom, romfile) in chd_roms_romfiles {
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
        extract_chd(
            &chd_path,
            &directory,
            &tmp_directory,
            &cue_path.file_name().unwrap().to_str().unwrap(),
            &file_names_sizes,
        )?;
        fs::remove_file(&chd_path)?;
        for rom in roms {
            let romfile_input = RomfileInput {
                path: &String::from(directory.join(&rom.name).as_os_str().to_str().unwrap()),
            };
            let romfile = create_romfile(connection, &romfile_input);
            update_rom_romfile(connection, &rom, &romfile.id);
        }
        delete_romfile_by_id(connection, &chd_romfile.id);
    }

    // convert CSOs
    for (_, roms_romfiles) in csos {
        for (_, cso_romfile) in roms_romfiles {
            let cso_path = Path::new(&cso_romfile.path).to_path_buf();
            let directory = cso_path.parent().unwrap();
            let iso_path = extract_cso(&cso_path, &directory)?;
            let iso_romfile_input = RomfileInput {
                path: &String::from(iso_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &cso_romfile, &iso_romfile_input);
        }
    }

    Ok(())
}
