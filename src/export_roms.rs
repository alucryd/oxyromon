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
use super::sevenzip::{AsArchive, ToArchive};
use super::util::*;
use super::SimpleResult;
use cfg_if::cfg_if;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::drop;
use std::path::PathBuf;
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
    Command::new("export-roms")
        .about("Export ROM files to common formats")
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
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select systems by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("DIRECTORY")
                .short('d')
                .long("directory")
                .help("Set the output directory")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("1G1R")
                .short('g')
                .long("1g1r")
                .help("Export 1G1R games only")
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
                systems.append(
                    &mut find_systems_by_name_like(connection, &format!("%{}%", system_name)).await,
                );
            }
            systems.dedup_by_key(|system| system.id);
            systems
        }
        None => prompt_for_systems(connection, None, false, false).await?,
    };
    let game_name = matches.get_one::<String>("NAME");
    let format = match matches.get_one::<String>("FORMAT") {
        Some(format) => format.as_str().to_owned(),
        None => ALL_FORMATS
            .get(select(&ALL_FORMATS, "Please select a format", None, None)?)
            .map(|&s| s.to_owned())
            .unwrap(),
    };

    let destination_directory =
        get_canonicalized_path(matches.get_one::<String>("DIRECTORY").unwrap()).await?;
    create_directory(progress_bar, &destination_directory, true).await?;

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
        "ORIGINAL" => {}
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

        let mut games = match game_name {
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

        if matches.get_flag("1G1R") {
            games.retain(|game| game.sorting == Sorting::OneRegion as i64);
        }

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
                    progress_bar,
                    &destination_directory,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            "7Z" => {
                let compression_level = get_integer(connection, "SEVENZIP_COMPRESSION_LEVEL").await;
                let solid: bool = get_bool(connection, "SEVENZIP_SOLID_COMPRESSION").await;
                to_archive(
                    connection,
                    progress_bar,
                    &destination_directory,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Sevenzip,
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
                    &destination_directory,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Zip,
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
                            &destination_directory,
                            roms_by_game_id,
                            romfiles_by_id,
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
                            &destination_directory,
                            roms_by_game_id,
                            romfiles_by_id,
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
                            &destination_directory,
                            roms_by_game_id,
                            romfiles_by_id,
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
                            &destination_directory,
                            roms_by_game_id,
                            romfiles_by_id,
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
                            &destination_directory,
                            roms_by_game_id,
                            romfiles_by_id,
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
    destination_directory: &PathBuf,
    system: &System,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    archive_type: sevenzip::ArchiveType,
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

    // export CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                if roms.len() == 1 {
                    let rom = roms.first().unwrap();
                    let game = games_by_id.get(&rom.game_id).unwrap();
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .as_common()?
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            destination_directory,
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
                        )
                        .await?;
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
                    cue_romfile
                        .as_common()?
                        .to_archive(
                            progress_bar,
                            &cue_romfile.as_common()?.path.parent().unwrap(),
                            destination_directory,
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
                        )
                        .await?;
                    let cue_bin_romfile = chd_romfile
                        .as_chd_with_cue(&cue_romfile.path)?
                        .to_cue_bin(progress_bar, &tmp_directory.path(), &cue_romfile.as_common()?, &bin_roms, true)
                        .await?;
                    for bin_romfile in cue_bin_romfile.bin_romfiles {
                        bin_romfile.to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            destination_directory,
                            &game.name,
                            &archive_type,
                            compression_level,
                            solid
                        )
                        .await?;
                    }
                }
            }
        }
    }

    // export CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
                    )
                    .await?;
            }
        }
    }

    // export NSZs
    cfg_if! {
        if #[cfg(feature = "nsz")] {
            for roms in nszs.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile
                    .as_nsz()?
                    .to_nsp(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
                    )
                    .await?;
            }
        }
    }

    // export RVZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            for roms in rvzs.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile
                    .as_rvz()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
                    )
                    .await?;
            }
        }
    }

    // export ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                let rom = roms.first().unwrap();
                let game = games_by_id.get(&rom.game_id).unwrap();
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                romfile
                    .as_xso()?
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid
                    )
                    .await?;
            }
        }
    }

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let archive_romfile = romfile.as_archive(rom)?;
            // skip archives that are the same type
            if archive_romfile.archive_type == archive_type {
                copy_file(
                    progress_bar,
                    &archive_romfile.path,
                    &destination_directory.join(archive_romfile.path.file_name().unwrap()),
                    false,
                )
                .await?;
                continue;
            }
            archive_romfile
                .to_archive(
                    progress_bar,
                    &tmp_directory.path(),
                    destination_directory,
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
                .await?;
        }
    }

    // export others
    for (game_id, mut roms) in roms_by_game_id {
        if roms.len() == 1 && !system.arcade {
            let rom = roms.first().unwrap();
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_common()?
                .to_archive(
                    progress_bar,
                    &romfile.as_common()?.path.parent().unwrap(),
                    destination_directory,
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
                .await?;
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
            let romfiles = roms
                .iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect::<Vec<&Romfile>>();
            for romfile in &romfiles {
                romfile
                    .as_common()?
                    .to_archive(
                        progress_bar,
                        &romfile.as_common()?.path.parent().unwrap(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid,
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "chd")]
async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        match cue_romfiles.first() {
            Some(cue_romfile) => {
                cue_romfile
                    .as_cue_bin(
                        &bin_romfiles
                            .iter()
                            .map(|bin_iso_romfile| &bin_iso_romfile.path)
                            .collect::<Vec<&PathBuf>>(),
                    )?
                    .to_chd(progress_bar, destination_directory, &Some(cue_romfile))
                    .await?
            }
            None => {
                bin_romfiles
                    .first()
                    .unwrap()
                    .as_iso()?
                    .to_chd(progress_bar, destination_directory, &None)
                    .await?
            }
        };
    }

    // export CUE/BIN
    for roms in cue_bins.values() {
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
        cue_romfile
            .as_cue_bin(
                &bin_romfiles
                    .iter()
                    .map(|romfile| &romfile.path)
                    .collect::<Vec<&String>>(),
            )?
            .to_chd(
                progress_bar,
                destination_directory,
                &Some(&cue_romfile.as_common()?),
            )
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_iso()?
                .to_chd(progress_bar, destination_directory, &None)
                .await?;
        }
    }

    // export CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile
                        .as_xso()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(progress_bar, destination_directory, &None)
                        .await?;
                }
            }
        }
    }

    // export ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile
                        .as_xso()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(progress_bar, destination_directory, &None)
                        .await?;
                }
            }
        }
    }

    // export CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    copy_file(
                        progress_bar,
                        &romfile.path,
                        &destination_directory.join(&romfile.as_common()?.path.file_name().unwrap()),
                        false
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "cso")]
async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // partition CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            let (csos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.len() == 1
                        && roms.par_iter().any(|rom| {
                            romfiles_by_id
                                .get(&rom.romfile_id.unwrap())
                                .unwrap()
                                .path
                                .ends_with(CSO_EXTENSION)
                        })
                });
        }
    }
    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        romfile
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, &maxcso::XsoType::Cso)
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_iso()?
                .to_xso(progress_bar, destination_directory, &maxcso::XsoType::Cso)
                .await?;
        }
    }

    // export CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_xso(
                            progress_bar,
                            destination_directory,
                            &maxcso::XsoType::Cso
                        )
                        .await?;
                }
            }
        }
    }

    // export CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    copy_file(
                        progress_bar,
                        &romfile.path,
                        &destination_directory.join(&romfile.as_common()?.path.file_name().unwrap()),
                        false
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(feature = "nsz")]
async fn to_nsz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        romfile
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_nsp()?
            .to_nsz(progress_bar, destination_directory)
            .await?;
    }

    // export NSPs
    for roms in nsps.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_nsp()?
                .to_nsz(progress_bar, destination_directory)
                .await?;
        }
    }

    Ok(())
}

#[cfg(feature = "rvz")]
#[allow(clippy::too_many_arguments)]
async fn to_rvz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // partition RVZs
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

    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        romfile
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_rvz(
                progress_bar,
                destination_directory,
                compression_algorithm,
                compression_level,
                block_size,
            )
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_iso()?
                .to_rvz(
                    progress_bar,
                    destination_directory,
                    compression_algorithm,
                    compression_level,
                    block_size,
                )
                .await?;
        }
    }

    // export RVZs
    for roms in rvzs.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            copy_file(
                progress_bar,
                &romfile.path,
                &destination_directory.join(romfile.as_common()?.path.file_name().unwrap()),
                false,
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(feature = "zso")]
async fn to_zso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // partition ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            let (zsos, others): (HashMap<i64, Vec<Rom>>, HashMap<i64, Vec<Rom>>) =
                others.into_iter().partition(|(_, roms)| {
                    roms.len() == 1
                        && roms.par_iter().any(|rom| {
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

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        romfile
            .as_archive(rom)?
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, &maxcso::XsoType::Zso)
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            romfile
                .as_iso()?
                .to_xso(progress_bar, destination_directory, &maxcso::XsoType::Zso)
                .await?;
        }
    }

    // export CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                let tmp_directory = create_tmp_directory(connection).await?;
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile
                        .as_chd()?
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_xso(
                            progress_bar,
                            destination_directory,
                            &maxcso::XsoType::Zso
                        )
                        .await?;
                }
            }
        }
    }

    // export ZSOs
    for roms in zsos.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            copy_file(
                progress_bar,
                &romfile.path,
                &destination_directory.join(romfile.as_common()?.path.file_name().unwrap()),
                false,
            )
            .await?;
        }
    }

    Ok(())
}

async fn to_original(
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    system: &System,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: HashMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // export archives
    for roms in archives.values() {
        if sevenzip::get_version().await.is_err() {
            progress_bar.println("Please install sevenzip");
            break;
        }
        let mut romfiles: Vec<&Romfile> = roms
            .par_iter()
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
            .filter(|rom| rom.romfile_id.unwrap() == rom.id)
            .collect();
        for rom in &roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            if system.arcade {
                let destination_directory = destination_directory.join(&game.name);
                create_directory(progress_bar, &destination_directory, true).await?;
            }
            romfile
                .as_archive(rom)?
                .to_common(progress_bar, &destination_directory)
                .await?;
        }
    }

    // export CHDs
    cfg_if! {
        if #[cfg(feature = "chd")] {
            for roms in chds.values() {
                if chdman::get_version().await.is_err() {
                    progress_bar.println("Please install chdman");
                    break;
                }
                if roms.len() == 1 {
                    let romfile = romfiles_by_id.get(&roms.first().unwrap().romfile_id.unwrap()).unwrap();
                    romfile.as_chd()?.to_iso(progress_bar, destination_directory).await?;
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
                    let cue_romfile = romfiles_by_id.get(&cue_roms.first().unwrap().romfile_id.unwrap()).unwrap().as_common()?;
                    romfiles
                        .first()
                        .unwrap()
                        .as_chd_with_cue(&cue_romfile.path)?
                        .to_cue_bin(progress_bar, destination_directory, &cue_romfile, &bin_roms, false)
                        .await?;
                    copy_file(
                        progress_bar,
                        &cue_romfile.path,
                        &destination_directory.join(&cue_romfile.path.file_name().unwrap()),
                        false
                    )
                    .await?;
                }
            }
        }
    }

    // export CSOs
    cfg_if! {
        if #[cfg(feature = "cso")] {
            for roms in csos.values() {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    break;
                }
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile.as_xso()?.to_iso(progress_bar, destination_directory).await?;
                }
            }
        }
    }

    // export NSZs
    cfg_if! {
        if #[cfg(feature = "nsz")] {
            for roms in nszs.values() {
                if nsz::get_version().await.is_err() {
                    progress_bar.println("Please install nsz");
                    break;
                }
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile.as_nsz()?.to_nsp(progress_bar, destination_directory).await?;
                }
            }
        }
    }

    // cexport RVZs
    cfg_if! {
        if #[cfg(feature = "rvz")] {
            for roms in rvzs.values() {
                if dolphin::get_version().await.is_err() {
                    progress_bar.println("Please install dolphin");
                    break;
                }
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile.as_rvz()?.to_iso(progress_bar, destination_directory).await?;
                }
            }
        }
    }

    // export ZSOs
    cfg_if! {
        if #[cfg(feature = "zso")] {
            for roms in zsos.values() {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    break;
                }
                for rom in roms {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    romfile.as_xso()?.to_iso(progress_bar, destination_directory).await?;
                }
            }
        }
    }

    for roms in others.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            copy_file(
                progress_bar,
                &romfile.path,
                &destination_directory.join(romfile.as_common()?.path.file_name().unwrap()),
                false,
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "chd"))]
mod test_chd_to_chd_should_copy;
#[cfg(all(test, feature = "chd"))]
mod test_chd_to_cue_bin;
#[cfg(all(test, feature = "chd"))]
mod test_chd_to_iso;
#[cfg(all(test, feature = "chd", feature = "cso"))]
mod test_cso_to_chd;
#[cfg(all(test, feature = "cso"))]
mod test_cso_to_cso;
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
mod test_original_to_original_should_copy;
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
mod test_rvz_to_rvz_should_copy;
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
#[cfg(test)]
mod test_zip_to_zip_should_copy;
#[cfg(all(test, feature = "chd", feature = "zso"))]
mod test_zso_to_chd;
#[cfg(all(test, feature = "zso"))]
mod test_zso_to_iso;
#[cfg(all(test, feature = "zso"))]
mod test_zso_to_sevenzip_iso;
#[cfg(all(test, feature = "zso"))]
mod test_zso_to_zso_should_copy;
