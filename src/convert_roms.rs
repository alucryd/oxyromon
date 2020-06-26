use super::crud::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use clap::ArgMatches;
use diesel::pg::PgConnection;
use rayon::prelude::*;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

pub fn convert_roms(connection: &PgConnection, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let systems = prompt_for_systems(&connection, matches.is_present("ALL"));
    let format = matches.value_of("FORMAT");
    let tmp_directory = Path::new("/tmp").to_path_buf();

    for system in systems {
        println!("Processing {}", system.name);
        println!("");

        match format {
            Some("7Z") => to_archive(&connection, &system, &tmp_directory, ArchiveType::SEVENZIP)?,
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
            let romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &romfile_input);
        } else {
            let mut roms: Vec<Rom> = Vec::new();
            let mut romfiles: Vec<Romfile> = Vec::new();
            for (rom, romfile) in roms_romfiles {
                roms.push(rom);
                romfiles.push(romfile);
            }
            let paths: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let romfile = romfiles.remove(0);
            let mut archive_path = Path::new(&romfile.path).to_path_buf();

            extract_files_from_archive(&archive_path, &paths, tmp_directory)?;
            fs::remove_file(&archive_path)?;
            archive_path.set_extension(match archive_type {
                ArchiveType::SEVENZIP => "7z",
                ArchiveType::ZIP => "zip",
            });
            add_files_to_archive(&archive_path, &paths, tmp_directory)?;
            for path in paths {
                fs::remove_file(tmp_directory.join(path))?;
                let romfile_input = RomfileInput {
                    path: &String::from(archive_path.as_os_str().to_str().unwrap()),
                };
                update_romfile(connection, &romfile, &romfile_input);
            }
        }
    }

    // convert others
    for (game, mut roms_romfiles) in others {
        if roms_romfiles.len() == 1 {
            let (rom, romfile) = roms_romfiles.remove(0);
            let directory = Path::new(&romfile.path).parent().unwrap().to_path_buf();
            let archive_path = directory.join(&format!(
                "{}.{}",
                &rom.name,
                match archive_type {
                    ArchiveType::SEVENZIP => "7z",
                    ArchiveType::ZIP => "zip",
                }
            ));

            add_files_to_archive(&archive_path, &vec![&rom.name], &directory)?;
            fs::remove_file(directory.join(&rom.name))?;
            let romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            update_romfile(connection, &romfile, &romfile_input);
        } else {
            let mut roms: Vec<Rom> = Vec::new();
            let mut romfiles: Vec<Romfile> = Vec::new();
            for (rom, romfile) in roms_romfiles {
                roms.push(rom);
                romfiles.push(romfile);
            }
            let paths: Vec<&str> = roms.par_iter().map(|rom| rom.name.as_str()).collect();
            let directory = Path::new(&romfiles.get(0).unwrap().path)
                .parent()
                .unwrap()
                .to_path_buf();
            let archive_path = directory.join(&format!(
                "{}.{}",
                &game.name,
                match archive_type {
                    ArchiveType::SEVENZIP => "7z",
                    ArchiveType::ZIP => "zip",
                }
            ));

            add_files_to_archive(&archive_path, &paths, &directory)?;
            for path in paths {
                fs::remove_file(directory.join(path))?;
            }
            let romfile_input = RomfileInput {
                path: &String::from(archive_path.as_os_str().to_str().unwrap()),
            };
            let romfile = create_romfile(connection, &romfile_input);
            for rom in roms {
                update_rom_romfile(connection, &rom, &romfile.id);
            }
            for romfile in romfiles {
                delete_romfile_by_id(connection, &romfile.id)
            }
        }
    }

    Ok(())
}
