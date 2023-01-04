use super::config::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use num_traits::FromPrimitive;
use sqlx::sqlite::SqliteConnection;
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

    let merging = match matches.get_one::<String>("STRATEGY").map(String::as_str) {
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

    progress_bar.enable_steady_tick(Duration::from_millis(100));

    for system in systems {
        if system.merging == merging as i64 {
            progress_bar.println("Nothing to do");
            progress_bar.println("");
            continue;
        }
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
    progress_bar.println(&format!("Processing \"{}\"", system.name));

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

    // mark games and system as complete if they are
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.set_message("Computing system completion");
    update_games_by_system_id_mark_complete(connection, system.id).await;
    update_system_mark_complete(connection, system.id).await;

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
    progress_bar.println(&format!("Processing \"{}\"", game.name));
    let tmp_directory = get_tmp_directory(connection).await;
    let mut transaction = begin_transaction(connection).await;
    let archive_romfile_path = get_system_directory(&mut transaction, progress_bar, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let archive_romfile = match find_romfile_by_path(
        &mut transaction,
        archive_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    {
        Some(romfile) => romfile,
        None => {
            let romfile_id = create_romfile(
                &mut transaction,
                archive_romfile_path.as_os_str().to_str().unwrap(),
                archive_romfile_path.metadata().await.unwrap().len(),
            )
            .await;
            find_romfile_by_id(&mut transaction, romfile_id).await
        }
    };
    let roms = match merging {
        Merging::NonMerged => {
            find_roms_by_game_id_parents_no_parent_bioses(&mut transaction, game.id).await
        }
        Merging::FullNonMerged => find_roms_by_game_id_parents(&mut transaction, game.id).await,
        _ => bail!("Not possible"),
    };
    for rom in &roms {
        add_rom(
            &mut transaction,
            progress_bar,
            system,
            rom,
            &archive_romfile,
            tmp_directory,
            compression_level,
        )
        .await?;
    }
    update_romfile(
        &mut transaction,
        archive_romfile.id,
        &archive_romfile.path,
        archive_romfile_path.metadata().await.unwrap().len(),
    )
    .await;
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
    progress_bar.println(&format!("Processing \"{}\"", game.name));
    let mut transaction = begin_transaction(connection).await;
    let archive_romfile_path = get_system_directory(&mut transaction, progress_bar, system)
        .await?
        .join(format!("{}.{}", &game.name, ZIP_EXTENSION));
    let archive_romfile = find_romfile_by_path(
        &mut transaction,
        archive_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();
    let roms = match merging {
        Merging::Split => find_roms_by_game_id_parents_only(&mut transaction, game.id).await,
        Merging::NonMerged => {
            find_roms_by_game_id_parent_bioses_only(&mut transaction, game.id).await
        }
        _ => bail!("Not possible"),
    };
    for rom in &roms {
        remove_rom(&mut transaction, progress_bar, rom, &archive_romfile).await?;
    }
    update_romfile(
        &mut transaction,
        archive_romfile.id,
        &archive_romfile.path,
        archive_romfile_path.metadata().await.unwrap().len(),
    )
    .await;
    commit_transaction(transaction).await;
    Ok(())
}

async fn add_rom(
    transaction: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    rom: &Rom,
    archive_romfile: &Romfile,
    tmp_directory: &PathBuf,
    compression_level: usize,
) -> SimpleResult<()> {
    match rom.romfile_id {
        Some(romfile_id) => {
            if romfile_id != archive_romfile.id {
                let romfile = find_romfile_by_id(transaction, romfile_id).await;
                let file_names = vec![rom.name.as_str()];
                if romfile.path.ends_with(ZIP_EXTENSION) {
                    sevenzip::extract_files_from_archive(
                        progress_bar,
                        &romfile.path,
                        &file_names,
                        tmp_directory,
                    )?;
                    sevenzip::add_files_to_archive(
                        progress_bar,
                        &archive_romfile.path,
                        &file_names,
                        tmp_directory,
                        compression_level,
                        false,
                    )?;
                    remove_file(progress_bar, &tmp_directory.join(&rom.name), true).await?;
                } else {
                    sevenzip::add_files_to_archive(
                        progress_bar,
                        &archive_romfile.path,
                        &file_names,
                        &Path::new(&romfile.path).parent().unwrap(),
                        compression_level,
                        false,
                    )?;
                }
                update_rom_romfile(transaction, rom.id, Some(archive_romfile.id)).await;
                if find_roms_by_romfile_id(transaction, romfile_id)
                    .await
                    .is_empty()
                {
                    remove_file(progress_bar, &romfile.path, false).await?;
                    delete_romfile_by_id(transaction, romfile_id).await;
                }
            }
        }
        None => {
            let mut existing_roms = find_roms_with_romfile_by_size_and_crc_and_system_id(
                transaction,
                rom.size,
                rom.crc.as_ref().unwrap(),
                system.id,
            )
            .await;
            if !existing_roms.is_empty() {
                let existing_rom = existing_roms.remove(0);
                let existing_romfile =
                    find_romfile_by_id(transaction, existing_rom.romfile_id.unwrap()).await;
                let file_names = vec![existing_rom.name.as_str()];
                if existing_romfile.path.ends_with(ZIP_EXTENSION) {
                    sevenzip::extract_files_from_archive(
                        progress_bar,
                        &existing_romfile.path,
                        &file_names,
                        tmp_directory,
                    )?;
                    if existing_rom.name != rom.name {
                        rename_file(
                            progress_bar,
                            &tmp_directory.join(&existing_rom.name),
                            &tmp_directory.join(&rom.name),
                            true,
                        )
                        .await?;
                    }
                    sevenzip::add_files_to_archive(
                        progress_bar,
                        &archive_romfile.path,
                        &[rom.name.as_str()],
                        tmp_directory,
                        compression_level,
                        false,
                    )?;
                    remove_file(progress_bar, &tmp_directory.join(&rom.name), true).await?;
                } else if existing_rom.name != rom.name {
                    copy_file(
                        progress_bar,
                        &existing_romfile.path,
                        &tmp_directory.join(&rom.name),
                        true,
                    )
                    .await?;
                    sevenzip::add_files_to_archive(
                        progress_bar,
                        &archive_romfile.path,
                        &[rom.name.as_str()],
                        tmp_directory,
                        compression_level,
                        false,
                    )?;
                    remove_file(progress_bar, &tmp_directory.join(&rom.name), true).await?;
                } else {
                    sevenzip::add_files_to_archive(
                        progress_bar,
                        &archive_romfile.path,
                        &file_names,
                        &Path::new(&existing_romfile.path).parent().unwrap(),
                        compression_level,
                        false,
                    )?;
                }
                update_rom_romfile(transaction, rom.id, Some(archive_romfile.id)).await;
            }
        }
    }
    Ok(())
}

async fn remove_rom(
    transaction: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    archive_romfile: &Romfile,
) -> SimpleResult<()> {
    if let Some(romfile_id) = rom.romfile_id {
        if romfile_id == archive_romfile.id {
            let romfile = find_romfile_by_id(transaction, romfile_id).await;
            let file_names = vec![rom.name.as_str()];
            if romfile.path.ends_with(ZIP_EXTENSION) {
                sevenzip::remove_files_from_archive(progress_bar, &romfile.path, &file_names)?;
            } else {
                remove_file(progress_bar, &romfile.path, false).await?;
            }
            update_rom_romfile(transaction, rom.id, None).await;
        }
    }
    Ok(())
}
