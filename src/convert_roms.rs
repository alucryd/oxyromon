use super::chdman;
use super::chdman::{AsChd, MediaType, ToChd};
use super::common::*;
use super::config::*;
use super::database::*;
use super::dolphin;
use super::dolphin::{AsRvz, RvzCompressionAlgorithm, ToRvz};
use super::maxcso;
use super::maxcso::{AsXso, ToXso, XsoType};
use super::model::*;
use super::nsz;
use super::nsz::{AsNsp, AsNsz, ToNsp, ToNsz};
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::{ArchiveFile, ArchiveRomfile, AsArchive, ToArchive};
use super::util::*;
use super::SimpleResult;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indexmap::map::IndexMap;
use indicatif::{HumanBytes, ProgressBar};
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::drop;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const ALL_FORMATS: &[&str] = &["ORIGINAL", "7Z", "CHD", "CSO", "NSZ", "RVZ", "ZIP", "ZSO"];
const ARCADE_FORMATS: &[&str] = &["ORIGINAL", "ZIP"];

pub fn subcommand() -> Command {
    Command::new("convert-roms")
        .about("Convert ROM files between common formats")
        .arg(
            Arg::new("FORMAT")
                .short('f')
                .long("format")
                .help("Set the destination format")
                .required(false)
                .num_args(1)
                .value_parser(PossibleValuesParser::new(ALL_FORMATS.iter())),
        )
        .arg(
            Arg::new("GAME")
                .short('g')
                .long("game")
                .help("Select games by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select systems by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Convert all systems/games")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("RECOMPRESS")
                .short('r')
                .long("recompress")
                .help("Force conversion even if already in the selected format")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("DIFF")
                .short('d')
                .long("diff")
                .help("Print size differences")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("CHECK")
                .short('c')
                .long("check")
                .help("Check ROM files after conversion")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("PARENTS")
                .short('p')
                .long("parents")
                .help("Prompt for CHD parents")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = match matches.get_many::<String>("SYSTEM") {
        Some(system_names) => {
            let mut systems: Vec<System> = Vec::new();
            for system_name in system_names {
                systems.append(&mut find_systems_by_name_like(connection, system_name).await);
            }
            systems.dedup_by_key(|system| system.id);
            systems
        }
        None => prompt_for_systems(connection, None, false, matches.get_flag("ALL")).await?,
    };
    let format = match matches.get_one::<String>("FORMAT") {
        Some(format) => format.as_str().to_owned(),
        None => ALL_FORMATS
            .get(select(ALL_FORMATS, "Please select a format", None, None)?)
            .map(|&s| s.to_owned())
            .unwrap(),
    };
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
    let recompress = matches.get_flag("RECOMPRESS");
    let diff = matches.get_flag("DIFF");
    let check = matches.get_flag("CHECK");

    match format.as_str() {
        "7Z" | "ZIP" => {
            if sevenzip::get_version().await.is_err() {
                progress_bar.println("Please install sevenzip");
                return Ok(());
            }
        }
        "CHD" => {
            if chdman::get_version().await.is_err() {
                progress_bar.println("Please install chdman");
                return Ok(());
            }
        }
        "CSO" => {
            if maxcso::get_version().await.is_err() {
                progress_bar.println("Please install maxcso");
                return Ok(());
            }
        }
        "NSZ" => {
            if nsz::get_version().await.is_err() {
                progress_bar.println("Please install nsz");
                return Ok(());
            }
        }
        "RVZ" => {
            if dolphin::get_version().await.is_err() {
                progress_bar.println("Please install dolphin-tool");
                return Ok(());
            }
        }
        "ZSO" => {
            if maxcso::get_version().await.is_err() {
                progress_bar.println("Please install maxcso");
                return Ok(());
            }
        }
        "ORIGINAL" => {}
        _ => bail!("Not supported"),
    }

    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));

        if format == "CHD"
            && system.name.contains("Dreamcast")
            && chdman::get_version()
                .await?
                .as_str()
                .cmp(chdman::MIN_DREAMCAST_VERSION)
                == Ordering::Less
        {
            progress_bar.println(format!("Older chdman versions have issues with Dreamcast games, please update to {} or newer", chdman::MIN_DREAMCAST_VERSION));
            continue;
        }

        if system.arcade && !ARCADE_FORMATS.contains(&format.as_str()) {
            progress_bar.println(format!(
                "Only {:?} are supported for arcade systems",
                ARCADE_FORMATS
            ));
            continue;
        }

        let games = match matches.get_many::<String>("GAME") {
            Some(game_names) => {
                let mut games: Vec<Game> = Vec::new();
                for game_name in game_names {
                    games.append(
                        &mut find_games_with_romfiles_by_name_and_system_id(
                            connection, game_name, system.id,
                        )
                        .await,
                    );
                }
                games.dedup_by_key(|game| game.id);
                prompt_for_games(games, cfg!(test))?
            }
            None => find_games_with_romfiles_by_system_id(connection, system.id).await,
        };

        if games.is_empty() {
            if matches.index_of("GAME").is_some() {
                progress_bar.println("No matching game");
            }
            continue;
        }

        let roms = find_roms_with_romfile_by_game_ids(
            connection,
            &games.par_iter().map(|game| game.id).collect::<Vec<i64>>(),
        )
        .await;
        let romfiles = find_romfiles_by_ids(
            connection,
            roms.par_iter()
                .map(|rom| rom.romfile_id.unwrap())
                .collect::<Vec<i64>>()
                .as_slice(),
        )
        .await;

        let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
        roms.into_iter().for_each(|rom| {
            let group = roms_by_game_id.entry(rom.game_id).or_default();
            group.push(rom);
        });
        let games_by_id: HashMap<i64, Game> =
            games.into_par_iter().map(|game| (game.id, game)).collect();
        let romfiles_by_id: HashMap<i64, Romfile> = romfiles
            .into_par_iter()
            .map(|romfile| (romfile.id, romfile))
            .collect();

        match format.as_str() {
            "ORIGINAL" => {
                to_original(
                    connection,
                    progress_bar,
                    &system,
                    roms_by_game_id,
                    romfiles_by_id,
                    check,
                    &hash_algorithm,
                )
                .await?
            }
            "7Z" => {
                let compression_level = get_integer(connection, "SEVENZIP_COMPRESSION_LEVEL").await;
                let solid = get_bool(connection, "SEVENZIP_SOLID_COMPRESSION").await;
                to_archive(
                    connection,
                    progress_bar,
                    sevenzip::ArchiveType::Sevenzip,
                    &system,
                    roms_by_game_id,
                    games_by_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                    &compression_level,
                    solid,
                )
                .await?
            }
            "ZIP" => {
                let compression_level = get_integer(connection, "ZIP_COMPRESSION_LEVEL").await;
                to_archive(
                    connection,
                    progress_bar,
                    sevenzip::ArchiveType::Zip,
                    &system,
                    roms_by_game_id,
                    games_by_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                    &compression_level,
                    false,
                )
                .await?
            }
            "CHD" => {
                let cd_compression_algorithms =
                    get_list(connection, "CHD_CD_COMPRESSION_ALGORITHMS").await;
                let cd_hunk_size = get_integer(connection, "CHD_CD_HUNK_SIZE").await;
                let dvd_compression_algorithms =
                    get_list(connection, "CHD_DVD_COMPRESSION_ALGORITHMS").await;
                let dvd_hunk_size = get_integer(connection, "CHD_DVD_HUNK_SIZE").await;
                let parents = get_bool(connection, "CHD_PARENTS").await;
                let prompt_for_parents = matches.get_flag("PARENTS");
                to_chd(
                    connection,
                    progress_bar,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                    &cd_compression_algorithms,
                    &cd_hunk_size,
                    &dvd_compression_algorithms,
                    &dvd_hunk_size,
                    parents,
                    prompt_for_parents,
                )
                .await?
            }
            "CSO" => {
                to_cso(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                )
                .await?
            }
            "NSZ" => {
                to_nsz(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                )
                .await?
            }
            "RVZ" => {
                let compression_algorithm = RvzCompressionAlgorithm::from_str(
                    &get_string(connection, "RVZ_COMPRESSION_ALGORITHM")
                        .await
                        .unwrap(),
                )
                .unwrap();
                let compression_level = get_integer(connection, "RVZ_COMPRESSION_LEVEL")
                    .await
                    .unwrap();
                let block_size = get_integer(connection, "RVZ_BLOCK_SIZE").await.unwrap();
                to_rvz(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                    &compression_algorithm,
                    compression_level,
                    block_size,
                )
                .await?
            }
            "ZSO" => {
                to_zso(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &hash_algorithm,
                )
                .await?
            }
            _ => bail!("Not supported"),
        }

        progress_bar.println("");
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    archive_type: sevenzip::ArchiveType,
    system: &System,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    games_by_id: HashMap<i64, Game>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
    compression_level: &Option<usize>,
    solid: bool,
) -> SimpleResult<()> {
    // partition CHDs
    let (mut chds, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
        });
    // make sure children are converted before parents
    chds.par_sort_by(|_, a, _, b| {
        b.par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .any(|romfile| romfile.parent_id.is_some())
            .cmp(
                &a.par_iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .any(|romfile| romfile.parent_id.is_some()),
            )
    });

    // partition CSOs
    let (csos, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CSO_EXTENSION)
            })
        });

    // partition NSZs
    let (nszs, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSZ_EXTENSION)
            })
        });

    // partition RVZs
    let (rvzs, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(RVZ_EXTENSION)
            })
        });

    // partition ZSOs
    let (zsos, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });

    // partition archives
    let (archives, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let path = &romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap().path;
                path.ends_with(SEVENZIP_EXTENSION) || path.ends_with(ZIP_EXTENSION)
            })
        });

    // convert CHDs
    for roms in chds.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        if roms.len() == 1 {
            let rom = roms.first().unwrap();
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_parent(&parent_chd_romfile)?
                }
                None => romfile.as_common(&mut transaction).await?.as_chd()?,
            };
            let archive_romfile = chd_romfile
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .as_common()?
                .to_archive(
                    progress_bar,
                    &tmp_directory.path(),
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
                .await?;

            if check
                && archive_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                archive_romfile
                    .as_common()?
                    .delete(progress_bar, false)
                    .await?;
                continue;
            };

            archive_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            update_romfile_parent(&mut transaction, romfile.id, None).await;

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[romfile],
                    &[&archive_romfile.path],
                )
                .await?;
            }

            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        } else {
            let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                .iter()
                .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
            let cue_rom = cue_roms.first().unwrap();
            let game = games_by_id.get(&cue_rom.game_id).unwrap();
            let cue_romfile = romfiles_by_id.get(&cue_rom.romfile_id.unwrap()).unwrap();
            let romfile = romfiles_by_id
                .get(&bin_roms.first().unwrap().romfile_id.unwrap())
                .unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_cue_and_parent(
                            &cue_romfile.as_common(&mut transaction).await?,
                            &parent_chd_romfile,
                        )?
                }
                None => romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_chd_with_cue(&cue_romfile.as_common(&mut transaction).await?)?,
            };

            let mut archive_romfiles: Vec<ArchiveRomfile> = Vec::new();
            let destination_directory = cue_romfile
                .as_common(&mut transaction)
                .await?
                .path
                .parent()
                .unwrap()
                .to_path_buf();
            let archive_romfile = cue_romfile
                .as_common(&mut transaction)
                .await?
                .to_archive(
                    progress_bar,
                    &cue_romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &destination_directory,
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
                .await?;
            archive_romfiles.push(archive_romfile);
            let cue_bin_romfile = chd_romfile
                .to_cue_bin(progress_bar, &tmp_directory.path(), &bin_roms, true)
                .await?;

            for bin_romfile in cue_bin_romfile.bin_romfiles {
                let archive_romfile = bin_romfile
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid,
                    )
                    .await?;
                archive_romfiles.push(archive_romfile);
            }

            if check {
                let mut error = false;
                let roms = [cue_roms.as_slice(), bin_roms.as_slice()].concat();
                for (archive_romfile, rom) in archive_romfiles.iter().zip(roms) {
                    if archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                    {
                        error = true;
                        break;
                    }
                }
                if error {
                    progress_bar.println("Converted file doesn't match the original");
                    archive_romfiles
                        .first()
                        .unwrap()
                        .as_common()?
                        .delete(progress_bar, false)
                        .await?;
                    continue;
                }
            }

            archive_romfiles
                .first()
                .unwrap()
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            update_romfile_parent(&mut transaction, romfile.id, None).await;
            update_rom_romfile(&mut transaction, cue_rom.id, Some(romfile.id)).await;
            delete_romfile_by_id(&mut transaction, cue_romfile.id).await;

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[cue_romfile, romfile],
                    &[&archive_romfiles.first().unwrap().path],
                )
                .await?;
            }

            cue_romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    for roms in csos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

        let archive_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_xso()?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .as_common()?
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &game.name,
                &archive_type,
                compression_level,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            archive_romfile
                .as_common()?
                .delete(progress_bar, false)
                .await?;
            continue;
        };

        archive_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[romfile],
                &[&archive_romfile.path],
            )
            .await?;
        }

        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert NSZs
    for roms in nszs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

        let archive_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_nsz()?
            .to_nsp(progress_bar, &tmp_directory.path())
            .await?
            .as_common()?
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &game.name,
                &archive_type,
                compression_level,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            archive_romfile
                .as_common()?
                .delete(progress_bar, false)
                .await?;
            continue;
        };

        archive_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[romfile],
                &[&archive_romfile.path],
            )
            .await?;
        }

        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert RVZs
    for roms in rvzs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

        let archive_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_rvz()?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .as_common()?
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &game.name,
                &archive_type,
                compression_level,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            archive_romfile
                .as_common()?
                .delete(progress_bar, false)
                .await?;
            continue;
        };

        archive_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[romfile],
                &[&archive_romfile.path],
            )
            .await?;
        }

        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

        let archive_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_xso()?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .as_common()?
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &game.name,
                &archive_type,
                compression_level,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            archive_romfile
                .as_common()?
                .delete(progress_bar, false)
                .await?;
            continue;
        };

        archive_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[romfile],
                &[&archive_romfile.path],
            )
            .await?;
        }

        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let romfile = romfiles_by_id
            .get(&roms.first().unwrap().romfile_id.unwrap())
            .unwrap();
        let source_archive_type = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(roms.first().unwrap())?
            .archive_type;

        let mut archive_romfiles_roms: Vec<(ArchiveRomfile, &Rom)> = Vec::new();

        for rom in roms {
            if source_archive_type != archive_type {
                archive_romfiles_roms.push((
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_archive(rom)?
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &romfile
                                .as_common(&mut transaction)
                                .await?
                                .path
                                .parent()
                                .unwrap(),
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid,
                        )
                        .await?,
                    rom,
                ));
            } else if recompress {
                archive_romfiles_roms.push((
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_archive(rom)?
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &tmp_directory.path(),
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid,
                        )
                        .await?,
                    rom,
                ));
            }
        }

        if let Some(archive_romfile_rom) = archive_romfiles_roms.first() {
            if check {
                let mut error = false;
                for (archive_romfile, rom) in &archive_romfiles_roms {
                    if archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                    {
                        error = true;
                        break;
                    };
                }
                if error {
                    progress_bar.println("Converted files don't match the original");
                    if source_archive_type != archive_type {
                        archive_romfile_rom
                            .0
                            .as_common()?
                            .delete(progress_bar, false)
                            .await?;
                    }
                    continue;
                }
            }

            if source_archive_type != archive_type {
                romfile
                    .as_common(&mut transaction)
                    .await?
                    .delete(progress_bar, false)
                    .await?;
                archive_romfile_rom
                    .0
                    .as_common()?
                    .update(&mut transaction, romfile.id)
                    .await?;
            } else {
                romfile
                    .as_common(&mut transaction)
                    .await?
                    .delete(progress_bar, false)
                    .await?;
                archive_romfile_rom
                    .0
                    .as_common()?
                    .rename(
                        progress_bar,
                        &romfile.as_common(&mut transaction).await?.path,
                        false,
                    )
                    .await?
                    .update(&mut transaction, romfile.id)
                    .await?;
            }
        };

        commit_transaction(transaction).await;
    }

    // convert others
    for (game_id, mut roms) in roms_by_game_id {
        let mut transaction = begin_transaction(connection).await;

        if roms.len() == 1 && !system.arcade {
            let rom = roms.first().unwrap();
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

            let archive_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .to_archive(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
                .await?;

            if check
                && archive_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                archive_romfile
                    .as_common()?
                    .delete(progress_bar, false)
                    .await?;
                continue;
            };

            archive_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[romfile],
                    &[&archive_romfile.path],
                )
                .await?;
            }

            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        } else {
            let game = games_by_id.get(&game_id).unwrap();
            roms.retain(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                !(romfile.path.ends_with(match archive_type {
                    sevenzip::ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                    sevenzip::ArchiveType::Zip => ZIP_EXTENSION,
                }))
            });
            let directory = romfiles_by_id
                .get(&roms.first().unwrap().romfile_id.unwrap())
                .unwrap()
                .as_common(&mut transaction)
                .await?
                .path
                .parent()
                .unwrap()
                .to_path_buf();

            let romfiles = roms
                .iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect::<Vec<&Romfile>>();
            let mut archive_romfiles: Vec<ArchiveRomfile> = Vec::new();
            for romfile in &romfiles {
                let archive_romfile = romfile
                    .as_common(&mut transaction)
                    .await?
                    .to_archive(
                        progress_bar,
                        &romfile
                            .as_common(&mut transaction)
                            .await?
                            .path
                            .parent()
                            .unwrap(),
                        &match system.arcade {
                            true => directory.parent().unwrap(),
                            false => directory.as_path(),
                        },
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid,
                    )
                    .await?;
                archive_romfiles.push(archive_romfile);
            }

            if check {
                let mut results: Vec<SimpleResult<()>> = Vec::new();
                for (archive_romfile, rom) in archive_romfiles.iter().zip(&roms) {
                    let result = archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await;
                    if result.is_err() {
                        progress_bar.println("Converted file doesn't match the original");
                        archive_romfile.delete_file(progress_bar).await?;
                    }
                    results.push(result);
                }
                if results.par_iter().any(|result| result.is_err()) {
                    continue;
                }
            }

            let archive_romfile_id = match find_romfile_by_path(
                &mut transaction,
                &archive_romfiles.first().unwrap().as_common()?.to_string(),
            )
            .await
            {
                Some(romfile) => romfile.id,
                None => {
                    archive_romfiles
                        .first()
                        .unwrap()
                        .as_common()?
                        .create(&mut transaction, RomfileType::Romfile)
                        .await?
                }
            };

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &romfiles,
                    &[&archive_romfiles.first().unwrap().path],
                )
                .await?;
            }

            for rom in &roms {
                delete_romfile_by_id(&mut transaction, rom.romfile_id.unwrap()).await;
                update_rom_romfile(&mut transaction, rom.id, Some(archive_romfile_id)).await;
            }
            if system.arcade {
                remove_directory(progress_bar, &directory, false).await?;
            } else {
                for rom in roms {
                    romfiles_by_id
                        .get(&rom.romfile_id.unwrap())
                        .unwrap()
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                }
            }
        }

        commit_transaction(transaction).await;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
    cd_compression_algorithms: &[String],
    cd_hunk_size: &Option<usize>,
    dvd_compression_algorithms: &[String],
    dvd_hunk_size: &Option<usize>,
    parents: bool,
    prompt_for_parents: bool,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition CUE/BINs
    let (cue_bins, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
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
    let (isos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
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
    let (csos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CSO_EXTENSION)
            })
        });

    // partition ZSOs
    let (zsos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });

    // partition CHDs
    let (mut chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
        });
    // make sure children are converted before parents
    chds.par_sort_by(|_, a, _, b| {
        b.par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .any(|romfile| romfile.parent_id.is_some())
            .cmp(
                &a.par_iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .any(|romfile| romfile.parent_id.is_some()),
            )
    });

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let romfile = romfiles.first().unwrap();

        // skip if not ISO or CUE/BIN
        if roms.len() == 1 {
            if !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
                continue;
            }
        } else {
            let is_cue_bin = roms
                .par_iter()
                .any(|rom| rom.name.ends_with(CUE_EXTENSION) || rom.name.ends_with(BIN_EXTENSION));
            if !is_cue_bin {
                continue;
            }
        }

        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let parent_chd_romfile = if prompt_for_parents {
            prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
        } else if parents {
            find_parent_chd_romfile_by_game(&mut transaction, game).await
        } else {
            None
        };

        let (cue_roms, bin_iso_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

        let cue_romfile = match cue_roms.first() {
            Some(cue_rom) => Some(
                romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_archive(cue_rom)?
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            ),
            None => None,
        };

        let mut bin_iso_romfiles: Vec<CommonRomfile> = Vec::new();
        for rom in &bin_iso_roms {
            bin_iso_romfiles.push(
                romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_archive(rom)?
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            );
        }

        let chd_romfile = match cue_romfile.as_ref() {
            Some(cue_romfile) => {
                cue_romfile
                    .as_cue_bin(
                        &bin_iso_romfiles
                            .iter()
                            .map(|bin_iso_romfile| &bin_iso_romfile.path)
                            .collect::<Vec<&PathBuf>>(),
                    )?
                    .to_chd(
                        progress_bar,
                        &romfile
                            .as_common(&mut transaction)
                            .await?
                            .path
                            .parent()
                            .unwrap(),
                        &MediaType::Cd,
                        cd_compression_algorithms,
                        cd_hunk_size,
                        &match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(
                                romfile
                                    .as_common(&mut transaction)
                                    .await
                                    .unwrap()
                                    .as_chd()
                                    .unwrap(),
                            ),
                            None => None,
                        },
                    )
                    .await?
            }
            None => {
                bin_iso_romfiles
                    .first()
                    .unwrap()
                    .as_iso()?
                    .to_chd(
                        progress_bar,
                        &romfile
                            .as_common(&mut transaction)
                            .await?
                            .path
                            .parent()
                            .unwrap(),
                        &MediaType::Dvd,
                        dvd_compression_algorithms,
                        dvd_hunk_size,
                        &match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(
                                romfile
                                    .as_common(&mut transaction)
                                    .await
                                    .unwrap()
                                    .as_chd()
                                    .unwrap(),
                            ),
                            None => None,
                        },
                    )
                    .await?
            }
        };

        if check
            && chd_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &bin_iso_roms,
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            chd_romfile.as_common()?.delete(progress_bar, false).await?;
            if let Some(cue_romfile) = cue_romfile {
                cue_romfile.delete(progress_bar, false).await?;
            }
            continue;
        };

        if diff {
            let mut new_paths = vec![&chd_romfile.path];
            if let Some(cue_romfile) = cue_romfile.as_ref() {
                new_paths.push(&cue_romfile.path)
            }
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[romfile],
                &new_paths,
            )
            .await?;
        }

        if let Some(cue_romfile) = cue_romfile {
            let cue_romfile_id = cue_romfile
                .rename(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap()
                        .join(cue_romfile.path.file_name().unwrap()),
                    false,
                )
                .await?
                .create(&mut transaction, RomfileType::Romfile)
                .await?;
            update_rom_romfile(
                &mut transaction,
                roms.par_iter()
                    .find_first(|rom| rom.name.ends_with(CUE_EXTENSION))
                    .unwrap()
                    .id,
                Some(cue_romfile_id),
            )
            .await;
        }

        chd_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;
        update_romfile_parent(
            &mut transaction,
            romfile.id,
            parent_chd_romfile.as_ref().map(|romfile| romfile.id),
        )
        .await;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert CUE/BIN
    for roms in cue_bins.values() {
        let mut transaction = begin_transaction(connection).await;

        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let parent_chd_romfile = if prompt_for_parents {
            prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
        } else if parents {
            find_parent_chd_romfile_by_game(&mut transaction, game).await
        } else {
            None
        };
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.first().unwrap().romfile_id.unwrap())
            .unwrap();
        let bin_romfiles = bin_roms
            .iter()
            .map(|bin_rom| romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap())
            .collect::<Vec<&Romfile>>();
        let mut bin_paths: Vec<PathBuf> = Vec::new();
        for bin_romfile in &bin_romfiles {
            bin_paths.push(bin_romfile.as_common(&mut transaction).await?.path);
        }
        let chd_romfile = cue_romfile
            .as_common(&mut transaction)
            .await?
            .as_cue_bin(&bin_paths)?
            .to_chd(
                progress_bar,
                &cue_romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &MediaType::Cd,
                cd_compression_algorithms,
                cd_hunk_size,
                &match parent_chd_romfile.as_ref() {
                    Some(romfile) => Some(
                        romfile
                            .as_common(&mut transaction)
                            .await
                            .unwrap()
                            .as_chd()
                            .unwrap(),
                    ),
                    None => None,
                },
            )
            .await?;

        if check
            && chd_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &bin_roms,
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            chd_romfile.as_common()?.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            let roms = [cue_roms.as_slice(), bin_roms.as_slice()].concat();
            let romfiles = [&[cue_romfile], bin_romfiles.as_slice()].concat();
            print_diff(
                &mut transaction,
                progress_bar,
                &roms,
                &romfiles,
                &[&chd_romfile.path],
            )
            .await?;
        }

        let chd_romfile_id = chd_romfile
            .as_common()?
            .create(&mut transaction, RomfileType::Romfile)
            .await?;
        update_romfile_parent(
            &mut transaction,
            chd_romfile_id,
            parent_chd_romfile.as_ref().map(|romfile| romfile.id),
        )
        .await;
        for bin_rom in bin_roms {
            let bin_romfile = romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap();
            update_rom_romfile(&mut transaction, bin_rom.id, Some(chd_romfile_id)).await;
            delete_romfile_by_id(&mut transaction, bin_romfile.id).await;
            bin_romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let parent_chd_romfile = if prompt_for_parents {
                prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
            } else if parents {
                find_parent_chd_romfile_by_game(&mut transaction, game).await
            } else {
                None
            };
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let chd_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_iso()?
                .to_chd(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &MediaType::Dvd,
                    dvd_compression_algorithms,
                    dvd_hunk_size,
                    &match parent_chd_romfile.as_ref() {
                        Some(romfile) => Some(
                            romfile
                                .as_common(&mut transaction)
                                .await
                                .unwrap()
                                .as_chd()
                                .unwrap(),
                        ),
                        None => None,
                    },
                )
                .await?;
            if check
                && chd_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                chd_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&chd_romfile.path],
                )
                .await?;
            }
            chd_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    for roms in csos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let parent_chd_romfile = if prompt_for_parents {
                prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
            } else if parents {
                find_parent_chd_romfile_by_game(&mut transaction, game).await
            } else {
                None
            };
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let chd_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_chd(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &MediaType::Dvd,
                    dvd_compression_algorithms,
                    dvd_hunk_size,
                    &match parent_chd_romfile.as_ref() {
                        Some(romfile) => Some(
                            romfile
                                .as_common(&mut transaction)
                                .await
                                .unwrap()
                                .as_chd()
                                .unwrap(),
                        ),
                        None => None,
                    },
                )
                .await?;
            if check
                && chd_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                chd_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&chd_romfile.path],
                )
                .await?;
            }
            chd_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let parent_chd_romfile = if prompt_for_parents {
                prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
            } else if parents {
                find_parent_chd_romfile_by_game(&mut transaction, game).await
            } else {
                None
            };
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let chd_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_chd(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &MediaType::Dvd,
                    dvd_compression_algorithms,
                    dvd_hunk_size,
                    &match parent_chd_romfile.as_ref() {
                        Some(romfile) => Some(
                            romfile
                                .as_common(&mut transaction)
                                .await
                                .unwrap()
                                .as_chd()
                                .unwrap(),
                        ),
                        None => None,
                    },
                )
                .await?;
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&chd_romfile.path],
                )
                .await?;
            }
            if check
                && chd_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                chd_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            chd_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CHDs
    if recompress {
        for roms in chds.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
            let parent_chd_romfile = if prompt_for_parents {
                prompt_for_parent_romfile(&mut transaction, game, CHD_EXTENSION).await?
            } else if parents {
                find_parent_chd_romfile_by_game(&mut transaction, game).await
            } else {
                None
            };

            if roms.len() == 1 {
                let romfile = romfiles_by_id
                    .get(&roms.first().unwrap().romfile_id.unwrap())
                    .unwrap();
                if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                    .await
                    .is_empty()
                {
                    progress_bar.println("CHD has children, skipping");
                    continue;
                }
                let chd_romfile = match romfile.parent_id {
                    Some(parent_id) => {
                        let parent_chd_romfile =
                            find_romfile_by_id(&mut transaction, parent_id).await;
                        romfile
                            .as_common(&mut transaction)
                            .await?
                            .as_chd_with_parent(
                                &parent_chd_romfile
                                    .as_common(&mut transaction)
                                    .await?
                                    .as_chd()?,
                            )?
                    }
                    None => romfile.as_common(&mut transaction).await?.as_chd()?,
                };
                let chd_romfile = chd_romfile
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .to_chd(
                        progress_bar,
                        &tmp_directory.path(),
                        &MediaType::Dvd,
                        dvd_compression_algorithms,
                        dvd_hunk_size,
                        &match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(
                                romfile
                                    .as_common(&mut transaction)
                                    .await
                                    .unwrap()
                                    .as_chd()
                                    .unwrap(),
                            ),
                            None => None,
                        },
                    )
                    .await?;

                if check
                    && chd_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[roms.first().unwrap()],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    chd_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&chd_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    chd_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                    update_romfile_parent(
                        &mut transaction,
                        romfile.id,
                        parent_chd_romfile.as_ref().map(|romfile| romfile.id),
                    )
                    .await;
                };
            } else {
                let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                    .iter()
                    .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

                let mut romfiles: Vec<&Romfile> = bin_roms
                    .iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .collect();
                romfiles.dedup();

                if romfiles.len() > 1 {
                    bail!("Multiple CHDs found");
                }

                let romfile = romfiles.first().unwrap();
                if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                    .await
                    .is_empty()
                {
                    progress_bar.println("CHD has children, skipping");
                    continue;
                }
                let cue_romfile = romfiles_by_id
                    .get(&cue_roms.first().unwrap().romfile_id.unwrap())
                    .unwrap()
                    .as_common(&mut transaction)
                    .await?;

                let chd_romfile = match romfile.parent_id {
                    Some(parent_id) => {
                        let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                            .await
                            .as_common(&mut transaction)
                            .await?
                            .as_chd()?;
                        romfile
                            .as_common(&mut transaction)
                            .await?
                            .as_chd_with_cue_and_parent(&cue_romfile, &parent_chd_romfile)?
                    }
                    None => romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_cue(&cue_romfile)?,
                };

                let chd_romfile = chd_romfile
                    .to_cue_bin(progress_bar, &tmp_directory.path(), &bin_roms, false)
                    .await?
                    .to_chd(
                        progress_bar,
                        &tmp_directory.path(),
                        &MediaType::Cd,
                        cd_compression_algorithms,
                        cd_hunk_size,
                        &match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(
                                romfile
                                    .as_common(&mut transaction)
                                    .await
                                    .unwrap()
                                    .as_chd()
                                    .unwrap(),
                            ),
                            None => None,
                        },
                    )
                    .await?;

                if check
                    && chd_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &bin_roms,
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    chd_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&chd_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    chd_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                    update_romfile_parent(
                        &mut transaction,
                        romfile.id,
                        parent_chd_romfile.as_ref().map(|romfile| romfile.id),
                    )
                    .await;
                };
            }

            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition ISOs
    let (isos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ISO_EXTENSION)
            })
        });

    // partition CHDs
    let (mut chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
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
    // make sure children are converted before parents
    chds.par_sort_by(|_, a, _, b| {
        b.par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .any(|romfile| romfile.parent_id.is_some())
            .cmp(
                &a.par_iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .any(|romfile| romfile.parent_id.is_some()),
            )
    });

    // partition ZSOs
    let (zsos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });

    // partition CSOs
    let (csos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
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

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }

        let rom = roms.first().unwrap();
        let romfile = romfiles.first().unwrap();

        let cso_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(
                progress_bar,
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &maxcso::XsoType::Cso,
            )
            .await?;

        if check
            && cso_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            cso_romfile.as_common()?.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &[rom],
                &[romfile],
                &[&cso_romfile.path],
            )
            .await?;
        }

        cso_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let cso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_iso()?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Cso,
                )
                .await?;
            if check
                && cso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                cso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&cso_romfile.path],
                )
                .await?;
            }

            cso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CHDs
    for roms in chds.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_parent(&parent_chd_romfile)?
                }
                None => romfile.as_common(&mut transaction).await?.as_chd()?,
            };
            let cso_romfile = chd_romfile
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Cso,
                )
                .await?;
            if check
                && cso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                cso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&cso_romfile.path],
                )
                .await?;
            }

            cso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            if romfile.parent_id.is_some() {
                update_romfile_parent(&mut transaction, romfile.id, None).await;
            }
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let cso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Cso,
                )
                .await?;
            if check
                && cso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                cso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&cso_romfile.path],
                )
                .await?;
            }

            cso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    if recompress {
        for roms in csos.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            for rom in roms {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                let cso_romfile = romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .to_xso(progress_bar, &tmp_directory.path(), &XsoType::Cso)
                    .await?;

                if check
                    && cso_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    cso_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&cso_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    cso_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                };
            }

            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_nsz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition NSPs
    let (nsps, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSP_EXTENSION)
            })
        });

    // partition NSZs
    let (nszs, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSZ_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(NSP_EXTENSION) {
            continue;
        }

        let rom = roms.first().unwrap();
        let romfile = romfiles.first().unwrap();

        let nsz_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_nsp()?
            .to_nsz(
                progress_bar,
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
            )
            .await?;

        if check
            && nsz_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            nsz_romfile.as_common()?.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &[rom],
                &[romfile],
                &[&nsz_romfile.path],
            )
            .await?;
        }

        nsz_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert NSPs
    for roms in nsps.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let nsz_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_nsp()?
                .to_nsz(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;
            if check
                && nsz_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                nsz_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&nsz_romfile.path],
                )
                .await?;
            }
            nsz_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert NSZs
    if recompress {
        for roms in nszs.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            for rom in roms {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                let nsz_romfile = romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_nsz()?
                    .to_nsp(progress_bar, &tmp_directory.path())
                    .await?
                    .to_nsz(progress_bar, &tmp_directory.path())
                    .await?;

                if check
                    && nsz_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    nsz_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&nsz_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    nsz_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                };
            }

            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_rvz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition ISOs
    let (isos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ISO_EXTENSION)
            })
        });

    // partition RVZs
    let (rvzs, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(RVZ_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }

        let rom = roms.first().unwrap();
        let romfile = romfiles.first().unwrap();

        let rvz_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_rvz(
                progress_bar,
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                compression_algorithm,
                compression_level,
                block_size,
                false,
            )
            .await?;

        if check
            && rvz_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            rvz_romfile.as_common()?.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &[rom],
                &[romfile],
                &[&rvz_romfile.path],
            )
            .await?;
        }

        rvz_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let rvz_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_iso()?
                .to_rvz(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    compression_algorithm,
                    compression_level,
                    block_size,
                    false,
                )
                .await?;
            if check
                && rvz_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                rvz_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&rvz_romfile.path],
                )
                .await?;
            }
            rvz_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert RVZs
    if recompress {
        for roms in rvzs.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            for rom in roms {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                let rvz_romfile = romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_rvz()?
                    .to_iso(
                        progress_bar,
                        &romfile
                            .as_common(&mut transaction)
                            .await?
                            .path
                            .parent()
                            .unwrap(),
                    )
                    .await?
                    .to_rvz(
                        progress_bar,
                        &tmp_directory.path(),
                        compression_algorithm,
                        compression_level,
                        block_size,
                        false,
                    )
                    .await?;

                if check
                    && rvz_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    rvz_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&rvz_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    rvz_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                };
            }

            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_zso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition ISOs
    let (isos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ISO_EXTENSION)
            })
        });

    // partition CHDs
    let (mut chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
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
    // make sure children are converted before parents
    chds.par_sort_by(|_, a, _, b| {
        b.par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .any(|romfile| romfile.parent_id.is_some())
            .cmp(
                &a.par_iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .any(|romfile| romfile.parent_id.is_some()),
            )
    });

    // partition CSOs
    let (csos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CSO_EXTENSION)
            })
        });

    // partition ZSOs
    let (zsos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }

        let rom = roms.first().unwrap();
        let romfile = romfiles.first().unwrap();

        let zso_romfile = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(
                progress_bar,
                &romfile
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap(),
                &maxcso::XsoType::Zso,
            )
            .await?;

        if check
            && zso_romfile
                .check(
                    &mut transaction,
                    progress_bar,
                    &None,
                    &[rom],
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            zso_romfile.as_common()?.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &[rom],
                &[romfile],
                &[&zso_romfile.path],
            )
            .await?;
        }

        zso_romfile
            .as_common()?
            .update(&mut transaction, romfile.id)
            .await?;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let zso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_iso()?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Zso,
                )
                .await?;
            if check
                && zso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                zso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&zso_romfile.path],
                )
                .await?;
            }
            zso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CHDs
    for roms in chds.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_parent(&parent_chd_romfile)?
                }
                None => romfile.as_common(&mut transaction).await?.as_chd()?,
            };
            let zso_romfile = chd_romfile
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Zso,
                )
                .await?;
            if check
                && zso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                zso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&zso_romfile.path],
                )
                .await?;
            }
            zso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            if romfile.parent_id.is_some() {
                update_romfile_parent(&mut transaction, romfile.id, None).await;
            }
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    for roms in csos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let zso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_xso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &maxcso::XsoType::Zso,
                )
                .await?;
            if check
                && zso_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                zso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[romfile],
                    &[&zso_romfile.path],
                )
                .await?;
            }
            zso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    if recompress {
        for roms in zsos.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            for rom in roms {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                let zso_romfile = romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .to_xso(progress_bar, &tmp_directory.path(), &XsoType::Zso)
                    .await?;

                if check
                    && zso_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                {
                    progress_bar.println("Converted file doesn't match the original");
                    zso_romfile.as_common()?.delete(progress_bar, false).await?;
                    continue;
                } else {
                    if diff {
                        print_diff(
                            &mut transaction,
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[romfile],
                            &[&zso_romfile.path],
                        )
                        .await?;
                    }
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    zso_romfile
                        .as_common()?
                        .rename(
                            progress_bar,
                            &romfile.as_common(&mut transaction).await?.path,
                            false,
                        )
                        .await?
                        .update(&mut transaction, romfile.id)
                        .await?;
                };
            }

            commit_transaction(transaction).await;
        }
    }
    Ok(())
}

async fn to_original(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition CHDs
    let (mut chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
        });
    // make sure children are converted before parents
    chds.par_sort_by(|_, a, _, b| {
        b.par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .any(|romfile| romfile.parent_id.is_some())
            .cmp(
                &a.par_iter()
                    .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                    .any(|romfile| romfile.parent_id.is_some()),
            )
    });

    // partition CSOs
    let (csos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CSO_EXTENSION)
            })
        });

    // partition NSZs
    let (nszs, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSP_EXTENSION)
            })
        });

    // partition RVZs
    let (rvzs, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(RVZ_EXTENSION)
            })
        });

    // partition ZSOs
    let (zsos, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });

    // drop originals
    drop(others);

    // convert archives
    for roms in archives.values() {
        if sevenzip::get_version().await.is_err() {
            progress_bar.println("Please install sevenzip");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .filter(|romfile| {
                romfile.path.ends_with(SEVENZIP_EXTENSION) || romfile.path.ends_with(ZIP_EXTENSION)
            })
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let romfile = romfiles.first().unwrap();
        let roms: Vec<&Rom> = roms
            .iter()
            .filter(|rom| rom.romfile_id.unwrap() == romfile.id)
            .collect();

        let destination_directory = match system.arcade {
            true => {
                let romfile_path = romfile.as_common(&mut transaction).await?.path;
                let directory = romfile_path
                    .parent()
                    .unwrap()
                    .join(romfile_path.file_stem().unwrap());
                create_directory(progress_bar, &directory, false).await?;
                directory
            }
            false => romfile
                .as_common(&mut transaction)
                .await?
                .path
                .parent()
                .unwrap()
                .to_path_buf(),
        };

        let mut common_romfiles: Vec<CommonRomfile> = Vec::new();
        for rom in &roms {
            let common_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_archive(rom)?
                .to_common(progress_bar, &destination_directory)
                .await?;
            common_romfiles.push(common_romfile);
        }

        if check {
            let mut error = false;
            for (common_romfile, rom) in common_romfiles
                .iter()
                .zip(roms.iter())
                .collect::<Vec<(&CommonRomfile, &&Rom)>>()
            {
                if common_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
                {
                    error = true;
                    break;
                };
            }
            if error {
                progress_bar.println("Converted files don't match the original");
                for common_romfile in common_romfiles {
                    common_romfile.delete(progress_bar, false).await?;
                }
                continue;
            }
        }

        for (common_romfile, rom) in common_romfiles
            .iter()
            .zip(roms.iter())
            .collect::<Vec<(&CommonRomfile, &&Rom)>>()
        {
            let romfile_id = common_romfile
                .create(&mut transaction, RomfileType::Romfile)
                .await?;
            update_rom_romfile(&mut transaction, rom.id, Some(romfile_id)).await;
        }

        delete_romfile_by_id(&mut transaction, romfile.id).await;
        romfile
            .as_common(&mut transaction)
            .await?
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert CHDs
    for roms in chds.values() {
        if chdman::get_version().await.is_err() {
            progress_bar.println("Please install chdman");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        if roms.len() == 1 {
            let romfile = romfiles_by_id
                .get(&roms.first().unwrap().romfile_id.unwrap())
                .unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_parent(&parent_chd_romfile)?
                }
                None => romfile.as_common(&mut transaction).await?.as_chd()?,
            };
            let iso_romfile = chd_romfile
                .to_iso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;

            if check
                && iso_romfile
                    .as_common()?
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                iso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            iso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            if romfile.parent_id.is_some() {
                update_romfile_parent(&mut transaction, romfile.id, None).await;
            }
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        } else {
            let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                .iter()
                .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

            let mut romfiles: Vec<&Romfile> = bin_roms
                .iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect();
            romfiles.dedup();

            if romfiles.len() > 1 {
                bail!("Multiple CHDs found");
            }

            let romfile = romfiles.first().unwrap();
            if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
                .await
                .is_empty()
            {
                progress_bar.println("CHD has children, skipping");
                continue;
            }
            let cue_romfile = romfiles_by_id
                .get(&cue_roms.first().unwrap().romfile_id.unwrap())
                .unwrap()
                .as_common(&mut transaction)
                .await?;
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id)
                        .await
                        .as_common(&mut transaction)
                        .await?
                        .as_chd()?;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_cue_and_parent(&cue_romfile, &parent_chd_romfile)?
                }
                None => romfile
                    .as_common(&mut transaction)
                    .await?
                    .as_chd_with_cue(&cue_romfile)?,
            };
            let cue_bin_romfile = chd_romfile
                .to_cue_bin(
                    progress_bar,
                    &cue_romfile.path.parent().unwrap(),
                    &bin_roms,
                    false,
                )
                .await?;

            if check {
                let mut error = false;
                for (bin_romfile, bin_rom) in cue_bin_romfile
                    .bin_romfiles
                    .iter()
                    .zip(&bin_roms)
                    .collect::<Vec<(&CommonRomfile, &&Rom)>>()
                {
                    if bin_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[bin_rom],
                            hash_algorithm,
                        )
                        .await
                        .is_err()
                    {
                        error = true;
                        break;
                    };
                }
                if error {
                    progress_bar.println("Converted files don't match the original");
                    for bin_romfile in cue_bin_romfile.bin_romfiles {
                        bin_romfile.delete(progress_bar, false).await?;
                    }
                    continue;
                }
            }

            for (bin_romfile, bin_rom) in cue_bin_romfile
                .bin_romfiles
                .iter()
                .zip(&bin_roms)
                .collect::<Vec<(&CommonRomfile, &&Rom)>>()
            {
                let romfile_id = bin_romfile
                    .create(&mut transaction, RomfileType::Romfile)
                    .await?;
                update_rom_romfile(&mut transaction, bin_rom.id, Some(romfile_id)).await;
            }
            delete_romfile_by_id(&mut transaction, romfile.id).await;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    for roms in csos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;

            if check
                && iso_romfile
                    .as_common()?
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                iso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            iso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert NSZs
    for roms in nszs.values() {
        if nsz::get_version().await.is_err() {
            progress_bar.println("Please install nsz");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let nsp_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_nsz()?
                .to_nsp(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;

            if check
                && nsp_romfile
                    .as_common()?
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                nsp_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            nsp_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert RVZs
    for roms in rvzs.values() {
        if dolphin::get_version().await.is_err() {
            progress_bar.println("Please install dolphin-tool");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_rvz()?
                .to_iso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;

            if check
                && iso_romfile
                    .as_common()?
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                iso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            iso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    for roms in zsos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }

        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let iso_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()?
                .to_iso(
                    progress_bar,
                    &romfile
                        .as_common(&mut transaction)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                )
                .await?;

            if check
                && iso_romfile
                    .as_common()?
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[rom],
                        hash_algorithm,
                    )
                    .await
                    .is_err()
            {
                progress_bar.println("Converted file doesn't match the original");
                iso_romfile.as_common()?.delete(progress_bar, false).await?;
                continue;
            };

            iso_romfile
                .as_common()?
                .update(&mut transaction, romfile.id)
                .await?;
            romfile
                .as_common(&mut transaction)
                .await?
                .delete(progress_bar, false)
                .await?;
        }

        commit_transaction(transaction).await;
    }

    Ok(())
}

async fn print_diff<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms: &[&Rom],
    romfiles: &[&Romfile],
    paths: &[&P],
) -> SimpleResult<()> {
    let original_size = roms.par_iter().map(|&r| r.size as u64).sum();
    let mut old_size = 0u64;
    for &romfile in romfiles {
        old_size += romfile.as_common(connection).await?.get_size().await?;
    }
    let mut new_size = 0u64;
    for &path in paths {
        new_size += CommonRomfile::from_path(path)?.get_size().await?;
    }
    progress_bar.println(format!(
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
mod test_chd_parents_to_chd_should_not_touch_parent;
#[cfg(test)]
mod test_chd_parents_to_iso;
#[cfg(test)]
mod test_cso_to_chd;
#[cfg(test)]
mod test_cso_to_cso;
#[cfg(test)]
mod test_cso_to_iso;
#[cfg(test)]
mod test_cso_to_sevenzip_iso;
#[cfg(test)]
mod test_cso_to_zso;
#[cfg(test)]
mod test_iso_chd_to_chd;
#[cfg(test)]
mod test_iso_chd_to_cso;
#[cfg(test)]
mod test_iso_chd_to_iso;
#[cfg(test)]
mod test_iso_chd_to_sevenzip_iso;
#[cfg(test)]
mod test_iso_chd_to_zso;
#[cfg(test)]
mod test_iso_to_chd;
#[cfg(test)]
mod test_iso_to_chd_parents;
#[cfg(test)]
mod test_iso_to_cso;
#[cfg(test)]
mod test_iso_to_rvz;
#[cfg(test)]
mod test_iso_to_zso;
#[cfg(test)]
mod test_multiple_tracks_chd_to_chd;
#[cfg(test)]
mod test_multiple_tracks_chd_to_cso_should_do_nothing;
#[cfg(test)]
mod test_multiple_tracks_chd_to_cue_bin;
#[cfg(test)]
mod test_multiple_tracks_chd_to_sevenzip_cue_bin;
#[cfg(test)]
mod test_multiple_tracks_chd_to_zso_should_do_nothing;
#[cfg(test)]
mod test_multiple_tracks_cue_bin_to_chd;
#[cfg(test)]
mod test_original_to_sevenzip;
#[cfg(test)]
mod test_original_to_zip;
#[cfg(test)]
mod test_original_to_zip_multiple_roms;
#[cfg(test)]
mod test_original_to_zip_with_correct_name;
#[cfg(test)]
mod test_original_to_zip_with_incorrect_name;
#[cfg(test)]
mod test_rvz_to_iso;
#[cfg(test)]
mod test_rvz_to_rvz;
#[cfg(test)]
mod test_rvz_to_sevenzip_iso;
#[cfg(test)]
mod test_sevenzip_iso_to_chd;
#[cfg(test)]
mod test_sevenzip_iso_to_cso;
#[cfg(test)]
mod test_sevenzip_iso_to_zso;
#[cfg(test)]
mod test_sevenzip_multiple_tracks_cue_bin_to_chd;
#[cfg(test)]
mod test_sevenzip_to_original;
#[cfg(test)]
mod test_sevenzip_to_sevenzip;
#[cfg(test)]
mod test_sevenzip_to_zip;
#[cfg(test)]
mod test_sevenzip_to_zip_multiple_files;
#[cfg(test)]
mod test_zip_to_original;
#[cfg(test)]
mod test_zip_to_sevenzip;
#[cfg(test)]
mod test_zip_to_zip;
#[cfg(test)]
mod test_zip_to_zip_should_do_nothing;
#[cfg(test)]
mod test_zso_to_chd;
#[cfg(test)]
mod test_zso_to_cso;
#[cfg(test)]
mod test_zso_to_iso;
#[cfg(test)]
mod test_zso_to_sevenzip_iso;
#[cfg(test)]
mod test_zso_to_zso;
