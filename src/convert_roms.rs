use super::chdman;
use super::chdman::{AsRdsk, AsRiff, ChdType, ToChd, ToRdsk, ToRiff};
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
use super::progress::*;
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::{ArchiveCompression, ArchiveFile, ArchiveRomfile, AsArchive, ToArchive};
use super::transcode::*;
use super::util::*;
use anyhow::{Result, bail};
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indexmap::map::IndexMap;
use indicatif::{HumanBytes, ProgressBar};
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::mem::drop;
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
) -> Result<()> {
    let systems = match matches.get_many::<String>("SYSTEM") {
        Some(system_names) => {
            let mut systems: Vec<System> = vec![];
            for system_name in system_names {
                systems.append(&mut find_systems_by_name_like(connection, system_name).await);
            }
            systems.dedup_by_key(|system| system.id);
            systems
        }
        None => prompt_for_systems(connection, None, false, false, matches.get_flag("ALL")).await?,
    };
    let format = match matches.get_one::<String>("FORMAT") {
        Some(format) => format.as_str().to_owned(),
        None => ALL_FORMATS
            .get(select(ALL_FORMATS, "Please select a format", None, None)?)
            .map(|&s| s.to_owned())
            .unwrap(),
    };
    let recompress = matches.get_flag("RECOMPRESS");
    let diff = matches.get_flag("DIFF");
    let check = matches.get_flag("CHECK");

    match format.as_str() {
        "7Z" | "ZIP" => {
            if sevenzip::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: sevenzip");
                return Ok(());
            }
        }
        "CHD" => {
            if chdman::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: chdman");
                return Ok(());
            }
        }
        "CSO" => {
            if maxcso::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: maxcso");
                return Ok(());
            }
        }
        "NSZ" => {
            if nsz::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: nsz");
                return Ok(());
            }
        }
        "RVZ" => {
            if dolphin::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: dolphin-tool");
                return Ok(());
            }
        }
        "ZSO" => {
            if maxcso::get_version().await.is_err() {
                print_error(progress_bar, "Required tool not found: maxcso");
                return Ok(());
            }
        }
        "ORIGINAL" => {}
        _ => bail!("Not supported"),
    }

    for system in systems {
        print_header(progress_bar, &format!("Processing \"{}\"", system.name));

        if format == "CHD"
            && system.name.contains("Dreamcast")
            && chdman::get_version()
                .await?
                .as_str()
                .cmp(chdman::MIN_DREAMCAST_VERSION)
                == Ordering::Less
        {
            print_warning(
                progress_bar,
                &format!(
                    "chdman {} or newer required for Dreamcast games",
                    chdman::MIN_DREAMCAST_VERSION
                ),
            );
            continue;
        }

        if system.arcade && !ARCADE_FORMATS.contains(&format.as_str()) {
            print_warning(
                progress_bar,
                &format!("Only {:?} are supported for arcade systems", ARCADE_FORMATS),
            );
            continue;
        }

        let games = match matches.get_many::<String>("GAME") {
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

        if games.is_empty() {
            if matches.index_of("GAME").is_some() {
                print_warning(progress_bar, "No matching games found");
            }
            continue;
        }

        let roms = find_original_roms_with_romfile_by_game_ids(
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
                )
                .await?
            }
            "7Z" => {
                let compression = sevenzip::get_archive_compression(
                    connection,
                    &sevenzip::ArchiveType::Sevenzip,
                    Some(system.id),
                )
                .await;
                let solid =
                    get_bool(connection, "SEVENZIP_SOLID_COMPRESSION", Some(system.id)).await;
                to_archive(
                    connection,
                    progress_bar,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Sevenzip,
                    recompress,
                    diff,
                    check,
                    &compression,
                    solid,
                )
                .await?
            }
            "ZIP" => {
                let compression = sevenzip::get_archive_compression(
                    connection,
                    &sevenzip::ArchiveType::Zip,
                    Some(system.id),
                )
                .await;
                to_archive(
                    connection,
                    progress_bar,
                    &system,
                    games_by_id,
                    roms_by_game_id,
                    romfiles_by_id,
                    sevenzip::ArchiveType::Zip,
                    recompress,
                    diff,
                    check,
                    &compression,
                    false,
                )
                .await?
            }
            "CHD" => {
                let cd_compression_algorithms =
                    get_list(connection, "CHD_CD_COMPRESSION_ALGORITHMS", Some(system.id)).await;
                let cd_hunk_size =
                    get_integer(connection, "CHD_CD_HUNK_SIZE", Some(system.id)).await;
                let dvd_compression_algorithms = get_list(
                    connection,
                    "CHD_DVD_COMPRESSION_ALGORITHMS",
                    Some(system.id),
                )
                .await;
                let dvd_hunk_size =
                    get_integer(connection, "CHD_DVD_HUNK_SIZE", Some(system.id)).await;
                let hd_compression_algorithms =
                    get_list(connection, "CHD_HD_COMPRESSION_ALGORITHMS", Some(system.id)).await;
                let hd_hunk_size =
                    get_integer(connection, "CHD_HD_HUNK_SIZE", Some(system.id)).await;
                let ld_compression_algorithms =
                    get_list(connection, "CHD_LD_COMPRESSION_ALGORITHMS", Some(system.id)).await;
                let ld_hunk_size =
                    get_integer(connection, "CHD_LD_HUNK_SIZE", Some(system.id)).await;
                let parents = get_bool(connection, "CHD_PARENTS", Some(system.id)).await;
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
                    &cd_compression_algorithms,
                    &cd_hunk_size,
                    &dvd_compression_algorithms,
                    &dvd_hunk_size,
                    &hd_compression_algorithms,
                    &hd_hunk_size,
                    &ld_compression_algorithms,
                    &ld_hunk_size,
                    parents,
                    prompt_for_parents,
                )
                .await?
            }
            "CSO" => {
                to_xso(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    XsoType::Cso,
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
                )
                .await?
            }
            "RVZ" => {
                let compression_algorithm = RvzCompressionAlgorithm::from_str(
                    &get_string(connection, "RVZ_COMPRESSION_ALGORITHM", Some(system.id))
                        .await
                        .unwrap(),
                )
                .unwrap();
                let compression_level =
                    get_integer(connection, "RVZ_COMPRESSION_LEVEL", Some(system.id))
                        .await
                        .unwrap();
                let block_size = get_integer(connection, "RVZ_BLOCK_SIZE", Some(system.id))
                    .await
                    .unwrap();
                to_rvz(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    &compression_algorithm,
                    compression_level,
                    block_size,
                )
                .await?
            }
            "ZSO" => {
                to_xso(
                    connection,
                    progress_bar,
                    roms_by_game_id,
                    romfiles_by_id,
                    recompress,
                    diff,
                    check,
                    XsoType::Zso,
                )
                .await?
            }
            _ => bail!("Not supported"),
        }

        print_separator(progress_bar);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn to_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    games_by_id: HashMap<i64, Game>,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    archive_type: sevenzip::ArchiveType,
    recompress: bool,
    diff: bool,
    check: bool,
    compression: &ArchiveCompression,
    solid: bool,
) -> Result<()> {
    // partition CHDs
    let (mut chds, roms_by_game_id) =
        partition_games_by_extensions(roms_by_game_id, &romfiles_by_id, &[CHD_EXTENSION]);
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
    let (csos, roms_by_game_id) =
        partition_games_by_extensions(roms_by_game_id, &romfiles_by_id, &[CSO_EXTENSION]);

    // partition NSZs
    let (nszs, roms_by_game_id) =
        partition_games_by_extensions(roms_by_game_id, &romfiles_by_id, &[NSZ_EXTENSION]);

    // partition RVZs
    let (rvzs, roms_by_game_id) =
        partition_games_by_extensions(roms_by_game_id, &romfiles_by_id, &[RVZ_EXTENSION]);

    // partition ZSOs
    let (zsos, roms_by_game_id) =
        partition_games_by_extensions(roms_by_game_id, &romfiles_by_id, &[ZSO_EXTENSION]);

    // partition archives
    let (archives, roms_by_game_id) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[SEVENZIP_EXTENSION, ZIP_EXTENSION],
    );

    // convert CHDs
    for roms in chds.values() {
        // leave arcade CHDs untouched
        if roms.iter().any(|rom| rom.disk) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let game = games_by_id.get(&bin_roms.first().unwrap().game_id).unwrap();
        let romfile = romfiles_by_id
            .get(&bin_roms.first().unwrap().romfile_id.unwrap())
            .unwrap();
        if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
            .await
            .is_empty()
        {
            print_skip(progress_bar, "CHD has children, skipping");
            continue;
        }
        let chd_romfile = romfile_as_chd(&mut transaction, romfile).await?;
        match chd_romfile.chd_type {
            ChdType::Cd => {
                if chd_romfile.track_count > 1
                    && chdman::get_version()
                        .await?
                        .as_str()
                        .cmp(chdman::MIN_SPLITBIN_VERSION)
                        == Ordering::Less
                {
                    print_warning(
                        progress_bar,
                        &format!(
                            "chdman {} or newer required for splitbin support",
                            chdman::MIN_SPLITBIN_VERSION
                        ),
                    );
                    continue;
                }
                let cue_rom = cue_roms.first().unwrap();
                let cue_romfile = romfiles_by_id
                    .get(&cue_rom.romfile_id.unwrap())
                    .unwrap()
                    .as_common(&mut transaction)
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

                let mut archive_romfiles: Vec<ArchiveRomfile> = vec![];
                let archive_romfile = cue_bin_romfile
                    .cue_romfile
                    .to_archive(
                        progress_bar,
                        &cue_bin_romfile.cue_romfile.path.parent().unwrap(),
                        &chd_romfile.romfile.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression,
                        solid,
                    )
                    .await?;
                archive_romfiles.push(archive_romfile);

                for bin_romfile in cue_bin_romfile.bin_romfiles {
                    let archive_romfile = bin_romfile
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &chd_romfile.romfile.path.parent().unwrap(),
                            &game.name,
                            &archive_type,
                            compression,
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
                            .check(&mut transaction, progress_bar, &None, &[rom])
                            .await
                            .is_err()
                        {
                            error = true;
                            break;
                        }
                    }
                    if error {
                        print_error(progress_bar, "Integrity check failed, reverting conversion");
                        archive_romfiles
                            .first()
                            .unwrap()
                            .romfile
                            .delete(progress_bar, false)
                            .await?;
                        continue;
                    }
                }

                archive_romfiles
                    .first()
                    .unwrap()
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                update_romfile_parent(&mut transaction, romfile.id, None).await;
                update_rom_romfile(&mut transaction, cue_rom.id, Some(romfile.id)).await;
                delete_romfile_by_id(&mut transaction, cue_rom.romfile_id.unwrap()).await;

                if diff {
                    print_diff(
                        &mut transaction,
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&cue_bin_romfile.cue_romfile, &chd_romfile.romfile],
                        &[&archive_romfiles.first().unwrap().romfile],
                    )
                    .await?;
                }

                cue_bin_romfile
                    .cue_romfile
                    .delete(progress_bar, false)
                    .await?;
                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Dvd => {
                let archive_romfile = chd_romfile
                    .to_iso(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &chd_romfile.romfile.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression,
                        solid,
                    )
                    .await?;

                if check
                    && archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[bin_roms.first().unwrap()],
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    archive_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                archive_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                update_romfile_parent(&mut transaction, romfile.id, None).await;

                if diff {
                    print_diff(
                        &mut transaction,
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&chd_romfile.romfile],
                        &[&archive_romfile.romfile],
                    )
                    .await?;
                }

                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Hd => {
                let archive_romfile = chd_romfile
                    .to_rdsk(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &chd_romfile.romfile.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression,
                        solid,
                    )
                    .await?;

                if check
                    && archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[bin_roms.first().unwrap()],
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    archive_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                archive_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                update_romfile_parent(&mut transaction, romfile.id, None).await;

                if diff {
                    print_diff(
                        &mut transaction,
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&chd_romfile.romfile],
                        &[&archive_romfile.romfile],
                    )
                    .await?;
                }

                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Ld => {
                let archive_romfile = chd_romfile
                    .to_riff(progress_bar, &tmp_directory.path())
                    .await?
                    .romfile
                    .to_archive(
                        progress_bar,
                        &tmp_directory.path(),
                        &chd_romfile.romfile.path.parent().unwrap(),
                        &game.name,
                        &archive_type,
                        compression,
                        solid,
                    )
                    .await?;

                if check
                    && archive_romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &[bin_roms.first().unwrap()],
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    archive_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                archive_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                update_romfile_parent(&mut transaction, romfile.id, None).await;

                if diff {
                    print_diff(
                        &mut transaction,
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&chd_romfile.romfile],
                        &[&archive_romfile.romfile],
                    )
                    .await?;
                }

                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
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
        let cso_romfile = romfile.as_common(&mut transaction).await?.as_xso().await?;
        let archive_romfile = cso_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &cso_romfile.romfile.path.parent().unwrap(),
                &game.name,
                &archive_type,
                compression,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(&mut transaction, progress_bar, &None, &[rom])
                .await
                .is_err()
        {
            print_error(progress_bar, "Integrity check failed, reverting conversion");
            archive_romfile.romfile.delete(progress_bar, false).await?;
            continue;
        };

        archive_romfile
            .romfile
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[&cso_romfile.romfile],
                &[&archive_romfile.romfile],
            )
            .await?;
        }

        cso_romfile.romfile.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert NSZs
    for roms in nszs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let nsz_romfile = romfile.as_common(&mut transaction).await?.as_nsz()?;
        let archive_romfile = nsz_romfile
            .to_nsp(progress_bar, &tmp_directory.path())
            .await?
            .romfile
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &nsz_romfile.romfile.path.parent().unwrap(),
                &game.name,
                &archive_type,
                compression,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(&mut transaction, progress_bar, &None, &[rom])
                .await
                .is_err()
        {
            print_error(progress_bar, "Integrity check failed, reverting conversion");
            archive_romfile.romfile.delete(progress_bar, false).await?;
            continue;
        };

        archive_romfile
            .romfile
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[&nsz_romfile.romfile],
                &[&archive_romfile.romfile],
            )
            .await?;
        }

        nsz_romfile.romfile.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert RVZs
    for roms in rvzs.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let rvz_romfile = romfile.as_common(&mut transaction).await?.as_rvz()?;
        let archive_romfile = rvz_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &rvz_romfile.romfile.path.parent().unwrap(),
                &game.name,
                &archive_type,
                compression,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(&mut transaction, progress_bar, &None, &[rom])
                .await
                .is_err()
        {
            print_error(progress_bar, "Integrity check failed, reverting conversion");
            archive_romfile.romfile.delete(progress_bar, false).await?;
            continue;
        };

        archive_romfile
            .romfile
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[&rvz_romfile.romfile],
                &[&archive_romfile.romfile],
            )
            .await?;
        }

        rvz_romfile.romfile.delete(progress_bar, false).await?;

        commit_transaction(transaction).await;
    }

    // convert ZSOs
    for roms in zsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let zso_romfile = romfile.as_common(&mut transaction).await?.as_xso().await?;
        let archive_romfile = zso_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?
            .romfile
            .to_archive(
                progress_bar,
                &tmp_directory.path(),
                &zso_romfile.romfile.path.parent().unwrap(),
                &game.name,
                &archive_type,
                compression,
                solid,
            )
            .await?;

        if check
            && archive_romfile
                .check(&mut transaction, progress_bar, &None, &[rom])
                .await
                .is_err()
        {
            print_error(progress_bar, "Integrity check failed, reverting conversion");
            archive_romfile.romfile.delete(progress_bar, false).await?;
            continue;
        };

        archive_romfile
            .romfile
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;

        if diff {
            print_diff(
                &mut transaction,
                progress_bar,
                &roms.iter().collect::<Vec<&Rom>>(),
                &[&zso_romfile.romfile],
                &[&archive_romfile.romfile],
            )
            .await?;
        }

        zso_romfile.romfile.delete(progress_bar, false).await?;

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
        let archive_romfiles = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(progress_bar, None)
            .await?;
        let source_archive_type = archive_romfiles.first().unwrap().archive_type;
        let mut archive_romfiles_roms: Vec<(ArchiveRomfile, &Rom)> = vec![];
        for rom in roms {
            let archive_romfile = archive_romfiles
                .iter()
                .find(|archive_romfile| archive_romfile.path == rom.name)
                .unwrap();
            if source_archive_type != archive_type {
                archive_romfiles_roms.push((
                    archive_romfile
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &archive_romfile.romfile.path.parent().unwrap(),
                            &game.name,
                            &archive_type,
                            compression,
                            solid,
                        )
                        .await?,
                    rom,
                ));
            } else if recompress {
                archive_romfiles_roms.push((
                    archive_romfile
                        .to_archive(
                            progress_bar,
                            &tmp_directory.path(),
                            &tmp_directory.path(),
                            &game.name,
                            &archive_type,
                            compression,
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
                        .check(&mut transaction, progress_bar, &None, &[rom])
                        .await
                        .is_err()
                    {
                        error = true;
                        break;
                    };
                }
                if error {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    if source_archive_type != archive_type {
                        archive_romfile_rom
                            .0
                            .romfile
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
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
            } else {
                romfile
                    .as_common(&mut transaction)
                    .await?
                    .delete(progress_bar, false)
                    .await?;
                archive_romfile_rom
                    .0
                    .romfile
                    .rename(
                        progress_bar,
                        &romfile.as_common(&mut transaction).await?.path,
                        false,
                    )
                    .await?
                    .update(&mut transaction, progress_bar, romfile.id)
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
            let common_romfile = romfile.as_common(&mut transaction).await?;
            let archive_romfile = common_romfile
                .to_archive(
                    progress_bar,
                    &common_romfile.path.parent().unwrap(),
                    &common_romfile.path.parent().unwrap(),
                    &game.name,
                    &archive_type,
                    compression,
                    solid,
                )
                .await?;

            if check
                && archive_romfile
                    .check(&mut transaction, progress_bar, &None, &[rom])
                    .await
                    .is_err()
            {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
                archive_romfile.romfile.delete(progress_bar, false).await?;
                continue;
            };

            archive_romfile
                .romfile
                .update(&mut transaction, progress_bar, romfile.id)
                .await?;

            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[&common_romfile],
                    &[&archive_romfile.romfile],
                )
                .await?;
            }

            common_romfile.delete(progress_bar, false).await?;
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
            let mut archive_romfiles: Vec<ArchiveRomfile> = vec![];
            for romfile in &romfiles {
                let common_romfile = romfile.as_common(&mut transaction).await?;
                let archive_romfile = common_romfile
                    .to_archive(
                        progress_bar,
                        &common_romfile.path.parent().unwrap(),
                        &match system.arcade {
                            true => directory.parent().unwrap(),
                            false => directory.as_path(),
                        },
                        &game.name,
                        &archive_type,
                        compression,
                        solid,
                    )
                    .await?;
                archive_romfiles.push(archive_romfile);
            }

            if check {
                let mut results: Vec<Result<()>> = vec![];
                for (archive_romfile, rom) in archive_romfiles.iter().zip(&roms) {
                    let result = archive_romfile
                        .check(&mut transaction, progress_bar, &None, &[rom])
                        .await;
                    if result.is_err() {
                        print_error(progress_bar, "Integrity check failed, reverting conversion");
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
                &archive_romfiles.first().unwrap().romfile.to_string(),
            )
            .await
            {
                Some(romfile) => romfile.id,
                None => {
                    archive_romfiles
                        .first()
                        .unwrap()
                        .romfile
                        .create(&mut transaction, progress_bar, RomfileType::Romfile)
                        .await?
                }
            };

            if diff {
                let mut common_romfiles: Vec<CommonRomfile> = vec![];
                for romfile in romfiles {
                    common_romfiles.push(romfile.as_common(&mut transaction).await?)
                }
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &common_romfiles.iter().collect::<Vec<&CommonRomfile>>(),
                    &[&archive_romfiles.first().unwrap().romfile],
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
    cd_compression_algorithms: &[String],
    cd_hunk_size: &Option<usize>,
    dvd_compression_algorithms: &[String],
    dvd_hunk_size: &Option<usize>,
    hd_compression_algorithms: &[String],
    hd_hunk_size: &Option<usize>,
    ld_compression_algorithms: &[String],
    ld_hunk_size: &Option<usize>,
    parents: bool,
    prompt_for_parents: bool,
) -> Result<()> {
    // partition archives
    let (archives, others) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[ZIP_EXTENSION, SEVENZIP_EXTENSION],
    );

    // partition CUE/BINs
    let (cue_bins, others) =
        partition_games_by_all_extensions(others, &romfiles_by_id, &[CUE_EXTENSION, BIN_EXTENSION]);

    // partition ISOs
    let (isos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[ISO_EXTENSION]);

    // partition CSOs
    let (csos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[CSO_EXTENSION]);

    // partition ZSOs
    let (zsos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[ZSO_EXTENSION]);

    // partition CHDs
    let (mut chds, others) =
        partition_games_by_extensions(others, &romfiles_by_id, &[CHD_EXTENSION]);
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
        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let parent_chd_romfile =
            find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;

        if roms.len() == 1 {
            let rom = roms.first().unwrap();
            let archive_romfile = romfile
                .as_common(&mut transaction)
                .await?
                .as_archive(progress_bar, Some(rom))
                .await?
                .pop()
                .unwrap();
            let common_romfile = archive_romfile
                .to_common(progress_bar, &tmp_directory.path())
                .await?;
            let mut extension = common_romfile
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase();
            if extension != ISO_EXTENSION
                && let Some(mimetype) = get_mimetype(&common_romfile.path).await?
            {
                extension = mimetype.extension().to_string();
            }
            let chd_romfile = match extension.as_str() {
                ISO_EXTENSION => {
                    common_romfile
                        .as_iso()?
                        .to_chd(
                            progress_bar,
                            &archive_romfile.romfile.path.parent().unwrap(),
                            cd_compression_algorithms,
                            cd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                RDSK_EXTENSION => {
                    common_romfile
                        .as_rdsk()
                        .await?
                        .to_chd(
                            progress_bar,
                            &archive_romfile.romfile.path.parent().unwrap(),
                            hd_compression_algorithms,
                            hd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                RIFF_EXTENSION => {
                    common_romfile
                        .as_riff()
                        .await?
                        .to_chd(
                            progress_bar,
                            &archive_romfile.romfile.path.parent().unwrap(),
                            ld_compression_algorithms,
                            ld_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                _ => {
                    print_skip(progress_bar, "Unsupported file type, skipping");
                    continue;
                }
            };
            if check
                && chd_romfile
                    .check(&mut transaction, progress_bar, &None, &[rom])
                    .await
                    .is_err()
            {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
                chd_romfile.romfile.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                let common_romfile = romfile.as_common(&mut transaction).await?;
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[&common_romfile],
                    &[&chd_romfile.romfile],
                )
                .await?;
            }
            chd_romfile
                .romfile
                .update(&mut transaction, progress_bar, romfile.id)
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
        } else {
            let (mut cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
                .iter()
                .partition(|rom| rom.name.ends_with(CUE_EXTENSION));

            if cue_roms.is_empty() {
                print_skip(progress_bar, "No CUE file, skipping");
                continue;
            }

            let cue_rom = cue_roms.pop().unwrap();
            let decoded = archive_to_cue_bin(
                &mut transaction,
                progress_bar,
                cue_rom,
                &bin_roms,
                romfile,
                &tmp_directory,
            )
            .await?;
            let cue_bin_romfile = decoded.inner;
            let chd_romfile = cue_bin_romfile
                .to_chd(
                    progress_bar,
                    &decoded.source.path.parent().unwrap(),
                    cd_compression_algorithms,
                    cd_hunk_size,
                    parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                )
                .await?;
            if check
                && chd_romfile
                    .check(&mut transaction, progress_bar, &None, &bin_roms)
                    .await
                    .is_err()
            {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
                chd_romfile.romfile.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    &[&decoded.source],
                    &[&cue_bin_romfile.cue_romfile, &chd_romfile.romfile],
                )
                .await?;
            }
            let cue_romfile_id = cue_bin_romfile
                .cue_romfile
                .rename(
                    progress_bar,
                    &decoded
                        .source
                        .path
                        .parent()
                        .unwrap()
                        .join(cue_bin_romfile.cue_romfile.path.file_name().unwrap()),
                    false,
                )
                .await?
                .create(&mut transaction, progress_bar, RomfileType::Romfile)
                .await?;
            update_rom_romfile(&mut transaction, cue_rom.id, Some(cue_romfile_id)).await;
            chd_romfile
                .romfile
                .update(&mut transaction, progress_bar, romfile.id)
                .await?;
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            decoded.source.delete(progress_bar, false).await?;
        }

        commit_transaction(transaction).await;
    }

    // convert CUE/BIN
    for roms in cue_bins.values() {
        let mut transaction = begin_transaction(connection).await;
        let game = games_by_id.get(&roms.first().unwrap().game_id).unwrap();
        let parent_chd_romfile =
            find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;
        let (cue_roms, bin_roms): (Vec<&Rom>, Vec<&Rom>) = roms
            .iter()
            .partition(|rom| rom.name.ends_with(CUE_EXTENSION));
        let cue_bin_romfile = assemble_cue_bin(
            &mut transaction,
            &romfiles_by_id,
            cue_roms.first().unwrap(),
            &bin_roms,
        )
        .await?;
        let chd_romfile = cue_bin_romfile
            .to_chd(
                progress_bar,
                &cue_bin_romfile.cue_romfile.path.parent().unwrap(),
                cd_compression_algorithms,
                cd_hunk_size,
                parent_as_common(&mut transaction, &parent_chd_romfile).await?,
            )
            .await?;

        if check
            && chd_romfile
                .check(&mut transaction, progress_bar, &None, &bin_roms)
                .await
                .is_err()
        {
            print_error(progress_bar, "Integrity check failed, reverting conversion");
            chd_romfile.romfile.delete(progress_bar, false).await?;
            continue;
        };

        if diff {
            let roms = [cue_roms.as_slice(), bin_roms.as_slice()].concat();
            let romfiles = [
                &[cue_bin_romfile.cue_romfile],
                cue_bin_romfile.bin_romfiles.as_slice(),
            ]
            .concat();
            print_diff(
                &mut transaction,
                progress_bar,
                &roms,
                &romfiles.iter().collect::<Vec<&CommonRomfile>>(),
                &[&chd_romfile.romfile],
            )
            .await?;
        }

        let chd_romfile_id = chd_romfile
            .romfile
            .create(&mut transaction, progress_bar, RomfileType::Romfile)
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
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile =
            find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;
        let decoded = common_as_source(&mut transaction, romfile).await?;
        let chd_romfile = decoded
            .inner
            .as_iso()?
            .to_chd(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                dvd_compression_algorithms,
                dvd_hunk_size,
                parent_as_common(&mut transaction, &parent_chd_romfile).await?,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &chd_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            commit_transaction(transaction).await;
        }
    }

    // convert CSOs/ZSOs
    for roms in csos.values().chain(zsos.values()) {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile =
            find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;
        let decoded = xso_to_iso(&mut transaction, progress_bar, romfile, &tmp_directory).await?;
        let chd_romfile = decoded
            .inner
            .to_chd(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                dvd_compression_algorithms,
                dvd_hunk_size,
                parent_as_common(&mut transaction, &parent_chd_romfile).await?,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &chd_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            update_romfile_parent(
                &mut transaction,
                romfile.id,
                parent_chd_romfile.as_ref().map(|romfile| romfile.id),
            )
            .await;
            commit_transaction(transaction).await;
        }
    }

    // convert RDSKs/RIFFs
    for roms in others.values() {
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let game = games_by_id.get(&rom.game_id).unwrap();
        let parent_chd_romfile =
            find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;
        let mimetype = get_mimetype(&romfile.as_common(&mut transaction).await?.path).await?;
        if let Some(mimetype) = mimetype {
            let chd_romfile = match mimetype.extension() {
                RDSK_EXTENSION => {
                    let rdsk_romfile = romfile.as_common(&mut transaction).await?.as_rdsk().await?;
                    rdsk_romfile
                        .to_chd(
                            progress_bar,
                            &rdsk_romfile.romfile.path.parent().unwrap(),
                            hd_compression_algorithms,
                            hd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                RIFF_EXTENSION => {
                    let riff_romfile = romfile.as_common(&mut transaction).await?.as_riff().await?;
                    riff_romfile
                        .to_chd(
                            progress_bar,
                            &riff_romfile.romfile.path.parent().unwrap(),
                            ld_compression_algorithms,
                            ld_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                _ => {
                    print_skip(progress_bar, "Unsupported file type, skipping");
                    continue;
                }
            };
            if check
                && chd_romfile
                    .check(&mut transaction, progress_bar, &None, &[rom])
                    .await
                    .is_err()
            {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
                chd_romfile.romfile.delete(progress_bar, false).await?;
                continue;
            };
            if diff {
                let common_romfile = romfile.as_common(&mut transaction).await?;
                print_diff(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &[&common_romfile],
                    &[&chd_romfile.romfile],
                )
                .await?;
            }
            chd_romfile
                .romfile
                .update(&mut transaction, progress_bar, romfile.id)
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
            let bin_roms: Vec<&Rom> = roms
                .iter()
                .filter(|rom| !rom.name.ends_with(CUE_EXTENSION))
                .collect();
            let game = games_by_id.get(&bin_roms.first().unwrap().game_id).unwrap();
            let parent_chd_romfile =
                find_parent_chd(&mut transaction, game, parents, prompt_for_parents).await?;
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
                print_skip(progress_bar, "CHD has children, skipping");
                continue;
            }

            let chd_romfile = romfile_as_chd(&mut transaction, romfile).await?;

            let new_chd_romfile = match chd_romfile.chd_type {
                ChdType::Cd => {
                    if chd_romfile.track_count > 1
                        && chdman::get_version()
                            .await?
                            .as_str()
                            .cmp(chdman::MIN_SPLITBIN_VERSION)
                            == Ordering::Less
                    {
                        print_warning(
                            progress_bar,
                            &format!(
                                "chdman {} or newer required for splitbin support",
                                chdman::MIN_SPLITBIN_VERSION
                            ),
                        );
                        continue;
                    }
                    chd_romfile
                        .to_cue_bin(progress_bar, &tmp_directory.path(), None, &[], false)
                        .await?
                        .to_chd(
                            progress_bar,
                            &tmp_directory.path(),
                            cd_compression_algorithms,
                            cd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                ChdType::Dvd => {
                    chd_romfile
                        .to_iso(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(
                            progress_bar,
                            &tmp_directory.path(),
                            dvd_compression_algorithms,
                            dvd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                ChdType::Hd => {
                    chd_romfile
                        .to_rdsk(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(
                            progress_bar,
                            &tmp_directory.path(),
                            hd_compression_algorithms,
                            hd_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
                ChdType::Ld => {
                    chd_romfile
                        .to_riff(progress_bar, &tmp_directory.path())
                        .await?
                        .to_chd(
                            progress_bar,
                            &tmp_directory.path(),
                            ld_compression_algorithms,
                            ld_hunk_size,
                            parent_as_common(&mut transaction, &parent_chd_romfile).await?,
                        )
                        .await?
                }
            };

            if check
                && new_chd_romfile
                    .check(
                        &mut transaction,
                        progress_bar,
                        &None,
                        &[roms.first().unwrap()],
                    )
                    .await
                    .is_err()
            {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
                new_chd_romfile.romfile.delete(progress_bar, false).await?;
                continue;
            } else {
                if diff {
                    print_diff(
                        &mut transaction,
                        progress_bar,
                        &roms.iter().collect::<Vec<&Rom>>(),
                        &[&chd_romfile.romfile],
                        &[&new_chd_romfile.romfile],
                    )
                    .await?;
                }
                chd_romfile.romfile.delete(progress_bar, false).await?;
                new_chd_romfile
                    .romfile
                    .rename(progress_bar, &chd_romfile.romfile.path, false)
                    .await?
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                update_romfile_parent(
                    &mut transaction,
                    romfile.id,
                    parent_chd_romfile.as_ref().map(|romfile| romfile.id),
                )
                .await;
            };

            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

/// Finds or prompts for the parent CHD romfile of a game, honoring the
/// parents/prompt flags.
async fn find_parent_chd(
    transaction: &mut SqliteConnection,
    game: &Game,
    parents: bool,
    prompt_for_parents: bool,
) -> Result<Option<Romfile>> {
    if prompt_for_parents {
        prompt_for_parent_romfile(transaction, game, CHD_EXTENSION).await
    } else if parents {
        Ok(find_parent_chd_romfile_by_game(transaction, game).await)
    } else {
        Ok(None)
    }
}

async fn parent_as_common(
    transaction: &mut SqliteConnection,
    parent_chd_romfile: &Option<Romfile>,
) -> Result<Option<CommonRomfile>> {
    Ok(match parent_chd_romfile {
        Some(romfile) => Some(romfile.as_common(transaction).await?),
        None => None,
    })
}

/// Checks and persists a conversion: on check failure the converted file is
/// deleted and false is returned, leaving the transaction to roll back.
/// Otherwise prints the diff, updates the database row, and deletes the source.
/// When rename_to_source is set the converted file also takes the source's path.
#[allow(clippy::too_many_arguments)]
async fn check_and_persist<T: Check + GetRomfile>(
    transaction: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms: &[&Rom],
    source_romfile: &CommonRomfile,
    target: &T,
    romfile_id: i64,
    check: bool,
    diff: bool,
    rename_to_source: bool,
) -> Result<bool> {
    if check
        && target
            .check(transaction, progress_bar, &None, roms)
            .await
            .is_err()
    {
        print_error(progress_bar, "Integrity check failed, reverting conversion");
        target.romfile().delete(progress_bar, false).await?;
        return Ok(false);
    }
    if diff {
        print_diff(
            transaction,
            progress_bar,
            roms,
            &[source_romfile],
            &[target.romfile()],
        )
        .await?;
    }
    if rename_to_source {
        source_romfile.delete(progress_bar, false).await?;
        target
            .romfile()
            .rename(progress_bar, &source_romfile.path, false)
            .await?
            .update(transaction, progress_bar, romfile_id)
            .await?;
    } else {
        target
            .romfile()
            .update(transaction, progress_bar, romfile_id)
            .await?;
        source_romfile.delete(progress_bar, false).await?;
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn to_xso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: HashMap<i64, Romfile>,
    recompress: bool,
    diff: bool,
    check: bool,
    xso_type: XsoType,
) -> Result<()> {
    // partition archives
    let (archives, others) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[ZIP_EXTENSION, SEVENZIP_EXTENSION],
    );

    // partition ISOs
    let (isos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[ISO_EXTENSION]);

    // partition CHDs
    let (mut chds, others) =
        partition_games_by_extensions(others, &romfiles_by_id, &[CHD_EXTENSION]);
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

    // partition XSOs of the same type
    let (same_xsos, others) =
        partition_games_by_extensions(others, &romfiles_by_id, &[xso_type.extension()]);

    // partition XSOs of the opposite type
    let (opposite_xsos, others) =
        partition_games_by_extensions(others, &romfiles_by_id, &[xso_type.opposite().extension()]);

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded =
            archive_to_common(&mut transaction, progress_bar, rom, romfile, &tmp_directory).await?;
        let xso_romfile = decoded
            .inner
            .as_iso()?
            .to_xso(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                xso_type,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &xso_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded = common_as_source(&mut transaction, romfile).await?;
        let xso_romfile = decoded
            .inner
            .as_iso()?
            .to_xso(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                xso_type,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &xso_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert CHDs
    for roms in chds.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        if !find_romfiles_by_parent_id(&mut transaction, romfile.id)
            .await
            .is_empty()
        {
            print_skip(progress_bar, "CHD has children, skipping");
            continue;
        }
        let Some(decoded) =
            chd_to_iso(&mut transaction, progress_bar, romfile, &tmp_directory).await?
        else {
            continue;
        };
        let xso_romfile = decoded
            .inner
            .to_xso(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                xso_type,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &xso_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            if romfile.parent_id.is_some() {
                update_romfile_parent(&mut transaction, romfile.id, None).await;
            }
            commit_transaction(transaction).await;
        }
    }

    // convert XSOs of the opposite type
    for roms in opposite_xsos.values() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded = xso_to_iso(&mut transaction, progress_bar, romfile, &tmp_directory).await?;
        let xso_romfile = decoded
            .inner
            .to_xso(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                xso_type,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &xso_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // recompress XSOs of the same type
    if recompress {
        for roms in same_xsos.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;
            let rom = roms.first().unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let decoded =
                xso_to_iso(&mut transaction, progress_bar, romfile, &tmp_directory).await?;
            let new_xso_romfile = decoded
                .inner
                .to_xso(progress_bar, &tmp_directory.path(), xso_type)
                .await?;
            if check_and_persist(
                &mut transaction,
                progress_bar,
                &[rom],
                &decoded.source,
                &new_xso_romfile,
                romfile.id,
                check,
                diff,
                true,
            )
            .await?
            {
                commit_transaction(transaction).await;
            }
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
) -> Result<()> {
    // partition archives
    let (archives, others) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[ZIP_EXTENSION, SEVENZIP_EXTENSION],
    );

    // partition NSPs
    let (nsps, others) = partition_games_by_extensions(others, &romfiles_by_id, &[NSP_EXTENSION]);

    // partition NSZs
    let (nszs, others) = partition_games_by_extensions(others, &romfiles_by_id, &[NSZ_EXTENSION]);

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(NSP_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded =
            archive_to_common(&mut transaction, progress_bar, rom, romfile, &tmp_directory).await?;
        let nsz_romfile = decoded
            .inner
            .as_nsp()?
            .to_nsz(progress_bar, &decoded.source.path.parent().unwrap())
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &nsz_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert NSPs
    for roms in nsps.values() {
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded = common_as_source(&mut transaction, romfile).await?;
        let nsz_romfile = decoded
            .inner
            .as_nsp()?
            .to_nsz(progress_bar, &decoded.source.path.parent().unwrap())
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &nsz_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // recompress NSZs
    if recompress {
        for roms in nszs.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;
            let rom = roms.first().unwrap();
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let decoded =
                nsz_to_nsp(&mut transaction, progress_bar, romfile, &tmp_directory).await?;
            let new_nsz_romfile = decoded
                .inner
                .to_nsz(progress_bar, &tmp_directory.path())
                .await?;
            if check_and_persist(
                &mut transaction,
                progress_bar,
                &[rom],
                &decoded.source,
                &new_nsz_romfile,
                romfile.id,
                check,
                diff,
                true,
            )
            .await?
            {
                commit_transaction(transaction).await;
            }
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
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
) -> Result<()> {
    // partition archives
    let (archives, others) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[ZIP_EXTENSION, SEVENZIP_EXTENSION],
    );

    // partition ISOs
    let (isos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[ISO_EXTENSION]);

    // partition RVZs
    let (rvzs, others) = partition_games_by_extensions(others, &romfiles_by_id, &[RVZ_EXTENSION]);

    // drop others
    drop(others);

    // convert archives
    for roms in archives.values() {
        if roms.len() > 1 || !roms.first().unwrap().name.ends_with(ISO_EXTENSION) {
            continue;
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded =
            archive_to_common(&mut transaction, progress_bar, rom, romfile, &tmp_directory).await?;
        let rvz_romfile = decoded
            .inner
            .as_iso()?
            .to_rvz(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                compression_algorithm,
                compression_level,
                block_size,
                false,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &rvz_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert ISOs
    for roms in isos.values() {
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let decoded = common_as_source(&mut transaction, romfile).await?;
        let rvz_romfile = decoded
            .inner
            .as_iso()?
            .to_rvz(
                progress_bar,
                &decoded.source.path.parent().unwrap(),
                compression_algorithm,
                compression_level,
                block_size,
                false,
            )
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &decoded.source,
            &rvz_romfile,
            romfile.id,
            check,
            diff,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // recompress RVZs
    if recompress {
        for roms in rvzs.values() {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut transaction = begin_transaction(connection).await;

            for rom in roms {
                let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                let decoded =
                    rvz_to_iso(&mut transaction, progress_bar, romfile, &tmp_directory).await?;
                let new_rvz_romfile = decoded
                    .inner
                    .to_rvz(
                        progress_bar,
                        &tmp_directory.path(),
                        compression_algorithm,
                        compression_level,
                        block_size,
                        false,
                    )
                    .await?;
                check_and_persist(
                    &mut transaction,
                    progress_bar,
                    &[rom],
                    &decoded.source,
                    &new_rvz_romfile,
                    romfile.id,
                    check,
                    diff,
                    true,
                )
                .await?;
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
) -> Result<()> {
    // partition archives
    let (archives, others) = partition_games_by_extensions(
        roms_by_game_id,
        &romfiles_by_id,
        &[ZIP_EXTENSION, SEVENZIP_EXTENSION],
    );

    // partition CHDs
    let (mut chds, others) =
        partition_games_by_extensions(others, &romfiles_by_id, &[CHD_EXTENSION]);
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
    let (csos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[CSO_EXTENSION]);

    // partition NSZs
    let (nszs, others) = partition_games_by_extensions(others, &romfiles_by_id, &[NSZ_EXTENSION]);

    // partition RVZs
    let (rvzs, others) = partition_games_by_extensions(others, &romfiles_by_id, &[RVZ_EXTENSION]);

    // partition ZSOs
    let (zsos, others) = partition_games_by_extensions(others, &romfiles_by_id, &[ZSO_EXTENSION]);

    // drop originals
    drop(others);

    // convert archives
    for roms in archives.values() {
        if sevenzip::get_version().await.is_err() {
            print_error(progress_bar, "Required tool not found: sevenzip");
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
        let archive_romfiles = romfile
            .as_common(&mut transaction)
            .await?
            .as_archive(progress_bar, None)
            .await?;
        let roms: Vec<&Rom> = roms
            .iter()
            .filter(|rom| rom.romfile_id.unwrap() == romfile.id)
            .collect();

        let destination_directory = match system.arcade {
            true => {
                let romfile_path = &archive_romfiles.first().unwrap().romfile.path;
                let directory = romfile_path
                    .parent()
                    .unwrap()
                    .join(romfile_path.file_stem().unwrap());
                create_directory(progress_bar, &directory, false).await?;
                directory
            }
            false => archive_romfiles
                .first()
                .unwrap()
                .romfile
                .path
                .parent()
                .unwrap()
                .to_path_buf(),
        };

        let mut common_romfiles: Vec<CommonRomfile> = vec![];
        for rom in &roms {
            common_romfiles.push(
                archive_romfiles
                    .iter()
                    .find(|archive_romfile| archive_romfile.path == rom.name)
                    .unwrap()
                    .to_common(progress_bar, &destination_directory)
                    .await?,
            );
        }

        if check {
            let mut error = false;
            for (common_romfile, rom) in common_romfiles
                .iter()
                .zip(roms.iter())
                .collect::<Vec<(&CommonRomfile, &&Rom)>>()
            {
                if common_romfile
                    .check(&mut transaction, progress_bar, &None, &[rom])
                    .await
                    .is_err()
                {
                    error = true;
                    break;
                };
            }
            if error {
                print_error(progress_bar, "Integrity check failed, reverting conversion");
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
                .create(&mut transaction, progress_bar, RomfileType::Romfile)
                .await?;
            update_rom_romfile(&mut transaction, rom.id, Some(romfile_id)).await;
        }

        delete_romfile_by_id(&mut transaction, romfile.id).await;
        archive_romfiles
            .first()
            .unwrap()
            .romfile
            .delete(progress_bar, false)
            .await?;

        commit_transaction(transaction).await;
    }

    // convert CHDs
    for roms in chds.values() {
        // leave arcade CHDs untouched
        if roms.iter().any(|rom| rom.disk) {
            continue;
        }
        if chdman::get_version().await.is_err() {
            print_error(progress_bar, "Required tool not found: chdman");
            break;
        }
        let mut transaction = begin_transaction(connection).await;
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
            print_skip(progress_bar, "CHD has children, skipping");
            continue;
        }
        let chd_romfile = romfile_as_chd(&mut transaction, romfile).await?;
        match chd_romfile.chd_type {
            ChdType::Cd => {
                if chd_romfile.track_count > 1
                    && chdman::get_version()
                        .await?
                        .as_str()
                        .cmp(chdman::MIN_SPLITBIN_VERSION)
                        == Ordering::Less
                {
                    print_warning(
                        progress_bar,
                        &format!(
                            "chdman {} or newer required for splitbin support",
                            chdman::MIN_SPLITBIN_VERSION
                        ),
                    );
                    continue;
                }
                let cue_romfile = romfiles_by_id
                    .get(&cue_roms.first().unwrap().romfile_id.unwrap())
                    .unwrap()
                    .as_common(&mut transaction)
                    .await?;
                let cue_bin_romfile = chd_romfile
                    .to_cue_bin(
                        progress_bar,
                        &chd_romfile.romfile.path.parent().unwrap(),
                        Some(cue_romfile),
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
                            .check(&mut transaction, progress_bar, &None, &[bin_rom])
                            .await
                            .is_err()
                        {
                            error = true;
                            break;
                        };
                    }
                    if error {
                        print_error(progress_bar, "Integrity check failed, reverting conversion");
                        if cue_roms.is_empty() {
                            cue_bin_romfile
                                .cue_romfile
                                .delete(progress_bar, false)
                                .await?;
                        }
                        for bin_romfile in cue_bin_romfile.bin_romfiles {
                            bin_romfile.delete(progress_bar, false).await?;
                        }
                        continue;
                    }
                }

                if cue_roms.is_empty() {
                    cue_bin_romfile
                        .cue_romfile
                        .delete(progress_bar, false)
                        .await?;
                }
                for (bin_romfile, bin_rom) in cue_bin_romfile
                    .bin_romfiles
                    .iter()
                    .zip(&bin_roms)
                    .collect::<Vec<(&CommonRomfile, &&Rom)>>()
                {
                    let romfile_id = bin_romfile
                        .create(&mut transaction, progress_bar, RomfileType::Romfile)
                        .await?;
                    update_rom_romfile(&mut transaction, bin_rom.id, Some(romfile_id)).await;
                }
                delete_romfile_by_id(&mut transaction, romfile.id).await;
                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Dvd => {
                let iso_romfile = chd_romfile
                    .to_iso(progress_bar, &chd_romfile.romfile.path.parent().unwrap())
                    .await?;

                if check
                    && iso_romfile
                        .romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &roms.iter().collect::<Vec<&Rom>>(),
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    iso_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                iso_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                if romfile.parent_id.is_some() {
                    update_romfile_parent(&mut transaction, romfile.id, None).await;
                }
                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Hd => {
                let rdsk_romfile = chd_romfile
                    .to_rdsk(progress_bar, &chd_romfile.romfile.path.parent().unwrap())
                    .await?;

                if check
                    && rdsk_romfile
                        .romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &roms.iter().collect::<Vec<&Rom>>(),
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    rdsk_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                rdsk_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                if romfile.parent_id.is_some() {
                    update_romfile_parent(&mut transaction, romfile.id, None).await;
                }
                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
            ChdType::Ld => {
                let riff_romfile = chd_romfile
                    .to_riff(progress_bar, &chd_romfile.romfile.path.parent().unwrap())
                    .await?;

                if check
                    && riff_romfile
                        .romfile
                        .check(
                            &mut transaction,
                            progress_bar,
                            &None,
                            &roms.iter().collect::<Vec<&Rom>>(),
                        )
                        .await
                        .is_err()
                {
                    print_error(progress_bar, "Integrity check failed, reverting conversion");
                    riff_romfile.romfile.delete(progress_bar, false).await?;
                    continue;
                };

                riff_romfile
                    .romfile
                    .update(&mut transaction, progress_bar, romfile.id)
                    .await?;
                if romfile.parent_id.is_some() {
                    update_romfile_parent(&mut transaction, romfile.id, None).await;
                }
                chd_romfile.romfile.delete(progress_bar, false).await?;
            }
        }

        commit_transaction(transaction).await;
    }

    // convert CSOs/ZSOs
    for roms in csos.values().chain(zsos.values()) {
        if maxcso::get_version().await.is_err() {
            print_error(progress_bar, "Required tool not found: maxcso");
            break;
        }
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let xso_romfile = romfile.as_common(&mut transaction).await?.as_xso().await?;
        let iso_romfile = xso_romfile
            .to_iso(progress_bar, &xso_romfile.romfile.path.parent().unwrap())
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &xso_romfile.romfile,
            &iso_romfile.romfile,
            romfile.id,
            check,
            false,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert NSZs
    for roms in nszs.values() {
        if nsz::get_version().await.is_err() {
            print_error(progress_bar, "Required tool not found: nsz");
            break;
        }
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let nsz_romfile = romfile.as_common(&mut transaction).await?.as_nsz()?;
        let nsp_romfile = nsz_romfile
            .to_nsp(progress_bar, &nsz_romfile.romfile.path.parent().unwrap())
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &nsz_romfile.romfile,
            &nsp_romfile.romfile,
            romfile.id,
            check,
            false,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    // convert RVZs
    for roms in rvzs.values() {
        if dolphin::get_version().await.is_err() {
            print_error(progress_bar, "Required tool not found: dolphin-tool");
            break;
        }
        let mut transaction = begin_transaction(connection).await;
        let rom = roms.first().unwrap();
        let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
        let rvz_romfile = romfile.as_common(&mut transaction).await?.as_rvz()?;
        let iso_romfile = rvz_romfile
            .to_iso(progress_bar, &rvz_romfile.romfile.path.parent().unwrap())
            .await?;
        if check_and_persist(
            &mut transaction,
            progress_bar,
            &[rom],
            &rvz_romfile.romfile,
            &iso_romfile.romfile,
            romfile.id,
            check,
            false,
            false,
        )
        .await?
        {
            commit_transaction(transaction).await;
        }
    }

    Ok(())
}

async fn print_diff(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms: &[&Rom],
    old_romfiles: &[&CommonRomfile],
    new_romfiles: &[&CommonRomfile],
) -> Result<()> {
    let original_size = roms.par_iter().map(|&r| r.size as u64).sum();
    let mut old_size = 0u64;
    for &romfile in old_romfiles {
        old_size += romfile.get_size(connection, progress_bar).await?;
    }
    let mut new_size = 0u64;
    for &romfile in new_romfiles {
        new_size += romfile.get_size(connection, progress_bar).await?;
    }
    print_info(
        progress_bar,
        &format!(
            "Before: {} ({:.1}%); After: {} ({:.1}%); Original: {}",
            HumanBytes(old_size),
            old_size as f64 / original_size as f64 * 100f64,
            HumanBytes(new_size),
            new_size as f64 / original_size as f64 * 100f64,
            HumanBytes(original_size)
        ),
    );
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
mod test_original_to_sevenzip_zstd;
#[cfg(test)]
mod test_original_to_zip;
#[cfg(test)]
mod test_original_to_zip_multiple_roms;
#[cfg(test)]
mod test_original_to_zip_with_correct_name;
#[cfg(test)]
mod test_original_to_zip_with_incorrect_name;
#[cfg(test)]
mod test_original_to_zip_zstd;
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
