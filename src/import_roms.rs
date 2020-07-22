use super::chdman::*;
use super::checksum::*;
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
use std::ffi::OsString;
use std::path::PathBuf;

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

pub fn main<'a>(
    connection: &SqliteConnection,
    matches: &ArgMatches<'a>,
    rom_directory: &PathBuf,
    tmp_directory: &PathBuf,
) -> SimpleResult<()> {
    let system = prompt_for_system(&connection);
    let header = find_header_by_system_id(&connection, system.id);

    let system_directory = rom_directory.join(&system.name);

    create_directory(&system_directory)?;

    let progress_bar = get_progress_bar(0, get_none_progress_style());

    for f in matches.values_of("ROMS").unwrap() {
        let file_path = get_canonicalized_path(f)?;
        let file_extension = file_path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase();

        progress_bar.println(&format!("Processing {:?}", file_path.file_name().unwrap()));

        if ARCHIVE_EXTENSIONS.contains(&file_extension.as_str()) {
            let sevenzip_infos = parse_archive(&file_path, &progress_bar)?;

            // archive contains a single file
            if sevenzip_infos.len() == 1 {
                let size: u64;
                let crc: String;
                let sevenzip_info = sevenzip_infos.get(0).unwrap();

                // system has a header or crc is absent
                if header.is_some() || sevenzip_info.crc == "" {
                    let extracted_path = extract_files_from_archive(
                        &file_path,
                        &vec![&sevenzip_info.path],
                        &tmp_directory,
                        &progress_bar,
                    )?
                    .remove(0);
                    let size_crc =
                        get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1)?;
                    remove_file(&extracted_path)?;
                    size = size_crc.0;
                    crc = size_crc.1;
                } else {
                    size = sevenzip_info.size;
                    crc = String::from(&sevenzip_info.crc);
                }

                let rom = match find_rom(&connection, size, &crc, &system, &progress_bar) {
                    Some(rom) => rom,
                    None => continue,
                };

                let mut new_name = OsString::from(&rom.name);
                new_name.push(".");
                new_name.push(&file_extension);
                let new_path = system_directory.join(new_name);

                // move file inside archive if needed
                if sevenzip_info.path != rom.name {
                    rename_file_in_archive(
                        &file_path,
                        &sevenzip_info.path,
                        &rom.name,
                        &progress_bar,
                    )?;
                }

                // move archive if needed
                move_file(&file_path, &new_path, &progress_bar)?;

                // persist in database
                create_or_update_file(&connection, &new_path, &rom);

            // archive contains multiple files
            } else {
                for sevenzip_info in sevenzip_infos {
                    let size: u64;
                    let crc: String;

                    let extracted_path = extract_files_from_archive(
                        &file_path,
                        &vec![&sevenzip_info.path],
                        &tmp_directory,
                        &progress_bar,
                    )?
                    .remove(0);

                    // system has a header or crc is absent
                    if header.is_some() || sevenzip_info.crc == "" {
                        let size_crc =
                            get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1)?;
                        size = size_crc.0;
                        crc = size_crc.1;
                    } else {
                        size = sevenzip_info.size;
                        crc = String::from(&sevenzip_info.crc);
                    }

                    let rom = match find_rom(&connection, size, &crc, &system, &progress_bar) {
                        Some(rom) => rom,
                        None => {
                            remove_file(&extracted_path)?;
                            continue;
                        }
                    };

                    let mut new_path = system_directory.join(&rom.name);
                    new_path.push(".");
                    new_path.push(&file_extension);

                    // move file
                    move_file(&extracted_path, &new_path, &progress_bar)?;

                    // persist in database
                    create_or_update_file(&connection, &new_path, &rom);
                }

                // delete archive
                remove_file(&file_path)?;
            }
        // file is a CHD
        } else if CHD_EXTENSION == file_extension {
            let mut cue_path = file_path.clone();
            cue_path.set_extension(CUE_EXTENSION);

            if !cue_path.is_file() {
                progress_bar.println(&format!("Missing {:?}", cue_path.file_name().unwrap()));
                continue;
            }

            let (size, crc) = get_file_size_and_crc(&cue_path, &header, &progress_bar, 1, 1)?;
            let cue_rom = match find_rom(&connection, size, &crc, &system, &progress_bar) {
                Some(rom) => rom,
                None => continue,
            };

            let roms: Vec<Rom> = find_roms_by_game_id(&connection, cue_rom.game_id)
                .into_iter()
                .filter(|rom| rom.id != cue_rom.id)
                .collect();

            let names_sizes: Vec<(&str, u64)> = roms
                .iter()
                .map(|rom| (rom.name.as_str(), rom.size as u64))
                .collect();
            let bin_paths = extract_chd(&file_path, &tmp_directory, &names_sizes, &progress_bar)?;
            let mut crcs: Vec<String> = Vec::new();
            for (i, bin_path) in bin_paths.iter().enumerate() {
                let (_, crc) =
                    get_file_size_and_crc(&bin_path, &header, &progress_bar, i, bin_paths.len())?;
                crcs.push(crc);
                remove_file(&bin_path)?;
            }

            if roms.iter().enumerate().any(|(i, rom)| crcs[i] != rom.crc) {
                progress_bar.println("CRC mismatch");
                continue;
            }

            let new_meta_path = system_directory.join(&cue_rom.name);
            let mut new_file_path = new_meta_path.clone();
            new_file_path.set_extension(CHD_EXTENSION);

            // move cue and chd if needed
            move_file(&cue_path, &new_meta_path, &progress_bar)?;
            move_file(&file_path, &new_file_path, &progress_bar)?;

            // persist in database
            create_or_update_file(&connection, &new_meta_path, &cue_rom);
            for rom in roms {
                create_or_update_file(&connection, &new_file_path, &rom);
            }
        // file is a CSO
        } else if CSO_EXTENSION == file_extension {
            let iso_path = extract_cso(&file_path, &tmp_directory, &progress_bar)?;
            let (size, crc) = get_file_size_and_crc(&iso_path, &header, &progress_bar, 1, 1)?;
            remove_file(&iso_path)?;
            let rom = match find_rom(&connection, size, &crc, &system, &progress_bar) {
                Some(rom) => rom,
                None => continue,
            };

            let mut new_file_path = system_directory.join(&rom.name);
            new_file_path.set_extension(CSO_EXTENSION);

            // move CSO if needed
            move_file(&file_path, &new_file_path, &progress_bar)?;

            // persist in database
            create_or_update_file(&connection, &new_file_path, &rom);
        } else {
            let (size, crc) = get_file_size_and_crc(&file_path, &header, &progress_bar, 1, 1)?;
            let rom = match find_rom(&connection, size, &crc, &system, &progress_bar) {
                Some(rom) => rom,
                None => continue,
            };

            let new_path = system_directory.join(&rom.name);

            // move file if needed
            move_file(&file_path, &new_path, &progress_bar)?;

            // persist in database
            create_or_update_file(&connection, &new_path, &rom);
        }
        progress_bar.inc(1);
    }

    Ok(())
}

fn find_rom(
    connection: &SqliteConnection,
    size: u64,
    crc: &str,
    system: &System,
    progress_bar: &ProgressBar,
) -> Option<Rom> {
    let rom: Rom;
    let mut roms = find_roms_by_size_and_crc_and_system(&connection, size, &crc, system.id);

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
        rom = prompt_for_rom(&mut roms);
    }

    // abort if rom already has a file
    if rom.romfile_id.is_some() {
        let romfile = find_romfile_by_id(&connection, rom.romfile_id.unwrap());
        if romfile.is_some() {
            let romfile = romfile.unwrap();
            progress_bar.println(&format!("Duplicate of \"{}\"", romfile.path));
            return None;
        }
    }

    Some(rom)
}

fn move_file(
    old_path: &PathBuf,
    new_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if old_path != new_path {
        progress_bar.println(&format!("Moving to {:?}", new_path));
        rename_file(&old_path, &new_path)?;
    }
    Ok(())
}

pub fn create_or_update_file(connection: &SqliteConnection, path: &PathBuf, rom: &Rom) {
    let romfile_input = RomfileInput {
        path: &String::from(path.as_os_str().to_str().unwrap()),
    };
    let file = find_romfile_by_path(&connection, &romfile_input.path);
    let file_id = match file {
        Some(file) => {
            update_romfile(&connection, &file, &romfile_input);
            file.id
        }
        None => create_romfile(&connection, &romfile_input),
    };
    update_rom_romfile(&connection, rom, file_id);
}
