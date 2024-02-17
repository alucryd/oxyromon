#[cfg(feature = "chd")]
use super::chdman;
#[cfg(feature = "chd")]
use super::chdman::AsChd;
use super::common::*;
use super::config::*;
use super::database::*;
#[cfg(feature = "rvz")]
use super::dolphin;
#[cfg(feature = "rvz")]
use super::dolphin::AsRvz;
#[cfg(any(feature = "cso", feature = "zso"))]
use super::maxcso;
#[cfg(any(feature = "cso", feature = "zso"))]
use super::maxcso::AsXso;
use super::model::*;
#[cfg(feature = "nsz")]
use super::nsz;
#[cfg(feature = "nsz")]
use super::nsz::AsNsz;
use super::prompt::*;
use super::sevenzip;
use super::util::*;
use cfg_if::cfg_if;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use simple_error::SimpleResult;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;
use std::path::Path;

pub fn subcommand() -> Command {
    Command::new("check-roms")
        .about("Check ROM files integrity")
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Check all systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("NAME")
                .short('n')
                .long("name")
                .help("Select games by name")
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("SIZE")
                .short('s')
                .long("size")
                .help("Recalculate ROM file sizes")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, matches.get_flag("ALL")).await?;
    let game_name = matches.get_one::<String>("NAME");
    let hash_algorithm = match find_setting_by_key(connection, "HASH_ALGORITHM")
        .await
        .unwrap()
        .value
        .as_deref()
    {
        Some("crc") => HashAlgorithm::Crc,
        Some("md5") => HashAlgorithm::Md5,
        Some("sha1") => HashAlgorithm::Sha1,
        Some(_) | None => bail!("Not possible"),
    };
    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));
        check_system(
            connection,
            progress_bar,
            &system,
            &game_name,
            matches.get_flag("SIZE"),
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
    game_name: &Option<&String>,
    size: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let games = match game_name {
        Some(game_name) => {
            let games = find_games_with_romfiles_by_name_and_system_id(
                connection,
                &format!("%{}%", game_name),
                system.id,
            )
            .await;
            prompt_for_games(games, cfg!(test))?
        }
        None => find_games_with_romfiles_by_system_id(connection, system.id).await,
    };

    if games.is_empty() {
        if game_name.is_some() {
            progress_bar.println(format!("No game matching \"{}\"", game_name.unwrap()));
        }
        return Ok(());
    }

    let roms = find_roms_with_romfile_by_game_ids(
        connection,
        &games.iter().map(|game| game.id).collect::<Vec<i64>>(),
    )
    .await;
    let romfiles = find_romfiles_by_ids(
        connection,
        roms.iter()
            .map(|rom| rom.romfile_id.unwrap())
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;
    let mut roms_by_romfile_id: HashMap<i64, Vec<&Rom>> = HashMap::new();
    roms.iter().for_each(|rom| {
        let group = roms_by_romfile_id
            .entry(rom.romfile_id.unwrap())
            .or_default();
        group.push(rom);
    });
    let header = find_header_by_system_id(connection, system.id).await;

    let mut transaction = begin_transaction(connection).await;

    let mut errors = 0;

    for romfile in &romfiles {
        let romfile_path = get_canonicalized_path(&romfile.path).await?;
        let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
        let romfile_roms = roms_by_romfile_id.remove(&romfile.id).unwrap();

        progress_bar.println(format!(
            "Processing \"{}\"",
            romfile_path.file_name().unwrap().to_str().unwrap()
        ));

        let result;
        if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
            if sevenzip::get_version().await.is_err() {
                progress_bar.println("Please install sevenzip");
                break;
            }
            result = check_archive(
                &mut transaction,
                progress_bar,
                &header,
                romfile,
                romfile_roms,
                hash_algorithm,
            )
            .await;
        } else if CHD_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "chd")] {
                    if chdman::get_version().await.is_err() {
                        progress_bar.println("Please install chdman");
                        break;
                    }
                    let game = games.iter().find(|game| game.id == romfile_roms.first().unwrap().game_id).unwrap();
                    let cue_rom = roms.iter().find(|rom| rom.game_id == game.id && rom.name.ends_with(CUE_EXTENSION));
                    let cue_romfile = cue_rom.map(|cue_rom| romfiles.iter().find(|romfile| romfile.id == cue_rom.romfile_id.unwrap()).unwrap());
                    result = match cue_romfile {
                        Some(cue_romfile) => romfile
                            .as_chd_with_cue(&cue_romfile.path)?
                            .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                            .await,
                        None => romfile
                            .as_chd()?
                            .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                            .await,
                    };
                } else {
                progress_bar.println("Please rebuild with the CHD feature enabled");
                    break;
                }
            }
        } else if CSO_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "cso")] {
                    if maxcso::get_version().await.is_err() {
                        progress_bar.println("Please install maxcso");
                        break;
                    }
                    result = romfile
                        .as_xso()?
                        .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                        .await;
                } else {
                    progress_bar.println("Please rebuild with the CSO feature enabled");
                    break;
                }
            }
        } else if NSZ_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "nsz")] {
                    if nsz::get_version().await.is_err() {
                        progress_bar.println("Please install nsz");
                        break;
                    }
                    result = romfile
                        .as_nsz()?
                        .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                        .await;
                } else {
                    progress_bar.println("Please rebuild with the NSZ feature enabled");
                    break;
                }
            }
        } else if RVZ_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "rvz")] {
                    if dolphin::get_version().await.is_err() {
                        progress_bar.println("Please install dolphin");
                        break;
                    }
                    result = romfile
                        .as_rvz()?
                        .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                        .await;
                } else {
                    progress_bar.println("Please rebuild with the RVZ feature enabled");
                    break;
                }
            }
        } else if ZSO_EXTENSION == romfile_extension {
            cfg_if! {
                if #[cfg(feature = "zso")] {
                    if maxcso::get_version().await.is_err() {
                        progress_bar.println("Please install maxcso");
                        break;
                    }
                    result = romfile
                        .as_xso()?
                        .check(&mut transaction, progress_bar, &header, &romfile_roms, hash_algorithm)
                        .await;
                } else {
                    progress_bar.println("Please rebuild with the ZSO feature enabled");
                    break;
                }
            }
        } else {
            result = romfile
                .as_common()?
                .check(
                    &mut transaction,
                    progress_bar,
                    &header,
                    &romfile_roms,
                    hash_algorithm,
                )
                .await;
        }

        if result.is_err() {
            errors += 1;
            move_to_trash(&mut transaction, progress_bar, system, romfile).await?;
        } else if size {
            update_romfile(
                &mut transaction,
                romfile.id,
                &romfile.path,
                Path::new(&romfile.path).metadata().unwrap().len(),
            )
            .await;
        }
    }

    // update games and systems completion
    if errors > 0 {
        if system.arcade {
            compute_arcade_system_incompletion(&mut transaction, progress_bar, system).await;
        } else {
            compute_system_incompletion(&mut transaction, progress_bar, system).await;
        }
    }

    commit_transaction(transaction).await;

    Ok(())
}

async fn check_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile: &Romfile,
    roms: Vec<&Rom>,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let archive_romfiles = sevenzip::parse(progress_bar, &romfile.path).await?;
    if archive_romfiles.len() != roms.len() {
        bail!("Archive contains a different number of ROM files");
    }
    for archive_romfile in archive_romfiles {
        let rom = roms
            .iter()
            .find(|rom| rom.name == archive_romfile.file_path)
            .unwrap();
        archive_romfile
            .check(connection, progress_bar, header, &[rom], hash_algorithm)
            .await?;
    }
    Ok(())
}

async fn move_to_trash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    romfile: &Romfile,
) -> SimpleResult<()> {
    let new_path = get_trash_directory(connection, Some(system))
        .await?
        .join(Path::new(&romfile.path).file_name().unwrap());
    rename_file(progress_bar, &romfile.path, &new_path, false).await?;
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
mod test_chd_multiple_tracks;
#[cfg(all(test, feature = "chd"))]
mod test_chd_single_track;
#[cfg(all(test, feature = "cso"))]
mod test_cso;
#[cfg(test)]
mod test_original;
#[cfg(test)]
mod test_original_crc_mismatch;
#[cfg(test)]
mod test_original_size_mismatch;
#[cfg(test)]
mod test_original_with_header;
#[cfg(all(test, feature = "rvz"))]
mod test_rvz;
#[cfg(test)]
mod test_sevenzip;
#[cfg(test)]
mod test_sevenzip_with_header;
#[cfg(test)]
mod test_zip;
#[cfg(all(test, feature = "zso"))]
mod test_zso;
