use super::SimpleResult;
use super::common::*;
use super::config::*;
use super::database::*;
use super::import_roms::{UnattendedMode, import_other};
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use num_traits::FromPrimitive;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

const MERGING_STRATEGIES: &[&str] = &["SPLIT", "NON_MERGED", "FULL_NON_MERGED"];

pub fn subcommand() -> Command {
    Command::new("rebuild-roms")
        .about("Rebuild arcade ROM sets according to the selected strategy")
        .arg(
            Arg::new("MERGING")
                .short('m')
                .long("merging")
                .help("Set the arcade merging strategy")
                .required(false)
                .num_args(1)
                .value_parser(PossibleValuesParser::new(MERGING_STRATEGIES)),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force rebuild even if merging strategy is unchanged")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Rebuild all arcade systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically say yes to prompts")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems =
        prompt_for_systems(connection, None, true, false, matches.get_flag("ALL")).await?;

    let merging = match matches.get_one::<String>("MERGING").map(String::as_str) {
        Some("SPLIT") => Merging::Split,
        Some("NON_MERGED") => Merging::NonMerged,
        Some("FULL_NON_MERGED") => Merging::FullNonMerged,
        Some(&_) | None => FromPrimitive::from_usize(select(
            MERGING_STRATEGIES,
            "Please select a merge strategy",
            None,
            None,
        )?)
        .unwrap(),
    };

    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));
        rebuild_system(
            connection,
            progress_bar,
            &system,
            merging,
            matches.get_flag("FORCE"),
        )
        .await?;
        progress_bar.println("");
    }

    Ok(())
}

async fn rebuild_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    merging: Merging,
    force: bool,
) -> SimpleResult<()> {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let tmp_directory = create_tmp_directory(connection).await?;
    let partial_games = find_partial_games_by_system_id(connection, system.id).await;
    let missing_roms = find_roms_without_romfile_by_game_ids(
        connection,
        &partial_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
    )
    .await;
    for missing_rom in missing_roms {
        if missing_rom.size == 0 {
            continue;
        }
        let mut existing_rom: Option<Rom> = None;
        if let Some(crc) = missing_rom.crc {
            let mut roms = find_roms_with_romfile_by_size_and_crc_and_system_id(
                connection,
                missing_rom.size,
                &crc,
                system.id,
            )
            .await;
            existing_rom = roms.pop();
        }
        if existing_rom.is_none() {
            if let Some(md5) = missing_rom.md5 {
                let mut roms = find_roms_with_romfile_by_size_and_md5_and_system_id(
                    connection,
                    missing_rom.size,
                    &md5,
                    system.id,
                )
                .await;
                existing_rom = roms.pop();
            }
        }
        if existing_rom.is_none() {
            if let Some(sha1) = missing_rom.sha1 {
                let mut roms = find_roms_with_romfile_by_size_and_sha1_and_system_id(
                    connection,
                    missing_rom.size,
                    &sha1,
                    system.id,
                )
                .await;
                existing_rom = roms.pop();
            }
        }
        if let Some(existing_rom) = existing_rom {
            let romfile = find_romfile_by_id(connection, existing_rom.romfile_id.unwrap())
                .await
                .as_common(connection)
                .await?;
            let mimetype = get_mimetype(&romfile.path).await?;
            let new_romfile = if mimetype.is_some()
                && ARCHIVE_EXTENSIONS.contains(&mimetype.as_ref().unwrap().extension())
            {
                romfile
                    .as_archive(progress_bar, Some(&existing_rom))
                    .await?
                    .first()
                    .unwrap()
                    .to_common(progress_bar, &tmp_directory)
                    .await?
            } else {
                romfile.copy(progress_bar, &tmp_directory, false).await?
            };
            import_other(
                connection,
                progress_bar,
                &Some(system),
                &None,
                &HashSet::from_iter(vec![missing_rom.game_id]),
                new_romfile,
                false,
                false,
                UnattendedMode::Skip,
            )
            .await?;
        }
    }
    compute_system_completion(connection, progress_bar, system).await;

    if system.merging == merging as i64 && !force {
        progress_bar.println("Nothing to do");
        return Ok(());
    }

    let games = find_full_games_by_system_id(connection, system.id).await;

    if (system.merging == Merging::Split as i64 || system.merging == Merging::NonMerged as i64)
        && (merging == Merging::NonMerged || merging == Merging::FullNonMerged)
    {
        let compression_level = get_integer(connection, "ZIP_COMPRESSION_LEVEL").await;
        for game in games {
            expand_game(
                connection,
                progress_bar,
                system,
                &game,
                merging,
                &compression_level,
            )
            .await?;
        }
    } else if (system.merging == Merging::NonMerged as i64
        || system.merging == Merging::FullNonMerged as i64)
        && (merging == Merging::Split || merging == Merging::NonMerged)
    {
        for game in games {
            trim_game(connection, progress_bar, system, &game, merging).await?;
        }
    }

    update_system_merging(connection, system.id, merging).await;
    compute_system_completion(connection, progress_bar, system).await;

    Ok(())
}

async fn expand_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
    merging: Merging,
    compression_level: &Option<usize>,
) -> SimpleResult<()> {
    progress_bar.println(format!("Processing \"{}\"", game.name));
    let rom_directory = get_rom_directory(connection).await;
    let game_directory = get_system_directory(connection, system)
        .await?
        .join(&game.name);
    let game_archive_path = get_system_directory(connection, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let relative_game_archive_path = try_with!(
        game_archive_path.strip_prefix(rom_directory),
        "Failed to retrieve relative path"
    );
    let archive_romfile = find_romfile_by_path(
        connection,
        relative_game_archive_path.as_os_str().to_str().unwrap(),
    )
    .await;
    let mut roms = match merging {
        Merging::NonMerged => {
            find_roms_by_game_id_parents_no_parent_bioses(connection, game.id).await
        }
        Merging::FullNonMerged => find_roms_by_game_id_parents(connection, game.id).await,
        _ => bail!("Not possible"),
    };
    // skip CHDs
    roms.retain(|rom| !rom.disk);
    let mut transaction = begin_transaction(connection).await;
    for rom in roms.iter().filter(|rom| rom.romfile_id.is_none()) {
        // find source rom in parent
        let mut source_rom: Option<Rom> = None;
        if let Some(parent_id) = rom.parent_id {
            let parent_rom = find_rom_by_id(&mut transaction, parent_id).await;
            if parent_rom.romfile_id.is_some() {
                source_rom = Some(parent_rom);
            }
        };
        if let Some(source_rom) = source_rom {
            add_rom(
                &mut transaction,
                progress_bar,
                game,
                rom,
                &source_rom,
                &game_directory,
                &archive_romfile,
                compression_level,
            )
            .await?;
        } else {
            progress_bar.println(format!("Missing \"{}\"", &rom.name));
            return Ok(());
        }
    }
    if let Some(romfile) = archive_romfile {
        romfile
            .as_common(&mut transaction)
            .await?
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;
    }
    commit_transaction(transaction).await;
    Ok(())
}

async fn trim_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
    merging: Merging,
) -> SimpleResult<()> {
    progress_bar.println(format!("Processing \"{}\"", game.name));
    let rom_directory = get_rom_directory(connection).await;
    let romfile_path = get_system_directory(connection, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let relative_path = try_with!(
        romfile_path.strip_prefix(rom_directory),
        "Failed to retrieve relative path"
    );
    let romfile =
        find_romfile_by_path(connection, relative_path.as_os_str().to_str().unwrap()).await;
    let roms = match merging {
        Merging::Split => find_roms_by_game_id_parents_only(connection, game.id).await,
        Merging::NonMerged => find_roms_by_game_id_parent_bioses_only(connection, game.id).await,
        _ => bail!("Not possible"),
    };
    let mut transaction = begin_transaction(connection).await;
    for rom in roms.iter().filter(|rom| rom.romfile_id.is_some()) {
        delete_rom(&mut transaction, progress_bar, rom, &romfile).await?;
    }
    if let Some(romfile) = romfile {
        romfile
            .as_common(&mut transaction)
            .await?
            .update(&mut transaction, progress_bar, romfile.id)
            .await?;
    }
    commit_transaction(transaction).await;
    Ok(())
}

async fn add_rom(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
    rom: &Rom,
    source_rom: &Rom,
    destination_directory: &PathBuf,
    destination_archive_romfile: &Option<Romfile>,
    compression_level: &Option<usize>,
) -> SimpleResult<()> {
    let source_romfile = find_romfile_by_id(connection, source_rom.romfile_id.unwrap())
        .await
        .as_common(connection)
        .await?;
    if let Some(destination_archive_romfile) = destination_archive_romfile {
        if source_romfile.path.extension().unwrap() == ZIP_EXTENSION {
            // both source and destination are archives
            copy_files_between_archives(
                progress_bar,
                &source_romfile.path,
                &destination_archive_romfile
                    .as_common(connection)
                    .await?
                    .path,
                &[&source_rom.name],
                &[&rom.name],
            )
            .await?;
        } else {
            // source is file and destination is archive
            let archive_romfile = source_romfile
                .to_archive(
                    progress_bar,
                    &source_romfile.path.parent().unwrap(),
                    &destination_archive_romfile
                        .as_common(connection)
                        .await?
                        .path
                        .parent()
                        .unwrap(),
                    &game.name,
                    &ArchiveType::Zip,
                    compression_level,
                    false,
                )
                .await?;
            if source_rom.name != rom.name {
                archive_romfile.rename_file(progress_bar, &rom.name).await?;
            }
        }
        update_rom_romfile(connection, rom.id, Some(destination_archive_romfile.id)).await;
    } else if source_romfile.path.extension().unwrap() == ZIP_EXTENSION {
        // source is archive and destination is file
        let romfile_id = source_romfile
            .as_archive(progress_bar, Some(source_rom))
            .await?
            .first()
            .unwrap()
            .to_common(progress_bar, destination_directory)
            .await?
            .rename(progress_bar, &destination_directory.join(&rom.name), true)
            .await?
            .create(connection, progress_bar, RomfileType::Romfile)
            .await?;
        update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
    } else {
        // source and destination are files
        let romfile_id = source_romfile
            .copy(progress_bar, destination_directory, false)
            .await?
            .create(connection, progress_bar, RomfileType::Romfile)
            .await?;
        update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
    }
    Ok(())
}

async fn delete_rom(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    archive_romfile: &Option<Romfile>,
) -> SimpleResult<()> {
    if let Some(archive_romfile) = archive_romfile {
        archive_romfile
            .as_common(connection)
            .await?
            .as_archive(progress_bar, Some(rom))
            .await?
            .first()
            .unwrap()
            .delete_file(progress_bar)
            .await?;
    } else {
        find_romfile_by_id(connection, rom.romfile_id.unwrap())
            .await
            .as_common(connection)
            .await?
            .delete(progress_bar, false)
            .await?;
    }
    update_rom_romfile(connection, rom.id, None).await;
    Ok(())
}
