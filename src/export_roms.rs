use super::SimpleResult;
use super::bchunk;
use super::chdman;
use super::chdman::{AsChd, ChdType, ToChd, ToRdsk, ToRiff};
use super::common::*;
use super::config::*;
use super::database::*;
use super::dolphin;
use super::dolphin::{AsRvz, RvzCompressionAlgorithm, ToRvz};
use super::maxcso;
use super::maxcso::{AsXso, ToXso, XsoType};
use super::mimetype::*;
use super::model::*;
use super::nsz;
use super::nsz::{AsNsp, AsNsz, ToNsp, ToNsz};
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::{AsArchive, ToArchive};
use super::util::*;
use super::wit;
use super::wit::ToWbfs;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indexmap::map::IndexMap;
use indicatif::ProgressBar;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::drop;
use std::path::PathBuf;
use std::str::FromStr;

const ALL_FORMATS: &[&str] = &[
    "ORIGINAL", "7Z", "CHD", "CSO", "ISO", "NSZ", "RVZ", "WBFS", "ZIP", "ZSO",
];
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
            Arg::new("DIRECTORY")
                .short('d')
                .long("directory")
                .help("Set the output directory")
                .required(true)
                .num_args(1),
        )
        .arg(
            Arg::new("1G1R")
                .short('o')
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
            let mut systems: Vec<System> = vec![];
            for system_name in system_names {
                systems.append(&mut find_systems_by_name_like(connection, system_name).await);
            }
            systems.dedup_by_key(|system| system.id);
            systems
        }
        None => prompt_for_systems(connection, None, false, false, false).await?,
    };
    let format = match matches.get_one::<String>("FORMAT") {
        Some(format) => format.as_str().to_owned(),
        None => ALL_FORMATS
            .get(select(ALL_FORMATS, "Please select a format", None, None)?)
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
        "ISO" => {
            if bchunk::get_version().await.is_err() {
                progress_bar.println("Please install bchunk");
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
        "WBFS" => {
            if wit::get_version().await.is_err() {
                progress_bar.println("Please install wit");
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

        let mut games = match matches.get_many::<String>("GAME") {
            Some(game_names) => {
                let mut games: Vec<Game> = vec![];
                for game_name in game_names {
                    games.append(
                        &mut find_full_games_by_name_and_system_id(
                            connection, game_name, system.id,
                        )
                        .await,
                    );
                }
                games.dedup_by_key(|game| game.id);
                prompt_for_games(games, cfg!(test))?
            }
            None => find_full_games_by_system_id(connection, system.id).await,
        };

        if matches.get_flag("1G1R") {
            games.retain(|game| game.sorting == Sorting::OneRegion as i64);
        }

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
                let solid = get_bool(connection, "SEVENZIP_SOLID_COMPRESSION").await;
                to_archive(
                    connection,
                    progress_bar,
                    &destination_directory,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Sevenzip,
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
                    &destination_directory,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Zip,
                    &compression_level,
                    false,
                )
                .await?
            }
            "ISO" => {
                to_iso(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
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
                to_chd(
                    connection,
                    progress_bar,
                    &destination_directory,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    &cd_compression_algorithms,
                    &cd_hunk_size,
                    &dvd_compression_algorithms,
                    &dvd_hunk_size,
                )
                .await?
            }
            "CSO" => {
                to_cso(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            "NSZ" => {
                to_nsz(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
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
                let scrub = get_bool(connection, "RVZ_SCRUB").await;
                to_rvz(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
                    &compression_algorithm,
                    compression_level,
                    block_size,
                    scrub,
                )
                .await?
            }
            "WBFS" => {
                to_wbfs(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
                )
                .await?
            }
            "ZSO" => {
                to_zso(
                    connection,
                    progress_bar,
                    &destination_directory,
                    roms_by_game_id,
                    romfiles_by_id,
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
    destination_directory: &PathBuf,
    system: &System,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    archive_type: sevenzip::ArchiveType,
    compression_level: &Option<usize>,
    solid: bool,
) -> SimpleResult<()> {
    // partition CHDs
    let (chds, roms_by_game_id): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        roms_by_game_id.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
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

    // export CHDs
    for roms in chds.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let game = games_by_id.get(&bin_roms.first().unwrap().game_id).unwrap();
        let romfile = romfiles_by_id
            .get(&bin_roms.first().unwrap().romfile_id.unwrap())
            .unwrap();
        let chd_romfile = match romfile.parent_id {
            Some(parent_id) => {
                let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                    .await
                    .as_common(connection)
                    .await?
                    .as_chd()
                    .await?;
                romfile
                    .as_common(connection)
                    .await?
                    .as_chd_with_parent(parent_chd_romfile)
                    .await?
            }
            None => romfile.as_common(connection).await?.as_chd().await?,
        };
        match chd_romfile.chd_type {
            ChdType::Cd => {
                if chd_romfile.track_count > 1
                    && chdman::get_version()
                        .await?
                        .as_str()
                        .cmp(chdman::MIN_SPLITBIN_VERSION)
                        == Ordering::Less
                {
                    progress_bar.println(format!(
                    "Older chdman versions don't support splitbin, please update to {} or newer",
                    chdman::MIN_SPLITBIN_VERSION
                ));
                    continue;
                }
                let cue_rom = cue_roms.first().unwrap();
                let cue_romfile = romfiles_by_id
                    .get(&cue_rom.romfile_id.unwrap())
                    .unwrap()
                    .as_common(connection)
                    .await?;
                let cue_bin_romfile = chd_romfile
                    .to_cue_bin(
                        progress_bar,
                        &tmp_directory.path(),
                        Some(cue_romfile),
                        &bin_roms,
                        true,
                    )
                    .await?;
                cue_bin_romfile
                    .cue_romfile
                    .to_archive(
                        progress_bar,
                        &cue_bin_romfile.cue_romfile.path.parent().unwrap(),
                        destination_directory,
                        &game.name,
                        &archive_type,
                        compression_level,
                        solid,
                    )
                    .await?;
                for bin_romfile in cue_bin_romfile.bin_romfiles {
                    bin_romfile
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
            ChdType::Dvd => {
                chd_romfile
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
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
            ChdType::Hd => {
                chd_romfile
                    .to_rdsk(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
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
            ChdType::Ld => {
                chd_romfile
                    .to_riff(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
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
    }

    // export CSOs
    for roms in csos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
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

    // export NSZs
    for roms in nszs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_nsz()?
            .to_nsp(progress_bar, &tmp_directory.path())
            .await?
            .romfile
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

    // export RVZs
    for roms in rvzs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_rvz()?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
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

    // export ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
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

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        for rom in roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let archive_romfile = romfile
                .as_common(connection)
                .await?
                .as_archive(progress_bar, Some(rom))
                .await?
                .pop()
                .unwrap();
            // skip archives that are the same type
            if archive_romfile.archive_type == archive_type {
                copy_file(
                    progress_bar,
                    &archive_romfile.romfile.path,
                    &destination_directory.join(archive_romfile.romfile.path.file_name().unwrap()),
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
                .as_common(connection)
                .await?
                .to_archive(
                    progress_bar,
                    &romfile.as_common(connection).await?.path.parent().unwrap(),
                    destination_directory,
                    &game.name,
                    &archive_type,
                    compression_level,
                    solid,
                )
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
            let romfiles = roms
                .iter()
                .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
                .collect::<Vec<&Romfile>>();
            for romfile in &romfiles {
                romfile
                    .as_common(connection)
                    .await?
                    .to_archive(
                        progress_bar,
                        &romfile.as_common(connection).await?.path.parent().unwrap(),
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

#[allow(clippy::too_many_arguments)]
async fn to_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    cd_compression_algorithms: &[String],
    cd_hunk_size: &Option<usize>,
    dvd_compression_algorithms: &[String],
    dvd_hunk_size: &Option<usize>,
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
    let (chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
            })
        });

    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
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
        let parent_chd_romfile = find_parent_chd_romfile_by_game(connection, game).await;

        let (cue_roms, bin_iso_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

        let cue_romfile = match cue_roms.first() {
            Some(&cue_rom) => Some(
                romfile
                    .as_common(connection)
                    .await?
                    .as_archive(progress_bar, Some(cue_rom))
                    .await?
                    .first()
                    .unwrap()
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            ),
            None => None,
        };

        let mut bin_iso_romfiles: Vec<CommonRomfile> = vec![];
        for rom in bin_iso_roms {
            bin_iso_romfiles.push(
                romfile
                    .as_common(connection)
                    .await?
                    .as_archive(progress_bar, Some(rom))
                    .await?
                    .first()
                    .unwrap()
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?,
            );
        }

        match cue_romfile {
            Some(cue_romfile) => {
                cue_romfile
                    .as_cue_bin(bin_iso_romfiles)?
                    .to_chd(
                        progress_bar,
                        destination_directory,
                        cd_compression_algorithms,
                        cd_hunk_size,
                        match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                            None => None,
                        },
                    )
                    .await?
            }
            None => {
                bin_iso_romfiles
                    .pop()
                    .unwrap()
                    .as_iso()?
                    .to_chd(
                        progress_bar,
                        destination_directory,
                        dvd_compression_algorithms,
                        dvd_hunk_size,
                        match parent_chd_romfile.as_ref() {
                            Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                            None => None,
                        },
                    )
                    .await?
            }
        };
    }

    // export CUE/BIN
    for roms in cue_bins.values() {
        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let parent_chd_romfile = find_parent_chd_romfile_by_game(connection, game).await;
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.first().unwrap().romfile_id.unwrap())
            .unwrap()
            .as_common(connection)
            .await?;
        let mut bin_romfiles: Vec<CommonRomfile> = vec![];
        for bin_rom in &bin_roms {
            bin_romfiles.push(
                romfiles_by_id
                    .get(&bin_rom.romfile_id.unwrap())
                    .unwrap()
                    .as_common(connection)
                    .await?,
            );
        }
        cue_romfile
            .as_cue_bin(bin_romfiles)?
            .to_chd(
                progress_bar,
                destination_directory,
                cd_compression_algorithms,
                cd_hunk_size,
                match parent_chd_romfile.as_ref() {
                    Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                    None => None,
                },
            )
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile = find_parent_chd_romfile_by_game(connection, game).await;
        romfile
            .as_common(connection)
            .await?
            .as_iso()?
            .to_chd(
                progress_bar,
                destination_directory,
                dvd_compression_algorithms,
                dvd_hunk_size,
                match parent_chd_romfile.as_ref() {
                    Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                    None => None,
                },
            )
            .await?;
    }

    // export CSOs
    for roms in csos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile = find_parent_chd_romfile_by_game(connection, game).await;
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .to_chd(
                progress_bar,
                destination_directory,
                dvd_compression_algorithms,
                dvd_hunk_size,
                match parent_chd_romfile.as_ref() {
                    Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                    None => None,
                },
            )
            .await?;
    }

    // export ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile = find_parent_chd_romfile_by_game(connection, game).await;
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .to_chd(
                progress_bar,
                destination_directory,
                dvd_compression_algorithms,
                dvd_hunk_size,
                match parent_chd_romfile.as_ref() {
                    Some(romfile) => Some(romfile.as_common(connection).await.unwrap()),
                    None => None,
                },
            )
            .await?;
    }

    // export CHDs
    for roms in chds.values() {
        let bin_roms: Vec<&Rom> = roms
            .iter()
            .filter(|rom| !rom.name.ends_with(CUE_EXTENSION))
            .collect();
        let mut romfiles: Vec<&Romfile> = bin_roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();
        if romfiles.len() > 1 {
            bail!("Multiple CHDs found");
        }
        let romfile = romfiles.first().unwrap();
        copy_file(
            progress_bar,
            &romfile.as_common(connection).await?.path,
            &destination_directory.join(
                romfile
                    .as_common(connection)
                    .await?
                    .path
                    .file_name()
                    .unwrap(),
            ),
            false,
        )
        .await?;
    }

    Ok(())
}

async fn to_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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
    let (chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
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

    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, XsoType::Cso)
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, XsoType::Cso)
            .await?;
    }

    // export CHDs
    for roms in chds.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let chd_romfile = match romfile.parent_id {
            Some(parent_id) => {
                let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                    .await
                    .as_common(connection)
                    .await?
                    .as_chd()
                    .await?;
                romfile
                    .as_common(connection)
                    .await?
                    .as_chd_with_parent(parent_chd_romfile)
                    .await?
            }
            None => romfile.as_common(connection).await?.as_chd().await?,
        };
        if chd_romfile.chd_type != ChdType::Dvd {
            continue;
        }
        chd_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .to_xso(progress_bar, destination_directory, XsoType::Cso)
            .await?;
    }

    // export ZSOs
    for roms in zsos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        copy_file(
            progress_bar,
            &romfile.as_common(connection).await?.path,
            &destination_directory.join(
                romfile
                    .as_common(connection)
                    .await?
                    .path
                    .file_name()
                    .unwrap(),
            ),
            false,
        )
        .await?;
    }

    // export CSOs
    for roms in csos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        copy_file(
            progress_bar,
            &romfile.as_common(connection).await?.path,
            &destination_directory.join(
                romfile
                    .as_common(connection)
                    .await?
                    .path
                    .file_name()
                    .unwrap(),
            ),
            false,
        )
        .await?;
    }

    Ok(())
}

async fn to_nsz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // drop others
    drop(others);

    // export archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(NSP_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_nsp()?
            .to_nsz(progress_bar, destination_directory)
            .await?;
    }

    // export NSPs
    for roms in nsps.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_nsp()?
            .to_nsz(progress_bar, destination_directory)
            .await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_rvz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
    scrub: bool,
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

    // export archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_rvz(
                progress_bar,
                destination_directory,
                compression_algorithm,
                compression_level,
                block_size,
                scrub,
            )
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_iso()?
            .to_rvz(
                progress_bar,
                destination_directory,
                compression_algorithm,
                compression_level,
                block_size,
                scrub,
            )
            .await?;
    }

    // export RVZs
    for roms in rvzs.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        if scrub {
            let tmp_directory = create_tmp_directory(connection).await?;
            romfile
                .as_common(connection)
                .await?
                .as_rvz()?
                .to_iso(progress_bar, &tmp_directory.path())
                .await?
                .to_rvz(
                    progress_bar,
                    destination_directory,
                    compression_algorithm,
                    compression_level,
                    block_size,
                    scrub,
                )
                .await?;
        } else {
            copy_file(
                progress_bar,
                &romfile.as_common(connection).await?.path,
                &destination_directory.join(
                    romfile
                        .as_common(connection)
                        .await?
                        .path
                        .file_name()
                        .unwrap(),
                ),
                false,
            )
            .await?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_wbfs(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // export archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_wbfs(progress_bar, destination_directory)
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_iso()?
            .to_wbfs(progress_bar, destination_directory)
            .await?;
    }

    // export RVZs
    for roms in rvzs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_rvz()?
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .to_wbfs(progress_bar, destination_directory)
            .await?;
    }

    Ok(())
}

async fn to_zso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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
    let (chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
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

    // export archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, &tmp_directory.path())
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, XsoType::Zso)
            .await?;
    }

    // export ISOs
    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_iso()?
            .to_xso(progress_bar, destination_directory, XsoType::Zso)
            .await?;
    }

    // export CHDs
    for roms in chds.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let chd_romfile = match romfile.parent_id {
            Some(parent_id) => {
                let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                    .await
                    .as_common(connection)
                    .await?
                    .as_chd()
                    .await?;
                romfile
                    .as_common(connection)
                    .await?
                    .as_chd_with_parent(parent_chd_romfile)
                    .await?
            }
            None => romfile.as_common(connection).await?.as_chd().await?,
        };
        if chd_romfile.chd_type != ChdType::Dvd {
            continue;
        }
        chd_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .to_xso(progress_bar, destination_directory, XsoType::Zso)
            .await?;
    }

    // export ZSOs
    for roms in zsos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        copy_file(
            progress_bar,
            &romfile.as_common(connection).await?.path,
            &destination_directory.join(
                romfile
                    .as_common(connection)
                    .await?
                    .path
                    .file_name()
                    .unwrap(),
            ),
            false,
        )
        .await?;
    }

    Ok(())
}

async fn to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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

    // partition CHDs
    let (chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
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

    drop(others);

    // export archives
    for roms in archives.values() {
        let mut romfiles: Vec<&Romfile> = roms
            .iter()
            .map(|rom| romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap())
            .collect();
        romfiles.dedup();
        if romfiles.len() > 1 {
            bail!("Multiple archives found");
        }
        if roms.len() == 1 && roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            let rom = roms.first().unwrap();
            let romfile = romfiles.first().unwrap();
            romfile
                .as_common(connection)
                .await?
                .as_archive(progress_bar, Some(rom))
                .await?
                .first()
                .unwrap()
                .to_common(progress_bar, destination_directory)
                .await?;
        } else if roms.len() == 2 && roms.par_iter().any(|rom| rom.name.ends_with(CUE_EXTENSION)) {
            let (mut cue_roms, mut bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                .iter()
                .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
            let tmp_directory = create_tmp_directory(connection).await?;
            let romfile = romfiles.first().unwrap();
            let cue_rom = cue_roms.pop().unwrap();
            let cue_romfile = romfile
                .as_common(connection)
                .await?
                .as_archive(progress_bar, Some(cue_rom))
                .await?
                .first()
                .unwrap()
                .to_common(progress_bar, &tmp_directory.path())
                .await?;
            let bin_rom = bin_roms.pop().unwrap();
            let bin_romfile = romfile
                .as_common(connection)
                .await?
                .as_archive(progress_bar, Some(bin_rom))
                .await?
                .first()
                .unwrap()
                .to_common(progress_bar, &tmp_directory.path())
                .await?;
            cue_romfile
                .as_cue_bin(vec![bin_romfile])?
                .to_iso(progress_bar, destination_directory)
                .await?;
        }
    }

    // export CUE/BIN
    for roms in cue_bins.values() {
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        if bin_roms.len() > 1 {
            continue;
        }
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.first().unwrap().romfile_id.unwrap())
            .unwrap()
            .as_common(connection)
            .await?;
        let mut bin_romfiles: Vec<CommonRomfile> = vec![];
        for bin_rom in &bin_roms {
            bin_romfiles.push(
                romfiles_by_id
                    .get(&bin_rom.romfile_id.unwrap())
                    .unwrap()
                    .as_common(connection)
                    .await?,
            );
        }
        cue_romfile
            .as_cue_bin(bin_romfiles)?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    // export CHDs
    for roms in chds.values() {
        if chdman::get_version().await.is_err() {
            progress_bar.println("Please install chdman");
            break;
        }
        if roms.len() > 2 {
            continue;
        }
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
        let tmp_directory = create_tmp_directory(connection).await?;
        let romfile = romfiles.first().unwrap();
        let cue_romfile = romfiles_by_id
            .get(&cue_roms.first().unwrap().romfile_id.unwrap())
            .unwrap()
            .as_common(connection)
            .await?;
        let chd_romfile = match romfile.parent_id {
            Some(parent_id) => {
                let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                    .await
                    .as_common(connection)
                    .await?
                    .as_chd()
                    .await?;
                romfile
                    .as_common(connection)
                    .await?
                    .as_chd_with_parent(parent_chd_romfile)
                    .await?
            }
            None => romfile.as_common(connection).await?.as_chd().await?,
        };
        match chd_romfile.chd_type {
            ChdType::Cd => {
                chd_romfile
                    .to_cue_bin(
                        progress_bar,
                        &tmp_directory.path(),
                        Some(cue_romfile),
                        &bin_roms,
                        false,
                    )
                    .await?
                    .to_iso(progress_bar, destination_directory)
                    .await?;
            }
            ChdType::Dvd => {
                chd_romfile
                    .to_iso(progress_bar, destination_directory)
                    .await?;
            }
            ChdType::Hd | ChdType::Ld => continue,
        }
    }

    // export CSOs
    for roms in csos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    // export ZSOs
    for roms in zsos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    for roms in isos.values() {
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        copy_file(
            progress_bar,
            &romfile.as_common(connection).await?.path,
            &destination_directory.join(
                romfile
                    .as_common(connection)
                    .await?
                    .path
                    .file_name()
                    .unwrap(),
            ),
            false,
        )
        .await?;
    }

    Ok(())
}

async fn to_original(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    destination_directory: &PathBuf,
    system: &System,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
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
    let (chds, others): (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) =
        others.into_iter().partition(|(_, roms)| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(CHD_EXTENSION)
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

    // export archives
    for roms in archives.values() {
        if sevenzip::get_version().await.is_err() {
            progress_bar.println("Please install sevenzip");
            break;
        }
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
        for rom in &roms {
            let game = games_by_id.get(&rom.game_id).unwrap();
            if system.arcade {
                let destination_directory = destination_directory.join(&game.name);
                create_directory(progress_bar, &destination_directory, true).await?;
            }
            romfile
                .as_common(connection)
                .await?
                .as_archive(progress_bar, Some(rom))
                .await?
                .first()
                .unwrap()
                .to_common(progress_bar, &destination_directory)
                .await?;
        }
    }

    // export CHDs
    for roms in chds.values() {
        if chdman::get_version().await.is_err() {
            progress_bar.println("Please install chdman");
            break;
        }
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
        let chd_romfile = match romfile.parent_id {
            Some(parent_id) => {
                let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                    .await
                    .as_common(connection)
                    .await?
                    .as_chd()
                    .await?;
                romfile
                    .as_common(connection)
                    .await?
                    .as_chd_with_parent(parent_chd_romfile)
                    .await?
            }
            None => romfile.as_common(connection).await?.as_chd().await?,
        };
        match chd_romfile.chd_type {
            ChdType::Cd => {
                if chd_romfile.track_count > 1
                    && chdman::get_version()
                        .await?
                        .as_str()
                        .cmp(chdman::MIN_SPLITBIN_VERSION)
                        == Ordering::Less
                {
                    progress_bar.println(format!(
                    "Older chdman versions don't support splitbin, please update to {} or newer",
                    chdman::MIN_SPLITBIN_VERSION
                ));
                    continue;
                }
                let cue_romfile = match cue_roms.first() {
                    Some(cue_rom) => Some(
                        romfiles_by_id
                            .get(&cue_rom.romfile_id.unwrap())
                            .unwrap()
                            .as_common(connection)
                            .await?,
                    ),
                    None => None,
                };
                let cue_bin_romfile = chd_romfile
                    .to_cue_bin(
                        progress_bar,
                        destination_directory,
                        cue_romfile,
                        &bin_roms,
                        false,
                    )
                    .await?;
                cue_bin_romfile
                    .cue_romfile
                    .copy(
                        progress_bar,
                        &destination_directory,
                        false,
                    )
                    .await?;
            }
            ChdType::Dvd => {
                chd_romfile
                    .to_iso(progress_bar, destination_directory)
                    .await?;
            }
            ChdType::Hd => {
                chd_romfile
                    .to_rdsk(progress_bar, destination_directory)
                    .await?;
            }
            ChdType::Ld => {
                chd_romfile
                    .to_riff(progress_bar, destination_directory)
                    .await?;
            }
        }
    }

    // export CSOs
    for roms in csos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    // export NSZs
    for roms in nszs.values() {
        if nsz::get_version().await.is_err() {
            progress_bar.println("Please install nsz");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_nsz()?
            .to_nsp(progress_bar, destination_directory)
            .await?;
    }

    // export RVZs
    for roms in rvzs.values() {
        if dolphin::get_version().await.is_err() {
            progress_bar.println("Please install dolphin-tool");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_rvz()?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    // export ZSOs
    for roms in zsos.values() {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            break;
        }
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        romfile
            .as_common(connection)
            .await?
            .as_xso()
            .await?
            .to_iso(progress_bar, destination_directory)
            .await?;
    }

    // export others
    for roms in others.values() {
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            copy_file(
                progress_bar,
                &romfile.as_common(connection).await?.path,
                &destination_directory.join(
                    romfile
                        .as_common(connection)
                        .await?
                        .path
                        .file_name()
                        .unwrap(),
                ),
                false,
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod test_cso_to_chd;
#[cfg(test)]
mod test_cso_to_cso;
#[cfg(test)]
mod test_cso_to_iso;
#[cfg(test)]
mod test_cso_to_sevenzip_iso;
#[cfg(test)]
mod test_iso_chd_to_chd_should_copy;
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
mod test_iso_to_cso;
#[cfg(test)]
mod test_iso_to_rvz;
#[cfg(test)]
mod test_iso_to_wbfs;
#[cfg(test)]
mod test_iso_to_zso;
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
#[cfg(test)]
mod test_rvz_to_iso;
#[cfg(test)]
mod test_rvz_to_rvz_should_copy;
#[cfg(test)]
mod test_rvz_to_sevenzip_iso;
#[cfg(test)]
mod test_rvz_to_wbfs;
#[cfg(test)]
mod test_sevenzip_iso_to_chd;
#[cfg(test)]
mod test_sevenzip_iso_to_cso;
#[cfg(test)]
mod test_sevenzip_iso_to_zso;
#[cfg(test)]
mod test_sevenzip_multiple_tracks_cue_bin_to_chd;
#[cfg(test)]
mod test_sevenzip_single_track_cue_bin_to_iso;
#[cfg(test)]
mod test_sevenzip_to_original;
#[cfg(test)]
mod test_sevenzip_to_zip;
#[cfg(test)]
mod test_single_track_chd_to_iso;
#[cfg(test)]
mod test_single_track_cue_bin_to_iso;
#[cfg(test)]
mod test_zip_to_original;
#[cfg(test)]
mod test_zip_to_sevenzip;
#[cfg(test)]
mod test_zip_to_zip_should_copy;
#[cfg(test)]
mod test_zso_to_chd;
#[cfg(test)]
mod test_zso_to_iso;
#[cfg(test)]
mod test_zso_to_sevenzip_iso;
#[cfg(test)]
mod test_zso_to_zso_should_copy;
