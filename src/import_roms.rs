use super::crud::*;
use super::model::*;
use super::prompt::*;
use clap::ArgMatches;
use crc::{crc32, Hasher32};
use diesel::pg::PgConnection;
use std::convert::TryFrom;
use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

pub fn import_roms(connection: &PgConnection, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    let system = prompt_for_system(&connection);
    let header = find_header_by_system_id(&connection, &system.id);

    let rom_directory = Path::new(&env::var("ROM_DIRECTORY").unwrap()).canonicalize()?;
    let new_directory = rom_directory.join(&system.name);
    let tmp_directory = Path::new("/tmp");
    let archive_extensions = vec!["7z", "zip"];
    let chd_extension = "chd";

    if !new_directory.is_dir() {
        fs::create_dir_all(&new_directory)?;
    }

    for f in matches.values_of("ROMS").unwrap() {
        let file_path = Path::new(f).canonicalize()?;
        let file_extension = file_path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase();

        if archive_extensions.contains(&file_extension.as_str()) {
            let sevenzip_infos = parse_archive(&file_path)?;

            // archive contains a single file
            if sevenzip_infos.len() == 1 {
                let size: u64;
                let crc: String;
                let sevenzip_info = sevenzip_infos.get(0).unwrap();

                // system has a header or crc is absent
                if header.is_some() || sevenzip_info.crc == "" {
                    let extracted_path =
                        extract_file_from_archive(&file_path, &sevenzip_info.path, &tmp_directory)?;
                    let res = get_file_size_and_crc(&extracted_path, &header)?;
                    fs::remove_file(&extracted_path)?;
                    size = res.0;
                    crc = res.1;
                } else {
                    size = sevenzip_info.size;
                    crc = String::from(&sevenzip_info.crc);
                }

                let rom = find_rom(&connection, size, &crc, &system);
                let rom = match rom {
                    Ok(rom) => rom,
                    Err(_) => continue,
                };

                let new_path = new_directory.join(format!("{}.{}", &rom.name, &file_extension));

                // move file inside archive if needed
                move_file_in_archive(&file_path, &sevenzip_info, &rom)?;

                // move archive if needed
                move_file(&file_path, &new_path)?;

                // persist in database
                create_or_update_file(&connection, &new_path, &rom);

            // archive contains multiple files
            } else {
                for sevenzip_info in sevenzip_infos {
                    let size: u64;
                    let crc: String;

                    let extracted_path =
                        extract_file_from_archive(&file_path, &sevenzip_info.path, &tmp_directory)?;

                    // system has a header or crc is absent
                    if header.is_some() || sevenzip_info.crc == "" {
                        let res = get_file_size_and_crc(&extracted_path, &header)?;
                        size = res.0;
                        crc = res.1;
                    } else {
                        size = sevenzip_info.size;
                        crc = String::from(&sevenzip_info.crc);
                    }

                    let rom = find_rom(&connection, size, &crc, &system);
                    let rom = match rom {
                        Ok(rom) => rom,
                        Err(_) => continue,
                    };

                    let new_path = new_directory.join(format!("{}.{}", &rom.name, &file_extension));

                    // move file
                    move_file(&extracted_path, &new_path)?;

                    // persist in database
                    create_or_update_file(&connection, &new_path, &rom);
                }

                // delete archive
                fs::remove_file(&file_path)?;
            }
        // file is a CHD
        } else if chd_extension == file_extension {
            let mut cue_name = file_path.file_stem().unwrap().to_os_string();
            cue_name.push(".cue");
            let cue_path = file_path.parent().unwrap().join(cue_name);

            if !cue_path.is_file() {
                println!("Couldn't find {:?}", cue_path);
                continue;
            }

            let (size, crc) = get_file_size_and_crc(&cue_path, &header)?;
            let cue_rom = find_rom(&connection, size, &crc, &system);
            let cue_rom = match cue_rom {
                Ok(cue_rom) => cue_rom,
                Err(_) => continue,
            };

            let mut roms: Vec<Rom> = find_roms_by_game_id(&connection, &cue_rom.game_id)
                .into_iter()
                .filter(|rom| rom.id != cue_rom.id)
                .collect();
            roms.sort_by(|a, b| a.name.cmp(&b.name));

            let sizes: Vec<u64> = roms.iter().map(|rom| rom.size as u64).collect();
            let bin_path = extract_chd(&file_path, &tmp_directory, &cue_path.file_name().unwrap())?;
            let crcs = get_chd_crcs(&bin_path, &sizes);
            fs::remove_file(&bin_path)?;

            let crcs = match crcs {
                Ok(crcs) => crcs,
                Err(_) => continue,
            };

            for (i, rom) in roms.iter().enumerate() {
                if crcs[i] != rom.crc {
                    println!("CRC(s) don't match");
                    continue;
                }
            }

            let mut chd_name = Path::new(&cue_rom.name).file_stem().unwrap().to_os_string();
            chd_name.push(format!(".{}", chd_extension));

            let new_meta_path = new_directory.join(&cue_rom.name);
            let new_file_path = new_directory.join(&chd_name);

            // move cue and chd if needed
            move_file(&cue_path, &new_meta_path)?;
            move_file(&file_path, &new_file_path)?;

            // persist in database
            create_or_update_file(&connection, &new_meta_path, &cue_rom);
            for rom in roms {
                create_or_update_file(&connection, &new_file_path, &rom);
            }
        } else {
            let (size, crc) = get_file_size_and_crc(&file_path, &header)?;
            let rom = find_rom(&connection, size, &crc, &system);
            let rom = match rom {
                Ok(rom) => rom,
                Err(_) => continue,
            };

            let new_path = new_directory.join(&rom.name);

            // move file if needed
            move_file(&file_path, &new_path)?;

            // persist in database
            create_or_update_file(&connection, &new_path, &rom);
        }
    }

    Ok(())
}

fn find_rom(
    connection: &PgConnection,
    size: u64,
    crc: &str,
    system: &System,
) -> Result<Rom, Box<dyn Error>> {
    let rom: Rom;
    let mut roms = find_roms_by_size_and_crc_and_system(&connection, size, &crc, &system.id);

    if roms.is_empty() {
        println!("No matching rom");
        bail!("No matching rom");
    }

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        rom = roms.remove(0);
        println!("Matches \"{}\"", rom.name);
    } else {
        rom = prompt_for_rom(&mut roms);
    }

    // abort if rom already has a file
    if rom.romfile_id.is_some() {
        let file = find_romfile_by_id(&connection, &rom.romfile_id.unwrap());
        if file.is_some() {
            let file = file.unwrap();
            println!("Duplicate of \"{}\"", file.path);
            bail!("Duplicate of \"{}\"", file.path);
        }
    }

    Ok(rom)
}

fn parse_archive(file_path: &PathBuf) -> Result<Vec<SevenzipInfo>, Box<dyn Error>> {
    println!("Scanning {:?}", file_path.file_name().unwrap());
    let output = Command::new("7z")
        .arg("l")
        .arg("-slt")
        .arg(&file_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr);
    }

    let stdout = String::from_utf8(output.stdout)?;
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|&line| {
            line.starts_with("Path =") || line.starts_with("Size =") || line.starts_with("CRC =")
        })
        .skip(1) // the first line is the archive itself
        .map(|line| line.split("=").last().unwrap().trim()) // keep only the rhs
        .collect();

    // each chunk will have the path, size and crc respectively
    let mut sevenzip_infos: Vec<SevenzipInfo> = Vec::new();
    for info in lines.chunks(3) {
        let sevenzip_info = SevenzipInfo {
            path: String::from(info.get(0).unwrap().to_owned()),
            size: FromStr::from_str(info.get(1).unwrap()).unwrap(),
            crc: String::from(info.get(2).unwrap().to_owned()),
        };
        sevenzip_infos.push(sevenzip_info);
    }
    Ok(sevenzip_infos)
}

fn get_file_size_and_crc(
    file_path: &PathBuf,
    header: &Option<Header>,
) -> Result<(u64, String), Box<dyn Error>> {
    let mut f = fs::File::open(&file_path)?;
    let mut size = f.metadata().unwrap().len();

    // extract a potential header, revert if none is found
    if header.is_some() {
        let header = header.as_ref().unwrap();

        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        (&mut f).take(header.size as u64).read_to_end(&mut buffer)?;
        let start = header.start as usize;
        let hex_values: Vec<String> = buffer[start..].iter().map(|b| format!("{:x}", b)).collect();
        let hex_value = hex_values.join("").to_uppercase();

        if hex_value.starts_with(&header.hex_value.to_uppercase()) {
            size -= header.size as u64;
        } else {
            f.seek(std::io::SeekFrom::Start(0))?;
        }
    }

    // read our file in 4k chunks
    let mut digest = crc32::Digest::new(crc32::IEEE);
    let mut buffer = [0; 4096];
    loop {
        let n = f.read(&mut buffer[..])?;
        if n == 0 {
            break;
        }
        digest.write(&mut buffer[..n]);
    }

    let crc = format!("{:08x}", digest.sum32());
    Ok((size, crc))
}

fn get_chd_crcs(file_path: &PathBuf, sizes: &Vec<u64>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut f = fs::File::open(&file_path)?;
    let size = f.metadata().unwrap().len();

    if size != sizes.iter().sum() {
        println!("Size(s) don't match");
        bail!("Size(s) don't match");
    }

    let mut crcs: Vec<String> = Vec::new();
    const BUFFER_SIZE: usize = 4096;

    for size in sizes {
        let mut digest = crc32::Digest::new(crc32::IEEE);
        let mut buffer = [0; BUFFER_SIZE];
        let mut consumed_bytes: usize = 0;

        // read 4k chunks until near the end
        loop {
            let n = f.read(&mut buffer[..])?;
            digest.write(&mut buffer[..n]);
            consumed_bytes += n;
            if (consumed_bytes as u64) + (BUFFER_SIZE as u64) >= *size {
                break;
            }
        }
        // read the exact remaining amount
        let remaining_bytes = size - consumed_bytes as u64;
        let remaining_bytes_usize = usize::try_from(remaining_bytes).unwrap();
        let mut buffer: Vec<u8> = Vec::with_capacity(remaining_bytes_usize);
        (&mut f).take(remaining_bytes).read_to_end(&mut buffer)?;
        digest.write(&mut buffer);

        crcs.push(format!("{:08x}", digest.sum32()));
    }
    Ok(crcs)
}

fn move_file_in_archive(
    archive_path: &PathBuf,
    sevenzip_info: &SevenzipInfo,
    rom: &Rom,
) -> Result<(), Box<dyn Error>> {
    if sevenzip_info.path != rom.name {
        println!("Renaming \"{}\" to \"{}\"", sevenzip_info.path, rom.name);
        let output = Command::new("7z")
            .arg("rn")
            .arg(archive_path)
            .arg(&sevenzip_info.path)
            .arg(&rom.name)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)?;
            println!("{}", stderr);
            bail!(stderr);
        }
    }
    Ok(())
}

fn extract_file_from_archive(
    archive_path: &PathBuf,
    path: &str,
    tmp_directory: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    println!("Extracting \"{}\" to {:?}", path, tmp_directory);
    let output = Command::new("7z")
        .arg("x")
        .arg(archive_path)
        .arg(format!("-o{}", tmp_directory.as_os_str().to_str().unwrap()))
        .arg(path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    Ok(tmp_directory.join(path))
}

fn extract_chd(
    chd_path: &PathBuf,
    tmp_directory: &Path,
    meta_name: &OsStr,
) -> Result<PathBuf, Box<dyn Error>> {
    println!(
        "Extracting {:?} to {:?}",
        chd_path.file_name().unwrap(),
        tmp_directory
    );
    let meta_path = tmp_directory.join(meta_name);
    let mut bin_name = chd_path.file_stem().unwrap().to_os_string();
    bin_name.push(".bin");
    let bin_path = tmp_directory.join(bin_name);
    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path)
        .arg("-o")
        .arg(&meta_path)
        .arg("-ob")
        .arg(&bin_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    fs::remove_file(meta_path)?;
    Ok(bin_path)
}

fn move_file(old_path: &PathBuf, new_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    if old_path != new_path {
        println!("Moving to {:?}", new_path);
        fs::rename(&old_path, &new_path)?;
    }
    Ok(())
}

pub fn create_or_update_file(connection: &PgConnection, new_path: &PathBuf, rom: &Rom) {
    let romfile_input = RomfileInput {
        path: &String::from(new_path.as_os_str().to_str().unwrap()),
    };
    let file = find_romfile_by_path(&connection, &romfile_input.path);
    let file = match file {
        Some(file) => update_romfile(&connection, &file, &romfile_input),
        None => create_romfile(&connection, &romfile_input),
    };
    update_rom_romfile(&connection, rom, &file.id);
}