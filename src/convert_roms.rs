#[cfg(feature = "chd")]
use super::chdman;
#[cfg(feature = "chd")]
use super::chdman::{AsChd, AsCueBin, ToChd, ToCueBin};
use super::common::*;
use super::config::*;
use super::database::*;
#[cfg(feature = "rvz")]
use super::dolphin;
#[cfg(feature = "rvz")]
use super::dolphin::{AsRvz, ToRvz};
#[cfg(any(feature = "cso", feature = "zso"))]
use super::maxcso;
use super::maxcso::{AsXso, ToXso};
use super::model::*;
#[cfg(feature = "nsz")]
use super::nsz;
#[cfg(feature = "nsz")]
use super::nsz::{AsNsp, AsNsz, ToNsp, ToNsz};
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::{ArchiveFile, ArchiveRomfile, AsArchive, ToArchive};
use super::util::*;
use super::SimpleResult;
use cfg_if::cfg_if;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::{HumanBytes, ProgressBar};
use lazy_static::lazy_static;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::drop;
use std::path::{Path, PathBuf};
use std::str::FromStr;

lazy_static! {
    static ref ALL_FORMATS: Vec<&'static str> = {
        let mut all_formats = vec!["ORIGINAL", "7Z", "ZIP"];
        cfg_if! {
            if #[cfg(feature = "chd")] {
                all_formats.push("CHD");
            }
        }
        cfg_if! {
            if #[cfg(feature = "cso")] {
                all_formats.push("CSO");
            }
        }
        cfg_if! {
            if #[cfg(feature = "nsz")] {
                all_formats.push("NSZ");
            }
        }
        cfg_if! {
            if #[cfg(feature = "rvz")] {
                all_formats.push("RVZ");
            }
        }
        cfg_if! {
            if #[cfg(feature = "zso")] {
                all_formats.push("ZSO");
            }
        }
        all_formats
    };
}
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
            Arg::new("NAME")
                .short('n')
                .long("name")
                .help("Select games by name")
                .required(false)
                .num_args(1),
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
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, matches.get_flag("ALL")).await?;
    let game_name = matches.get_one::<String>("NAME");
    let format = match matches.get_one::<String>("FORMAT") {
        Some(format) => format.as_str().to_owned(),
        None => ALL_FORMATS
            .get(select(&ALL_FORMATS, "Please select a format", None, None)?)
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
            cfg_if! {
                if #[cfg(feature = "chd")] {
                    if chdman::get_version().await.is_err() {
                        progress_bar.println("Please install chdman");
                        return Ok(());
                    }
                }
            }
        }
        "CSO" => {
            cfg_if! {
                if #[cfg(feature = "cso")] {
                    if maxcso::get_version().await.is_err() {
                        progress_bar.println("Please install maxcso");
                        return Ok(());
                    }
                }
            }
        }
        "NSZ" => {
            cfg_if! {
                if #[cfg(feature = "nsz")] {
                    if nsz::get_version().await.is_err() {
                        progress_bar.println("Please install nsz");
                        return Ok(());
                    }
                }
            }
        }
        "RVZ" => {
            cfg_if! {
                if #[cfg(feature = "rvz")] {
                    if dolphin::get_version().await.is_err() {
                        progress_bar.println("Please install dolphin");
                        return Ok(());
                    }
                }
            }
        }
        "ZSO" => {
            cfg_if! {
                if #[cfg(feature = "zso")] {
                    if maxcso::get_version().await.is_err() {
                        progress_bar.println("Please install maxcso");
                        return Ok(());
                    }
                }
            }
        }
        _ => bail!("Not supported"),
    }

    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));

        if format == "CHD" && system.name.contains("Dreamcast") {
            if chdman::get_version()
                .await?
                .as_str()
                .cmp(chdman::MIN_DREAMCAST_VERSION)
                == Ordering::Less
            {
                progress_bar.println(format!("Older chdman versions have issues with Dreamcast games, please update to {} or newer", chdman::MIN_DREAMCAST_VERSION));
                continue;
            }
            continue;
        }

        if system.arcade && !ARCADE_FORMATS.contains(&format.as_str()) {
            progress_bar.println(format!(
                "Only {:?} are supported for arcade systems",
                ARCADE_FORMATS
            ));
            continue;
        }

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
            continue;
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

        let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
        roms.into_iter().for_each(|rom| {
            let group = roms_by_game_id.entry(rom.game_id).or_default();
            group.push(rom);
        });
        let games_by_id: HashMap<i64, Game> =
            games.into_iter().map(|game| (game.id, game)).collect();
        let romfiles_by_id: HashMap<i64, Romfile> = romfiles
            .into_iter()
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
                let solid: bool = get_bool(connection, "SEVENZIP_SOLID_COMPRESSION").await;
                to_archive(
                    connection,
                    progress_bar,
                    sevenzip::ArchiveType::Sevenzip,
                    &system,
                    roms_by_game_id,
                    games_by_id,
                    romfiles_by_id,
                    diff,
                    check,
                    &hash_algorithm,
                    compression_level,
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
                    diff,
                    check,
                    &hash_algorithm,
                    compression_level,
                    false,
                )
                .await?
            }
            "CHD" => {
                cfg_if! {
                    if #[cfg(feature = "chd")] {
                        to_chd(
                            connection,
                            progress_bar,
                            roms_by_game_id,
                            romfiles_by_id,
                            diff,
                            check,
                            &hash_algorithm,
                        )
                        .await?
                    }
                }
            }
            "CSO" => {
                cfg_if! {
                    if #[cfg(feature = "cso")] {
                        to_cso(
                            connection,
                            progress_bar,
                            roms_by_game_id,
                            romfiles_by_id,
                            diff,
                            check,
                            &hash_algorithm,
                        )
                        .await?
                    }
                }
            }
            "NSZ" => {
                cfg_if! {
                    if #[cfg(feature = "nsz")] {
                        to_nsz(
                            connection,
                            progress_bar,
                            roms_by_game_id,
                            romfiles_by_id,
                            diff,
                            check,
                            &hash_algorithm,
                        )
                        .await?
                    }
                }
            }
            "RVZ" => {
                cfg_if! {
                    if #[cfg(feature = "rvz")] {
                        let compression_algorithm = RvzCompressionAlgorithm::from_str(&get_string(connection, "RVZ_COMPRESSION_ALGORITHM").await).unwrap();
                        let compression_level = get_integer(connection, "RVZ_COMPRESSION_LEVEL").await;
                        let block_size = get_integer(connection, "RVZ_BLOCK_SIZE").await;
                        to_rvz(
                            connection,
                            progress_bar,
                            roms_by_game_id,
                            romfiles_by_id,
                            diff,
                            check,
                            &hash_algorithm,
                            &compression_algorithm,
                            compression_level,
                            block_size,
                        )
                        .await?
                    }
                }
            }
            "ZSO" => {
                cfg_if! {
                    if #[cfg(feature = "zso")] {
                        to_zso(
                            connection,
                            progress_bar,
                            roms_by_game_id,
                            romfiles_by_id,
                            diff,
                            check,
                            &hash_algorithm,
                        )
                        .await?
                    }
                }
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
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    games_by_id: HashMap<i64, Game>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
    compression_level: usize,
    solid: bool,
) -> SimpleResult<()> {
    // partition CHDs
    let (chds, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
        });
    cfg_if! {
        if #[cfg(not(feature = "chd"))] {
            drop(chds)
        }
    }

    // partition CSOs
    let (csos, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CSO_EXTENSION)
            })
        });
    cfg_if! {
        if #[cfg(not(feature = "cso"))] {
            drop(csos)
        }
    }

    // partition NSZs
    let (nszs, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSZ_EXTENSION)
            })
        });
    cfg_if! {
        if #[cfg(not(feature = "nsz"))] {
            drop(nszs)
        }
    }

    // partition RVZs
    let (rvzs, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(RVZ_EXTENSION)
            })
        });
    cfg_if! {
        if #[cfg(not(feature = "rvz"))] {
            drop(rvzs)
        }
    }

    // partition ZSOs
    let (zsos, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(ZSO_EXTENSION)
            })
        });
    cfg_if! {
        if #[cfg(not(feature = "zso"))] {
            drop(zsos)
        }
    }

    // partition archives
    let (archives, roms_by_game_id): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let path = &romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap().path;
                path.ends_with(SEVENZIP_EXTENSION) || path.ends_with(ZIP_EXTENSION)
            })
        });

    // convert CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                if roms.len() == 1 {
                    let rom = roms.first().unwrap();
                    let game = games_by_id.get(&rom.game_id).unwrap();
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let archive_romfile = romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .as_common()?
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &romfile.as_common()?.path.parent().unwrap(),
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &archive_romfile.as_common()?.to_string(),
                        archive_romfile.as_common()?.get_size().await?,
                    )
                    .await;

                    if diff {
                        print_diff(
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[&romfile.path],
                            &[&archive_romfile.path],
                        )
                        .await?;
                    }

                    romfile.as_common()?.delete(progress_bar, false).await?;
                } else {
                    let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                        .into_par_iter()
                        .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
                    let cue_rom = cue_roms.first().unwrap();
                    let game = games_by_id.get(&cue_rom.game_id).unwrap();
                    let cue_romfile = romfiles_by_id.get(&cue_rom.romfile_id.unwrap()).unwrap();
                    let chd_romfile = romfiles_by_id
                        .get(&bin_roms.first().unwrap().romfile_id.unwrap())
                        .unwrap();

                    let mut archive_romfiles: Vec<ArchiveRomfile> = Vec::new();
                    let destination_directory = Path::new(&cue_romfile.path).parent().unwrap();
                    let archive_romfile = cue_romfile
                        .as_common()?
                        .to_archive(
                            progress_bar,
                            &cue_romfile.as_common()?.path.parent().unwrap(),
                            &destination_directory,
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
                        )
                        .await?;
                    archive_romfiles.push(archive_romfile);
                    let cue_bin_romfile = chd_romfile
                        .as_chd_with_cue(&cue_romfile.path)?
                        .to_cue_bin(progress_bar, &tmp_directory.path(), &cue_romfile.as_common()?, &bin_roms, true)
                        .await?;

                    for bin_romfile in cue_bin_romfile.bin_romfiles {
                        let archive_romfile = bin_romfile.to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &destination_directory,
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
                        )
                        .await?;
                        archive_romfiles.push(archive_romfile);
                    }

                    if check {
                        let mut error = false;
                        let roms = [cue_roms.as_slice(), bin_roms.as_slice()].concat();
                        for (archive_romfile, rom) in archive_romfiles.iter().zip(roms) {
                            if archive_romfile.check(&mut transaction, progress_bar, &None, &[rom], hash_algorithm)
                                .await
                                .is_err()
                            {
                                error = true;
                                break;
                            }
                        }
                        if error {
                            progress_bar.println("Converted file doesn't match the original");
                            archive_romfiles.first().unwrap().as_common()?.delete(progress_bar, false).await?;
                            continue;
                        }
                    }

                    update_romfile(
                        &mut transaction,
                        chd_romfile.id,
                        &archive_romfiles.first().unwrap().as_common()?.to_string(),
                        archive_romfiles.first().unwrap().as_common()?.get_size().await?,
                    )
                    .await;
                    update_rom_romfile(&mut transaction, cue_rom.id, Some(chd_romfile.id)).await;
                    delete_romfile_by_id(&mut transaction, cue_romfile.id).await;

                    if diff {
                        print_diff(
                            progress_bar,
                            &roms.iter().collect::<Vec<&Rom>>(),
                            &[&cue_romfile.path, &chd_romfile.path],
                            &[&archive_romfiles.first().unwrap().path],
                        )
                        .await?;
                    }

                    cue_romfile.as_common()?.delete(progress_bar, false).await?;
                    chd_romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    // convert CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

                let archive_romfile = romfile
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &romfile.as_common()?.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
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

                update_romfile(
                    &mut transaction,
                    romfile.id,
                    &archive_romfile.as_common()?.to_string(),
                    archive_romfile.as_common()?.get_size().await?,
                )
                .await;

                if diff {
                    print_diff(
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&romfile.path],
                        &[&archive_romfile.path],
                    )
                    .await?;
                }

                romfile.as_common()?.delete(progress_bar, false).await?;

                commit_transaction(transaction).await;
            }
        }
    }

    // convert NSZs
    cfg_if! {
        if #[cfg(feature = "nsz")] {
            for roms in nszs.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

                let archive_romfile = romfile
                    .as_nsz()?
                    .to_nsp(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &romfile.as_common()?.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
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

                update_romfile(
                    &mut transaction,
                    romfile.id,
                    &archive_romfile.as_common()?.to_string(),
                    archive_romfile.as_common()?.get_size().await?
                )
                .await;

                if diff {
                    print_diff(
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&romfile.path],
                        &[&archive_romfile.path],
                    )
                    .await?;
                }

                romfile.as_common()?.delete(progress_bar, false).await?;

                commit_transaction(transaction).await;
            }
        }
    }

    // convert RVZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            for roms in rvzs.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

                let archive_romfile = romfile
                    .as_rvz()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &romfile.as_common()?.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
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

                update_romfile(
                    &mut transaction,
                    romfile.id,
                    &archive_romfile.as_common()?.to_string(),
                    archive_romfile.as_common()?.get_size().await?
                )
                .await;

                if diff {
                    print_diff(
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&romfile.path],
                        &[&archive_romfile.path],
                    )
                    .await?;
                }

                remove_file(progress_bar, &romfile.path, false).await?;

                commit_transaction(transaction).await;
            }
        }
    }

    // convert ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

                let archive_romfile = romfile
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &romfile.as_common()?.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
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

                update_romfile(
                    &mut transaction,
                    romfile.id,
                    &archive_romfile.as_common()?.to_string(),
                    archive_romfile.as_common()?.get_size().await?,
                )
                .await;

                if diff {
                    print_diff(
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&romfile.path],
                        &[&archive_romfile.path],
                    )
                    .await?;
                }

                romfile.as_common()?.delete(progress_bar, false).await?;

                commit_transaction(transaction).await;
            }
        }
    }

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();

            let archive_romfile = romfile
                .as_archive(rom)?
                .to_archive(
                    progress_bar,
                    &tmp_directory.path(),
                    &romfile.as_common()?.path.parent().unwrap(),
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

            update_romfile(
                &mut transaction,
                romfile.id,
                &archive_romfile.as_common()?.to_string(),
                archive_romfile.as_common()?.get_size().await?,
            )
            .await;

            romfile.as_common()?.delete(progress_bar, false).await?;
        }

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
                .as_common()?
                .to_archive(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
                    &romfile.as_common()?.path.parent().unwrap(),
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

            update_romfile(
                &mut transaction,
                romfile.id,
                archive_romfile.path.as_os_str().to_str().unwrap(),
                archive_romfile.path.metadata().unwrap().len(),
            )
            .await;

            if diff {
                print_diff(
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[&romfile.path],
                    &[&archive_romfile.path],
                )
                .await?;
            }

            romfile.as_common()?.delete(progress_bar, false).await?;
        } else {
            let game = games_by_id.get(&game_id).unwrap();
            roms = roms
                .into_par_iter()
                .filter(|rom| {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    !(romfile.path.ends_with(match archive_type {
                        sevenzip::ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                        sevenzip::ArchiveType::Zip => ZIP_EXTENSION,
                    }))
                })
                .collect();
            let directory = Path::new(
                &romfiles_by_id
                    .get(&roms.first().unwrap().romfile_id.unwrap())
                    .unwrap()
                    .path,
            )
            .parent()
            .unwrap();
            let archive_name = format!(
                "{}.{}",
                &game.name,
                match archive_type {
                    sevenzip::ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                    sevenzip::ArchiveType::Zip => ZIP_EXTENSION,
                }
            );
            let archive_path = match system.arcade {
                true => directory.parent().unwrap().join(&archive_name),
                false => directory.join(&archive_name),
            };

            let romfiles = roms
                .iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect::<Vec<&Romfile>>();
            let mut archive_romfiles: Vec<ArchiveRomfile> = Vec::new();
            for romfile in &romfiles {
                let archive_romfile = romfile
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &romfile.as_common()?.path.parent().unwrap(),
                        &match system.arcade {
                            true => directory.parent().unwrap(),
                            false => directory,
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
                if results.iter().any(|result| result.is_err()) {
                    continue;
                }
            }

            let archive_romfile_id = match find_romfile_by_path(
                &mut transaction,
                archive_path.as_os_str().to_str().unwrap(),
            )
            .await
            {
                Some(romfile) => romfile.id,
                None => {
                    create_romfile(
                        &mut transaction,
                        archive_path.as_os_str().to_str().unwrap(),
                        archive_path.metadata().unwrap().len(),
                    )
                    .await
                }
            };

            if diff {
                print_diff(
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &romfiles
                        .iter()
                        .map(|romfile| &romfile.path)
                        .collect::<Vec<&String>>(),
                    &[&archive_path],
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
                        .as_common()?
                        .delete(progress_bar, false)
                        .await?;
                }
            }
        }

        commit_transaction(transaction).await;
    }

    Ok(())
}

#[cfg(feature = "chd")]
async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition CUE/BINs
    let (cue_bins, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
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
    cfg_if! {
        if #[cfg(feature = "cso")] {
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
        }
    }

    // partition ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            let (zsos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.par_iter().any(|rom| {
                        romfiles_by_id
                            .get(&rom.romfile_id.unwrap())
                            .unwrap()
                            .path
                            .ends_with(ZSO_EXTENSION)
                    })
                });
        }
    }

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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

        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .into_par_iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

        let mut cue_romfiles: Vec<CommonRomfile> = Vec::new();
        for rom in &cue_roms {
            cue_romfiles.push(
                romfile
                    .as_archive(rom)?
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            );
        }

        let mut bin_romfiles: Vec<CommonRomfile> = Vec::new();
        for rom in &bin_roms {
            bin_romfiles.push(
                romfile
                    .as_archive(rom)?
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            );
        }

        let chd_romfile = match cue_romfiles.first() {
            Some(cue_romfile) => {
                cue_romfile
                    .as_cue_bin(
                        &bin_romfiles
                            .iter()
                            .map(|bin_iso_romfile| &bin_iso_romfile.path)
                            .collect::<Vec<&PathBuf>>(),
                    )?
                    .to_chd(
                        progress_bar,
                        &romfile.as_common()?.path.parent().unwrap(),
                        &Some(cue_romfile),
                    )
                    .await?
            }
            None => {
                bin_romfiles
                    .first()
                    .unwrap()
                    .as_iso()?
                    .to_chd(
                        progress_bar,
                        &romfile.as_common()?.path.parent().unwrap(),
                        &None,
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
                    &bin_roms,
                    hash_algorithm,
                )
                .await
                .is_err()
        {
            progress_bar.println("Converted file doesn't match the original");
            chd_romfile.as_common()?.delete(progress_bar, false).await?;
            if let Some(cue_romfile) = cue_romfiles.first() {
                cue_romfile.delete(progress_bar, false).await?;
            }
            continue;
        };

        if diff {
            let mut new_paths = vec![&chd_romfile.path];
            if let Some(cue_romfile) = cue_romfiles.first() {
                new_paths.push(&cue_romfile.path)
            }
            print_diff(
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[&romfile.path],
                &new_paths,
            )
            .await?;
        }

        if let Some(cue_romfile) = cue_romfiles.first() {
            let new_cue_path = Path::new(&romfile.path)
                .parent()
                .unwrap()
                .join(cue_romfile.path.file_name().unwrap());
            let new_cue_romfile = cue_romfile
                .rename(progress_bar, &new_cue_path, false)
                .await?;
            let cue_romfile_id = create_romfile(
                &mut transaction,
                &new_cue_romfile.to_string(),
                new_cue_romfile.get_size().await?,
            )
            .await;
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

        update_romfile(
            &mut transaction,
            romfile.id,
            &chd_romfile.as_common()?.to_string(),
            chd_romfile.as_common()?.get_size().await?,
        )
        .await;
        remove_file(progress_bar, &romfile.path, false).await?;

        commit_transaction(transaction).await;
    }

    // convert CUE/BIN
    for roms in cue_bins.values() {
        let mut transaction = begin_transaction(connection).await;

        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .into_par_iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.first().unwrap().romfile_id.unwrap())
            .unwrap();
        let bin_romfiles = bin_roms
            .iter()
            .map(|bin_rom| romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap())
            .collect::<Vec<&Romfile>>();
        let chd_romfile = cue_romfile
            .as_cue_bin(
                &bin_romfiles
                    .iter()
                    .map(|romfile| &romfile.path)
                    .collect::<Vec<&String>>(),
            )?
            .to_chd(
                progress_bar,
                &cue_romfile.as_common()?.path.parent().unwrap(),
                &Some(&cue_romfile.as_common()?),
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
            let mut romfile_paths = romfiles_by_id
                .iter()
                .filter(|(&k, _)| bin_roms.iter().any(|&r| r.romfile_id.unwrap() == k))
                .map(|(_, v)| &v.path)
                .collect::<Vec<&String>>();
            romfile_paths.push(&cue_romfile.path);
            print_diff(progress_bar, &roms, &romfile_paths, &[&chd_romfile.path]).await?;
        }

        let chd_romfile_id = create_romfile(
            &mut transaction,
            &chd_romfile.as_common()?.to_string(),
            chd_romfile.as_common()?.get_size().await?,
        )
        .await;
        for bin_rom in bin_roms {
            let bin_romfile = romfiles_by_id.get(&bin_rom.romfile_id.unwrap()).unwrap();
            update_rom_romfile(&mut transaction, bin_rom.id, Some(chd_romfile_id)).await;
            delete_romfile_by_id(&mut transaction, bin_romfile.id).await;
            bin_romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let chd_romfile = romfile
                .as_iso()?
                .to_chd(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
                    &None,
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
                print_diff(progress_bar, &[rom], &[&romfile.path], &[&chd_romfile.path]).await?;
            }
            update_romfile(
                &mut transaction,
                romfile.id,
                &chd_romfile.as_common()?.to_string(),
                chd_romfile.as_common()?.get_size().await?,
            )
            .await;
            romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let chd_romfile = romfile
                        .as_xso()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(progress_bar, &romfile.as_common()?.path.parent().unwrap(), &None)
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
                        print_diff(progress_bar, &[rom], &[&romfile.path], &[&chd_romfile.path]).await?;
                    }
                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &chd_romfile.as_common()?.to_string(),
                        chd_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    // convert ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let chd_romfile = romfile
                        .as_xso()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(progress_bar, &romfile.as_common()?.path.parent().unwrap(), &None)
                        .await?;
                    if diff {
                        print_diff(progress_bar, &[rom], &[&romfile.path], &[&chd_romfile.path]).await?;
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
                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &chd_romfile.as_common()?.to_string(),
                        chd_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    remove_file(progress_bar, &romfile.path, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    Ok(())
}

#[cfg(feature = "cso")]
async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
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

    // partition CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
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
        }
    }

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(
                progress_bar,
                &romfile.as_common()?.path.parent().unwrap(),
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
            print_diff(progress_bar, &[rom], &[&romfile.path], &[&cso_romfile.path]).await?;
        }

        update_romfile(
            &mut transaction,
            romfile.id,
            cso_romfile.path.as_os_str().to_str().unwrap(),
            cso_romfile.path.metadata().unwrap().len(),
        )
        .await;

        romfile.as_common()?.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let cso_romfile = romfile
                .as_iso()?
                .to_xso(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
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
                print_diff(progress_bar, &[rom], &[&romfile.path], &[&cso_romfile.path]).await?;
            }
            update_romfile(
                &mut transaction,
                romfile.id,
                cso_romfile.path.as_os_str().to_str().unwrap(),
                cso_romfile.path.metadata().unwrap().len(),
            )
            .await;
            romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let cso_romfile = romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_xso(
                            progress_bar,
                            &Path::new(&romfile.path).parent().unwrap(),
                            &maxcso::XsoType::Cso
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
                        print_diff(progress_bar, &[rom], &[&romfile.path], &[&cso_romfile.path]).await?;
                    }
                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &cso_romfile.as_common()?.to_string(),
                        cso_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    Ok(())
}

#[cfg(feature = "nsz")]
async fn to_nsz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition NSPs
    let (nsps, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(NSP_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_nsp()?
            .to_nsz(progress_bar, &romfile.as_common()?.path.parent().unwrap())
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
            print_diff(progress_bar, &[rom], &[&romfile.path], &[&nsz_romfile.path]).await?;
        }

        update_romfile(
            &mut transaction,
            romfile.id,
            &nsz_romfile.as_common()?.to_string(),
            nsz_romfile.as_common()?.get_size().await?,
        )
        .await;

        romfile.as_common()?.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert NSPs
    for roms in nsps.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let nsz_romfile = romfile
                .as_nsp()?
                .to_nsz(progress_bar, &romfile.as_common()?.path.parent().unwrap())
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
                print_diff(progress_bar, &[rom], &[&romfile.path], &[&nsz_romfile.path]).await?;
            }
            update_romfile(
                &mut transaction,
                romfile.id,
                &nsz_romfile.as_common()?.to_string(),
                nsz_romfile.as_common()?.get_size().await?,
            )
            .await;
            romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    Ok(())
}

#[cfg(feature = "rvz")]
#[allow(clippy::too_many_arguments)]
async fn to_rvz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
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

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_rvz(
                progress_bar,
                &romfile.as_common()?.path.parent().unwrap(),
                compression_algorithm,
                compression_level,
                block_size,
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
            print_diff(progress_bar, &[rom], &[&romfile.path], &[&rvz_romfile.path]).await?;
        }

        update_romfile(
            &mut transaction,
            romfile.id,
            &rvz_romfile.as_common()?.to_string(),
            rvz_romfile.as_common()?.get_size().await?,
        )
        .await;
        romfile.as_common()?.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let rvz_romfile = romfile
                .as_iso()?
                .to_rvz(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
                    compression_algorithm,
                    compression_level,
                    block_size,
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
                print_diff(progress_bar, &[rom], &[&romfile.path], &[&rvz_romfile.path]).await?;
            }
            update_romfile(
                &mut transaction,
                romfile.id,
                &rvz_romfile.as_common()?.to_string(),
                rvz_romfile.as_common()?.get_size().await?,
            )
            .await;
            romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    Ok(())
}

#[cfg(feature = "zso")]
async fn to_zso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    diff: bool,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
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

    // partition CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
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
        }
    }

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;

        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(
                progress_bar,
                &romfile.as_common()?.path.parent().unwrap(),
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
            print_diff(progress_bar, &[rom], &[&romfile.path], &[&zso_romfile.path]).await?;
        }

        update_romfile(
            &mut transaction,
            romfile.id,
            &zso_romfile.as_common()?.to_string(),
            zso_romfile.as_common()?.get_size().await?,
        )
        .await;
        romfile.as_common()?.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;

        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let zso_romfile = romfile
                .as_iso()?
                .to_xso(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
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
                print_diff(progress_bar, &[rom], &[&romfile.path], &[&zso_romfile.path]).await?;
            }
            update_romfile(
                &mut transaction,
                romfile.id,
                &zso_romfile.as_common()?.to_string(),
                zso_romfile.as_common()?.get_size().await?,
            )
            .await;
            romfile.as_common()?.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let mut transaction = begin_transaction(connection).await;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let zso_romfile = romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_xso(
                            progress_bar,
                            &romfile.as_common()?.path.parent().unwrap(),
                            &maxcso::XsoType::Zso
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
                        print_diff(progress_bar, &[rom], &[&romfile.path], &[&zso_romfile.path]).await?;
                    }
                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &zso_romfile.as_common()?.to_string(),
                        zso_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    Ok(())
}

async fn to_original(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    check: bool,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    // partition archives
    let (archives, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile.path.ends_with(ZIP_EXTENSION) || romfile.path.ends_with(SEVENZIP_EXTENSION)
            })
        });

    // partition CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
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
        }
    }

    // partition CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
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
        }
    }

    // partition NSZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            let (nszs, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.par_iter().any(|rom| {
                        romfiles_by_id
                            .get(&rom.romfile_id.unwrap())
                            .unwrap()
                            .path
                            .ends_with(NSP_EXTENSION)
                    })
                });
        }
    }

    // partition RVZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            let (rvzs, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.par_iter().any(|rom| {
                        romfiles_by_id
                            .get(&rom.romfile_id.unwrap())
                            .unwrap()
                            .path
                            .ends_with(RVZ_EXTENSION)
                    })
                });
        }
    }

    // partition ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            let (zsos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.par_iter().any(|rom| {
                        romfiles_by_id
                            .get(&rom.romfile_id.unwrap())
                            .unwrap()
                            .path
                            .ends_with(ZSO_EXTENSION)
                    })
                });
        }
    }

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
            .par_iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();

        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }

        let romfile = romfiles.first().unwrap();

        let destination_directory = match system.arcade {
            true => {
                let romfile_path = Path::new(&romfile.path);
                let directory = romfile_path
                    .parent()
                    .unwrap()
                    .join(romfile_path.file_stem().unwrap());
                create_directory(progress_bar, &directory, false).await?;
                directory
            }
            false => Path::new(&romfile.path).parent().unwrap().to_path_buf(),
        };

        let mut common_romfiles: Vec<CommonRomfile> = Vec::new();
        for rom in roms {
            let common_romfile = romfile
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
                .collect::<Vec<(&CommonRomfile, &Rom)>>()
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
            .collect::<Vec<(&CommonRomfile, &Rom)>>()
        {
            let romfile_id = create_romfile(
                &mut transaction,
                &common_romfile.to_string(),
                common_romfile.path.metadata().unwrap().len(),
            )
            .await;
            update_rom_romfile(&mut transaction, rom.id, Some(romfile_id)).await;
        }

        delete_romfile_by_id(&mut transaction, romfile.id).await;
        romfile.as_common()?.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                if chdman::get_version().await.is_err() {
                    progress_bar.println("Please install chdman");
                    break;
                }

                let mut transaction = begin_transaction(connection).await;

                if roms.len() == 1 {
                    let romfile = romfiles_by_id.get(&roms.first().unwrap().romfile_id.unwrap()).unwrap();
                    let iso_romfile = romfile.as_chd()?.to_iso(progress_bar, &romfile.as_common()?.path.parent().unwrap()).await?;

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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &iso_romfile.as_common()?.to_string(),
                        iso_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                } else {
                    let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                        .into_par_iter()
                        .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

                    let mut romfiles: Vec<&Romfile> = bin_roms
                        .par_iter()
                        .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                        .collect();
                    romfiles.dedup();

                    if romfiles.len() > 1 {
                        bail!("Multiple CHDs found");
                    }

                    let chd_romfile = romfiles.first().unwrap();
                    let cue_romfile = romfiles_by_id.get(&cue_roms.first().unwrap().romfile_id.unwrap()).unwrap().as_common()?;

                    let cue_bin_romfile = romfiles
                        .first()
                        .unwrap()
                        .as_chd_with_cue(&cue_romfile.path)?
                        .to_cue_bin(progress_bar, &cue_romfile.path.parent().unwrap(), &cue_romfile, &bin_roms, false)
                        .await?;

                    if check {
                        let mut error = false;
                        for (bin_romfile, bin_rom) in cue_bin_romfile.bin_romfiles.iter().zip(&bin_roms).collect::<Vec<(&CommonRomfile, &&Rom)>>() {
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

                    for (bin_romfile, bin_rom) in cue_bin_romfile.bin_romfiles.iter().zip(&bin_roms).collect::<Vec<(&CommonRomfile, &&Rom)>>() {
                        let romfile_id = create_romfile(
                            &mut transaction,
                            &bin_romfile.to_string(),
                            bin_romfile.get_size().await?,
                        )
                        .await;
                        update_rom_romfile(&mut transaction, bin_rom.id, Some(romfile_id)).await;
                    }
                    delete_romfile_by_id(&mut transaction, chd_romfile.id).await;
                    chd_romfile.as_common()?.delete(progress_bar, false).await?;
                }


                commit_transaction(transaction).await;
            }
        }
    }

    // convert CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    break;
                }

                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let iso_romfile = romfile.as_xso()?.to_iso(progress_bar, &romfile.as_common()?.path.parent().unwrap()).await?;

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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &iso_romfile.as_common()?.to_string(),
                        iso_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    // convert NSZs
    cfg_if! {
        if #[cfg(feature = "nsz")] {
            for roms in nszs.values() {
                if nsz::get_version().await.is_err() {
                    progress_bar.println("Please install nsz");
                    break;
                }

                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let nsp_romfile = romfile.as_nsz()?.to_nsp(progress_bar, &romfile.as_common()?.path.parent().unwrap()).await?;

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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &nsp_romfile.as_common()?.to_string(),
                        nsp_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    // convert RVZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            for roms in rvzs.values() {
                if dolphin::get_version().await.is_err() {
                    progress_bar.println("Please install dolphin");
                    break;
                }

                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let iso_romfile = romfile.as_rvz()?.to_iso(progress_bar, &romfile.as_common()?.path.parent().unwrap()).await?;

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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        &iso_romfile.as_common()?.to_string(),
                        iso_romfile.as_common()?.get_size().await?,
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    // convert ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    break;
                }

                let mut transaction = begin_transaction(connection).await;

                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let iso_romfile = romfile.as_xso()?.to_iso(progress_bar, &romfile.as_common()?.path.parent().unwrap()).await?;

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

                    update_romfile(
                        &mut transaction,
                        romfile.id,
                        iso_romfile.path.as_os_str().to_str().unwrap(),
                        iso_romfile.path.metadata().unwrap().len(),
                    )
                    .await;
                    romfile.as_common()?.delete(progress_bar, false).await?;
                }

                commit_transaction(transaction).await;
            }
        }
    }

    Ok(())
}

async fn print_diff<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    roms: &[&Rom],
    old_files: &[&P],
    new_files: &[&Q],
) -> SimpleResult<()> {
    let original_size = roms.iter().map(|&r| r.size as u64).sum();
    let mut old_size = 0u64;
    for &old_file in old_files {
        old_size += try_with!(old_file.as_ref().metadata(), "Failed to read file metadata").len();
    }
    let mut new_size = 0u64;
    for &new_file in new_files {
        new_size += try_with!(new_file.as_ref().metadata(), "Failed to read file metadata").len();
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

#[cfg(all(test, feature = "chd"))]
mod test_chd_to_cue_bin;
#[cfg(all(test, feature = "chd"))]
mod test_chd_to_iso;
#[cfg(all(test, feature = "chd", feature = "cso"))]
mod test_cso_to_chd;
#[cfg(all(test, feature = "cso"))]
mod test_cso_to_iso;
#[cfg(all(test, feature = "cso"))]
mod test_cso_to_sevenzip_iso;
#[cfg(all(test, feature = "chd"))]
mod test_cue_bin_to_chd;
#[cfg(all(test, feature = "chd"))]
mod test_iso_to_chd;
#[cfg(all(test, feature = "cso"))]
mod test_iso_to_cso;
#[cfg(all(test, feature = "rvz"))]
mod test_iso_to_rvz;
#[cfg(all(test, feature = "zso"))]
mod test_iso_to_zso;
#[cfg(all(test, feature = "chd", feature = "cso"))]
mod test_multiple_tracks_chd_to_cso_should_do_nothing;
#[cfg(all(test, feature = "chd"))]
mod test_multiple_tracks_chd_to_sevenzip_cue_bin;
#[cfg(all(test, feature = "chd", feature = "zso"))]
mod test_multiple_tracks_chd_to_zso_should_do_nothing;
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
#[cfg(all(test, feature = "rvz"))]
mod test_rvz_to_iso;
#[cfg(all(test, feature = "rvz"))]
mod test_rvz_to_sevenzip_iso;
#[cfg(all(test, feature = "chd"))]
mod test_sevenzip_cue_bin_to_chd;
#[cfg(all(test, feature = "chd"))]
mod test_sevenzip_iso_to_chd;
#[cfg(all(test, feature = "cso"))]
mod test_sevenzip_iso_to_cso;
#[cfg(all(test, feature = "zso"))]
mod test_sevenzip_iso_to_zso;
#[cfg(test)]
mod test_sevenzip_to_original;
#[cfg(test)]
mod test_sevenzip_to_zip;
#[cfg(all(test, feature = "chd", feature = "cso"))]
mod test_single_track_chd_to_cso;
#[cfg(all(test, feature = "chd"))]
mod test_single_track_chd_to_sevenzip_iso;
#[cfg(all(test, feature = "chd", feature = "zso"))]
mod test_single_track_chd_to_zso;
#[cfg(test)]
mod test_zip_to_original;
#[cfg(test)]
mod test_zip_to_sevenzip;
#[cfg(all(test, feature = "chd", feature = "zso"))]
mod test_zso_to_chd;
#[cfg(all(test, feature = "zso"))]
mod test_zso_to_iso;
#[cfg(all(test, feature = "zso"))]
mod test_zso_to_sevenzip_iso;
