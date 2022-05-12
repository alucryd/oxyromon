#[cfg(feature = "chd")]
use super::chdman;
use super::checksum::*;
use super::config::*;
use super::database::*;
#[cfg(feature = "rvz")]
use super::dolphin;
#[cfg(feature = "cso")]
use super::maxcso;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip;
use super::util::*;
use async_std::path::Path;
use cfg_if::cfg_if;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use simple_error::SimpleResult;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;
use std::convert::TryFrom;

pub fn subcommand<'a>() -> Command<'a> {
    Command::new("check-roms")
        .about("Check ROM files integrity")
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Check all systems")
                .required(false),
        )
        .arg(
            Arg::new("SIZE")
                .short('s')
                .long("size")
                .help("Recalculate ROM file sizes")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, matches.is_present("ALL")).await?;
    let hash_algorithm = match find_setting_by_key(connection, "HASH_ALGORITHM")
        .await
        .unwrap()
        .value
        .as_deref()
    {
        Some("CRC") => HashAlgorithm::Crc,
        Some("MD5") => HashAlgorithm::Md5,
        Some("SHA1") => HashAlgorithm::Sha1,
        Some(_) | None => bail!("Not possible"),
    };
    for system in systems {
        progress_bar.println(&format!("Processing \"{}\"", system.name));
        check_system(
            connection,
            progress_bar,
            &system,
            matches.is_present("SIZE"),
            &hash_algorithm,
        )
        .await?;
        progress_bar.println("");
    }
    Ok(())
}

async fn check_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    size: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let header = find_header_by_system_id(connection, system.id).await;
    let roms = find_roms_with_romfile_by_system_id(connection, system.id).await;
    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let mut roms_by_romfile_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms.into_iter().for_each(|rom| {
        let group = roms_by_romfile_id
            .entry(rom.romfile_id.unwrap())
            .or_insert_with(Vec::new);
        group.push(rom);
    });

    let mut transaction = begin_transaction(connection).await;

    let mut errors = 0;

    for romfile in romfiles {
        let romfile_path = get_canonicalized_path(&romfile.path).await?;
        let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
        let roms = roms_by_romfile_id.remove(&romfile.id).unwrap();

        progress_bar.println(&format!(
            "Processing {:?}",
            romfile_path.file_name().unwrap()
        ));

        let result;
        if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
            result = check_archive(
                &mut transaction,
                progress_bar,
                &header,
                &romfile_path,
                roms,
                hash_algorithm,
            )
            .await;
        } else if CHD_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "chd")] {
                    result = check_chd(
                        &mut transaction,
                        progress_bar, &header,
                        &romfile_path,
                        roms,
                        hash_algorithm
                    )
                    .await;
                } else {
                progress_bar.println("Please rebuild with the CHD feature enabled");
                    continue;
                }
            }
        } else if CSO_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "cso")] {
                    result = check_cso(
                        &mut transaction,
                        progress_bar,
                        &header,
                        &romfile_path,
                        roms.get(0).unwrap(),
                        hash_algorithm
                    )
                    .await;
                } else {
                    progress_bar.println("Please rebuild with the CSO feature enabled");
                    continue;
                }
            }
        } else if RVZ_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "rvz")] {
                    result = check_rvz(
                        &mut transaction,
                        progress_bar,
                        &header,
                        &romfile_path,
                        roms.get(0).unwrap(),
                        hash_algorithm
                    )
                    .await;
                } else {
                    progress_bar.println("Please rebuild with the RVZ feature enabled");
                    continue;
                }
            }
        } else {
            result = check_other(
                &mut transaction,
                progress_bar,
                &header,
                &romfile_path,
                roms.get(0).unwrap(),
                hash_algorithm,
            )
            .await;
        }

        if result.is_err() {
            errors += 1;
            move_to_trash(&mut transaction, progress_bar, system, &romfile).await?;
        } else if size {
            update_romfile(
                &mut transaction,
                romfile.id,
                &romfile.path,
                Path::new(&romfile.path).metadata().await.unwrap().len(),
            )
            .await;
        }
    }

    commit_transaction(transaction).await;

    // update games and systems completion
    if errors > 0 {
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(100);
        progress_bar.set_message("Computing system completion");
        update_games_by_system_id_mark_incomplete(connection, system.id).await;
        cfg_if! {
            if #[cfg(feature = "ird")] {
                update_jbfolder_games_by_system_id_mark_incomplete(connection, system.id).await;
            }
        }
        update_system_mark_complete(connection, system.id).await;
        update_system_mark_incomplete(connection, system.id).await;
    }

    Ok(())
}

async fn check_archive<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    mut roms: Vec<Rom>,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let sevenzip_infos = sevenzip::parse_archive(progress_bar, romfile_path)?;

    if sevenzip_infos.len() != roms.len() {
        bail!("Archive contains a different number of ROM files");
    }

    for sevenzip_info in sevenzip_infos {
        let size: u64;
        let hash: String;
        if header.is_some() || sevenzip_info.crc.is_empty() || hash_algorithm != &HashAlgorithm::Crc
        {
            let tmp_directory = create_tmp_directory(connection).await?;
            let extracted_path = sevenzip::extract_files_from_archive(
                progress_bar,
                romfile_path,
                &[&sevenzip_info.path],
                &tmp_directory.path(),
            )?
            .remove(0);
            let size_hash = get_size_and_hash(
                connection,
                progress_bar,
                &extracted_path,
                header,
                1,
                1,
                hash_algorithm,
            )
            .await?;
            size = size_hash.0;
            hash = size_hash.1;
        } else {
            size = sevenzip_info.size;
            hash = sevenzip_info.crc.clone();
        }
        let rom_index = roms
            .iter()
            .position(|rom| rom.name == sevenzip_info.path)
            .unwrap();
        let rom = roms.remove(rom_index);
        check_size_and_hash(&rom, i64::try_from(size).unwrap(), &hash, hash_algorithm)?;
    }

    Ok(())
}

#[cfg(feature = "chd")]
async fn check_chd<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    roms: Vec<Rom>,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;

    let names_sizes: Vec<(&str, u64)> = roms
        .iter()
        .map(|rom| (rom.name.as_str(), rom.size as u64))
        .collect();
    let bin_paths = chdman::extract_chd_to_multiple_tracks(
        progress_bar,
        romfile_path,
        &tmp_directory.path(),
        &names_sizes,
        true,
    )
    .await?;
    let mut hashes: Vec<String> = Vec::new();
    for (i, bin_path) in bin_paths.iter().enumerate() {
        let (_, hash) = get_size_and_hash(
            connection,
            progress_bar,
            &bin_path,
            header,
            i,
            bin_paths.len(),
            hash_algorithm,
        )
        .await?;
        hashes.push(hash);
    }

    match hash_algorithm {
        HashAlgorithm::Crc => {
            if roms
                .iter()
                .enumerate()
                .any(|(i, rom)| &hashes[i] != rom.crc.as_ref().unwrap())
            {
                bail!("Checksum mismatch");
            }
        }
        HashAlgorithm::Md5 => {
            if roms
                .iter()
                .enumerate()
                .any(|(i, rom)| &hashes[i] != rom.md5.as_ref().unwrap())
            {
                bail!("Checksum mismatch");
            }
        }
        HashAlgorithm::Sha1 => {
            if roms
                .iter()
                .enumerate()
                .any(|(i, rom)| &hashes[i] != rom.sha1.as_ref().unwrap())
            {
                bail!("Checksum mismatch");
            }
        }
    }

    Ok(())
}

#[cfg(feature = "cso")]
async fn check_cso<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    rom: &Rom,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let iso_path = maxcso::extract_cso(progress_bar, romfile_path, &tmp_directory.path())?;
    let (size, hash) = get_size_and_hash(
        connection,
        progress_bar,
        &iso_path,
        header,
        1,
        1,
        hash_algorithm,
    )
    .await?;
    check_size_and_hash(rom, i64::try_from(size).unwrap(), &hash, hash_algorithm)?;
    Ok(())
}

#[cfg(feature = "rvz")]
async fn check_rvz<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    rom: &Rom,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let iso_path = dolphin::extract_rvz(progress_bar, romfile_path, &tmp_directory.path())?;
    let (size, hash) = get_size_and_hash(
        connection,
        progress_bar,
        &iso_path,
        header,
        1,
        1,
        hash_algorithm,
    )
    .await?;
    check_size_and_hash(rom, i64::try_from(size).unwrap(), &hash, hash_algorithm)?;
    Ok(())
}

async fn check_other<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile_path: &P,
    rom: &Rom,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let (size, hash) = get_size_and_hash(
        connection,
        progress_bar,
        romfile_path,
        header,
        1,
        1,
        hash_algorithm,
    )
    .await?;
    check_size_and_hash(rom, i64::try_from(size).unwrap(), &hash, hash_algorithm)?;
    Ok(())
}

fn check_size_and_hash(
    rom: &Rom,
    size: i64,
    hash: &str,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    if size != rom.size {
        bail!("Size mismatch");
    };
    match hash_algorithm {
        HashAlgorithm::Crc => {
            if hash != rom.crc.as_ref().unwrap() {
                bail!("Checksum mismatch");
            }
        }
        HashAlgorithm::Md5 => {
            if hash != rom.md5.as_ref().unwrap() {
                bail!("Checksum mismatch");
            }
        }
        HashAlgorithm::Sha1 => {
            if hash != rom.sha1.as_ref().unwrap() {
                bail!("Checksum mismatch");
            }
        }
    }
    Ok(())
}

async fn move_to_trash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    romfile: &Romfile,
) -> SimpleResult<()> {
    let new_path = get_trash_directory(connection, progress_bar, system)
        .await?
        .join(Path::new(&romfile.path).file_name().unwrap());
    rename_file(progress_bar, &romfile.path, &new_path, true).await?;
    update_romfile(
        connection,
        romfile.id,
        new_path.as_os_str().to_str().unwrap(),
        romfile.size as u64,
    )
    .await;
    Ok(())
}

#[cfg(all(test, feature = "chd"))]
mod test_check_chd_single_track;

#[cfg(all(test, feature = "chd"))]
mod test_check_chd_multiple_tracks;

#[cfg(all(test, feature = "cso"))]
mod test_check_cso;

#[cfg(test)]
mod test_check_other;

#[cfg(test)]
mod test_check_other_with_header;

#[cfg(test)]
mod test_check_other_crc_mismatch;

#[cfg(test)]
mod test_check_other_size_mismatch;

#[cfg(test)]
mod test_check_sevenzip;

#[cfg(test)]
mod test_check_sevenzip_with_header;

#[cfg(test)]
mod test_check_zip;
