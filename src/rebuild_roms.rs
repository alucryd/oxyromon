use super::config::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip;
use super::util::*;
use super::SimpleResult;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use num_traits::FromPrimitive;
use sqlx::sqlite::SqliteConnection;
use std::path::{Path, PathBuf};
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
    let systems = prompt_for_systems(connection, None, true, matches.get_flag("ALL")).await?;

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
        rebuild_system(connection, progress_bar, &system, merging).await?;
        progress_bar.println("");
    }

    Ok(())
}

async fn rebuild_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    merging: Merging,
) -> SimpleResult<()> {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    if system.merging == merging as i64 {
        progress_bar.println("Nothing to do");
        return Ok(());
    }

    let games = find_games_with_romfiles_by_system_id(connection, system.id).await;

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
                compression_level,
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
    compute_arcade_system_completion(connection, progress_bar, system).await;

    Ok(())
}

async fn expand_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
    merging: Merging,
    compression_level: usize,
) -> SimpleResult<()> {
    progress_bar.println(format!("Processing \"{}\"", game.name));
    let game_directory = get_system_directory(connection, system)
        .await?
        .join(&game.name);
    let mut transaction = begin_transaction(connection).await;
    let archive_romfile_path = get_system_directory(&mut transaction, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let archive_romfile = find_romfile_by_path(
        &mut transaction,
        archive_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await;
    let roms = match merging {
        Merging::NonMerged => {
            find_roms_by_game_id_parents_no_parent_bioses(&mut transaction, game.id).await
        }
        Merging::FullNonMerged => find_roms_by_game_id_parents(&mut transaction, game.id).await,
        _ => bail!("Not possible"),
    };
    for rom in roms.iter().filter(|rom| rom.romfile_id.is_none()) {
        let mut source_rom: Option<Rom> = None;
        if let Some(parent_id) = rom.parent_id {
            let parent_rom = find_rom_by_id(&mut transaction, parent_id).await;
            if parent_rom.romfile_id.is_some() {
                source_rom = Some(parent_rom);
            }
        };
        if source_rom.is_none() {
            let mut existing_roms = find_roms_with_romfile_by_size_and_crc_and_system_id(
                &mut transaction,
                rom.size,
                rom.crc.as_ref().unwrap(),
                system.id,
            )
            .await;
            if !existing_roms.is_empty() {
                source_rom = Some(existing_roms.remove(0));
            }
        }
        if let Some(source_rom) = source_rom {
            add_rom(
                &mut transaction,
                progress_bar,
                rom,
                &source_rom,
                &archive_romfile,
                &game_directory,
                compression_level,
            )
            .await?;
        } else {
            progress_bar.println(format!("Missing \"{}\"", &rom.name));
            return Ok(());
        }
    }
    if let Some(romfile) = archive_romfile {
        update_romfile(
            &mut transaction,
            romfile.id,
            &romfile.path,
            archive_romfile_path.metadata().unwrap().len(),
        )
        .await;
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
    let mut transaction = begin_transaction(connection).await;
    let archive_romfile_path = get_system_directory(&mut transaction, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let archive_romfile = find_romfile_by_path(
        &mut transaction,
        archive_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await;
    let roms = match merging {
        Merging::Split => find_roms_by_game_id_parents_only(&mut transaction, game.id).await,
        Merging::NonMerged => {
            find_roms_by_game_id_parent_bioses_only(&mut transaction, game.id).await
        }
        _ => bail!("Not possible"),
    };
    for rom in roms.iter().filter(|rom| rom.romfile_id.is_some()) {
        delete_rom(&mut transaction, progress_bar, rom, &archive_romfile).await?;
    }
    if let Some(romfile) = archive_romfile {
        update_romfile(
            &mut transaction,
            romfile.id,
            &romfile.path,
            archive_romfile_path.metadata().unwrap().len(),
        )
        .await;
    }
    commit_transaction(transaction).await;
    Ok(())
}

async fn add_rom(
    transaction: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    source_rom: &Rom,
    archive_romfile: &Option<Romfile>,
    game_directory: &PathBuf,
    compression_level: usize,
) -> SimpleResult<()> {
    let source_romfile = find_romfile_by_id(transaction, source_rom.romfile_id.unwrap()).await;
    if let Some(archive_romfile) = archive_romfile {
        if source_romfile.path.ends_with(ZIP_EXTENSION) {
            // both source and destination are archives
            sevenzip::copy_files_between_archives(
                progress_bar,
                &source_romfile.path,
                &archive_romfile.path,
                &[&source_rom.name],
                &[&rom.name],
            )
            .await?;
            update_rom_romfile(transaction, rom.id, Some(archive_romfile.id)).await;
        } else {
            // source is directory and destination is archive
            sevenzip::add_files_to_archive(
                progress_bar,
                &archive_romfile.path,
                &[&source_rom.name],
                &Path::new(&source_romfile.path).parent().unwrap(),
                compression_level,
                false,
            )
            .await?;
            if source_rom.name != rom.name {
                sevenzip::rename_file_in_archive(
                    progress_bar,
                    &archive_romfile.path,
                    &source_rom.name,
                    &rom.name,
                )
                .await?;
            }
            update_rom_romfile(transaction, rom.id, Some(source_romfile.id)).await;
        }
    } else if source_romfile.path.ends_with(ZIP_EXTENSION) {
        // source is archive and destination is directory
        let romfile_path = game_directory.join(&rom.name);
        sevenzip::extract_files_from_archive(
            progress_bar,
            &source_romfile.path,
            &[&source_rom.name],
            game_directory,
        )
        .await?;
        if source_rom.name != rom.name {
            rename_file(
                progress_bar,
                &game_directory.join(&source_rom.name),
                &romfile_path,
                true,
            )
            .await?;
        }
        let romfile_id = create_romfile(
            transaction,
            romfile_path.as_os_str().to_str().unwrap(),
            romfile_path.metadata().unwrap().len(),
        )
        .await;
        update_rom_romfile(transaction, rom.id, Some(romfile_id)).await;
    } else {
        // source and destination are directories
        let romfile_path = game_directory.join(&rom.name);
        copy_file(progress_bar, &source_romfile.path, &romfile_path, false).await?;
        let romfile_id = create_romfile(
            transaction,
            romfile_path.as_os_str().to_str().unwrap(),
            romfile_path.metadata().unwrap().len(),
        )
        .await;
        update_rom_romfile(transaction, rom.id, Some(romfile_id)).await;
    }
    Ok(())
}

async fn delete_rom(
    transaction: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    archive_romfile: &Option<Romfile>,
) -> SimpleResult<()> {
    if let Some(archive_romfile) = archive_romfile {
        sevenzip::remove_files_from_archive(progress_bar, &archive_romfile.path, &[&rom.name])
            .await?;
    } else {
        let romfile = find_romfile_by_id(transaction, rom.romfile_id.unwrap()).await;
        remove_file(progress_bar, &romfile.path, false).await?;
    }
    update_rom_romfile(transaction, rom.id, None).await;
    Ok(())
}
