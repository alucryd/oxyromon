use super::archive::*;
use super::common::*;
use super::config::*;
use super::database::*;
use super::import_roms::{UnattendedMode, import_other};
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use num_traits::FromPrimitive;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::path::PathBuf;

// in `Merging` order, which the interactive choice relies on
const MERGING_STRATEGIES: &[&str] = &["SPLIT", "NON_MERGED", "FULL_NON_MERGED", "MERGED"];

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
) -> Result<()> {
    let systems =
        prompt_for_systems(connection, None, true, false, matches.get_flag("ALL")).await?;
    if systems.is_empty() {
        print_warning(progress_bar, "No arcade systems found");
        return Ok(());
    }

    let merging = match matches.get_one::<String>("MERGING").map(String::as_str) {
        Some("SPLIT") => Merging::Split,
        Some("NON_MERGED") => Merging::NonMerged,
        Some("FULL_NON_MERGED") => Merging::FullNonMerged,
        Some("MERGED") => Merging::Merged,
        Some(&_) | None => FromPrimitive::from_usize(select(
            MERGING_STRATEGIES,
            "Please select a merge strategy",
            None,
            None,
        )?)
        .unwrap(),
    };

    for system in systems {
        print_header(progress_bar, &format!("Processing \"{}\"", system.name));
        rebuild_system(
            connection,
            progress_bar,
            &system,
            merging,
            matches.get_flag("FORCE"),
        )
        .await?;
        print_separator(progress_bar);
    }

    Ok(())
}

async fn rebuild_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    merging: Merging,
    force: bool,
) -> Result<()> {
    start_action(progress_bar, None);

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
        if existing_rom.is_none()
            && let Some(md5) = missing_rom.md5
        {
            let mut roms = find_roms_with_romfile_by_size_and_md5_and_system_id(
                connection,
                missing_rom.size,
                &md5,
                system.id,
            )
            .await;
            existing_rom = roms.pop();
        }
        if existing_rom.is_none()
            && let Some(sha1) = missing_rom.sha1
        {
            let mut roms = find_roms_with_romfile_by_size_and_sha1_and_system_id(
                connection,
                missing_rom.size,
                &sha1,
                system.id,
            )
            .await;
            existing_rom = roms.pop();
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
    compute_system_completion(connection, progress_bar, system).await?;

    if system.merging == merging as i64 && !force {
        print_skip(progress_bar, "Nothing to do");
        return Ok(());
    }

    let games = find_full_games_by_system_id(connection, system.id).await;

    // A merged set is a split one with each clone's files in its parent's ZIP,
    // so the other strategies are reached through split.
    let mut current = Merging::from_i64(system.merging).context("Unknown merging strategy")?;
    if current == Merging::Merged && merging != Merging::Merged {
        for game in find_games_by_system_id(connection, system.id).await {
            unmerge_game(connection, progress_bar, system, &game).await?;
        }
        current = Merging::Split;
    }
    let target = match merging {
        Merging::Merged => Merging::Split,
        merging => merging,
    };

    if (current == Merging::Split || current == Merging::NonMerged)
        && (target == Merging::NonMerged || target == Merging::FullNonMerged)
    {
        let compression =
            get_archive_compression(connection, &ArchiveType::Zip, Some(system.id)).await;
        for game in games {
            expand_game(
                connection,
                progress_bar,
                system,
                &game,
                target,
                &compression,
            )
            .await?;
        }
    } else if (current == Merging::NonMerged || current == Merging::FullNonMerged)
        && (target == Merging::Split || target == Merging::NonMerged)
    {
        for game in games {
            trim_game(connection, progress_bar, system, &game, target).await?;
        }
    }

    if merging == Merging::Merged {
        for game in find_games_by_system_id(connection, system.id).await {
            merge_game(connection, progress_bar, system, &game).await?;
        }
    }

    update_system_merging(connection, system.id, merging).await;
    // completion follows the new strategy, which `system` predates
    let system = find_system_by_id(connection, system.id).await;
    compute_system_completion(connection, progress_bar, &system).await?;

    Ok(())
}

async fn expand_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
    merging: Merging,
    compression: &ArchiveCompression,
) -> Result<()> {
    print_subheader(progress_bar, &format!("Processing \"{}\"", game.name));
    let rom_directory = get_rom_directory(connection).await;
    let game_directory = get_system_directory(connection, system)
        .await?
        .join(&game.name);
    let game_archive_path = get_system_directory(connection, system)
        .await?
        .join(format!("{}.{}", game.name, ZIP_EXTENSION));
    let relative_game_archive_path = game_archive_path
        .strip_prefix(rom_directory)
        .context("Failed to retrieve relative path")?;
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
                compression,
            )
            .await?;
        } else {
            print_warning(progress_bar, &format!("Missing \"{}\"", rom.name));
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
) -> Result<()> {
    print_subheader(progress_bar, &format!("Processing \"{}\"", game.name));
    let rom_directory = get_rom_directory(connection).await;
    let romfile_path = get_system_directory(connection, system)
        .await?
        .join(format!("{}.{}", game.name, ZIP_EXTENSION));
    let relative_path = romfile_path
        .strip_prefix(rom_directory)
        .context("Failed to retrieve relative path")?;
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

#[allow(clippy::too_many_arguments)]
async fn add_rom(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
    rom: &Rom,
    source_rom: &Rom,
    destination_directory: &PathBuf,
    destination_archive_romfile: &Option<Romfile>,
    compression: &ArchiveCompression,
) -> Result<()> {
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
                    compression,
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

async fn find_game_archive(
    connection: &mut SqliteConnection,
    system: &System,
    game: &Game,
) -> Result<(PathBuf, Option<Romfile>)> {
    let rom_directory = get_rom_directory(connection).await;
    let path = get_system_directory(connection, system)
        .await?
        .join(format!("{}.{}", game.name, ZIP_EXTENSION));
    let relative_path = path
        .strip_prefix(&rom_directory)
        .context("Failed to retrieve relative path")?;
    let romfile =
        find_romfile_by_path(connection, relative_path.as_os_str().to_str().unwrap()).await;
    Ok((path, romfile))
}

async fn find_or_create_archive_romfile(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    path: &PathBuf,
    romfile: &mut Option<Romfile>,
) -> Result<i64> {
    if romfile.is_none() {
        let id = CommonRomfile::from_path(path)?
            .create(connection, progress_bar, RomfileType::Romfile)
            .await?;
        *romfile = Some(find_romfile_by_id(connection, id).await);
    }
    Ok(romfile.as_ref().unwrap().id)
}

/// Moves a clone's own ROMs from its ZIP into its parent's, as a merged set
/// files them. One its parent's ZIP already holds is not copied again, and one
/// whose name it holds with other contents goes under `<clone>/`, as
/// ClrMamePro does.
async fn merge_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
) -> Result<()> {
    let Some(parent_id) = game.parent_id else {
        return Ok(());
    };
    let parent = find_game_by_id(connection, parent_id).await;
    let (parent_path, mut parent_romfile) = find_game_archive(connection, system, &parent).await?;
    let parent_romfile_id = parent_romfile.as_ref().map(|romfile| romfile.id);
    let roms: Vec<Rom> = find_roms_by_game_id_no_parents(connection, game.id)
        .await
        .into_iter()
        // skip CHDs
        .filter(|rom| !rom.disk && rom.romfile_id.is_some() && rom.romfile_id != parent_romfile_id)
        .collect();
    if roms.is_empty() {
        return Ok(());
    }
    print_subheader(progress_bar, &format!("Processing \"{}\"", game.name));

    let mut sources: Vec<(Rom, CommonRomfile)> = vec![];
    for rom in roms {
        let source = find_romfile_by_id(connection, rom.romfile_id.unwrap())
            .await
            .as_common(connection)
            .await?;
        if source
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some(ZIP_EXTENSION)
        {
            print_warning(
                progress_bar,
                &format!("\"{}\" is not in a ZIP, convert it first", rom.name),
            );
            return Ok(());
        }
        sources.push((rom, source));
    }

    let mut entries: Vec<(String, u64, String)> = if parent_path.is_file() {
        CommonRomfile::from_path(&parent_path)?
            .parse_archive(progress_bar, None)
            .await?
    } else {
        vec![]
    };

    let mut transaction = begin_transaction(connection).await;
    for (rom, source) in &sources {
        let (source_name, size, crc) =
            rom_entries(&source.parse_archive(progress_bar, None).await?, rom)
                .into_iter()
                .next()
                .with_context(|| {
                    format!(
                        "Failed to find \"{}\" in \"{}\"",
                        rom.name,
                        source.path.display()
                    )
                })?;
        let mut destination_name = rom.name.clone();
        if entries
            .iter()
            .any(|(path, _, entry_crc)| path == &destination_name && entry_crc != &crc)
        {
            destination_name = format!("{}/{}", game.name, rom.name);
        }
        match entries
            .iter()
            .find(|(path, _, _)| path == &destination_name)
        {
            Some((_, _, entry_crc)) if entry_crc == &crc => {}
            Some(_) => bail!(
                "\"{}\" already holds another \"{}\"",
                parent_path.display(),
                destination_name
            ),
            None => {
                copy_files_between_archives(
                    progress_bar,
                    &source.path,
                    &parent_path,
                    &[&source_name],
                    &[&destination_name],
                )
                .await?;
                entries.push((destination_name, size, crc));
            }
        }
        let romfile_id = find_or_create_archive_romfile(
            &mut transaction,
            progress_bar,
            &parent_path,
            &mut parent_romfile,
        )
        .await?;
        update_rom_romfile(&mut transaction, rom.id, Some(romfile_id)).await;
    }

    let mut source_ids: Vec<i64> = sources
        .iter()
        .map(|(rom, _)| rom.romfile_id.unwrap())
        .collect();
    source_ids.sort_unstable();
    source_ids.dedup();
    for source_id in source_ids {
        prune_archive(&mut transaction, progress_bar, source_id).await?;
    }
    prune_archive(&mut transaction, progress_bar, parent_romfile.unwrap().id).await?;
    commit_transaction(transaction).await;
    Ok(())
}

/// Moves a clone's own ROMs out of its parent's ZIP into its own, undoing
/// `merge_game`.
async fn unmerge_game(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    game: &Game,
) -> Result<()> {
    let Some(parent_id) = game.parent_id else {
        return Ok(());
    };
    let parent = find_game_by_id(connection, parent_id).await;
    let (parent_path, parent_romfile) = find_game_archive(connection, system, &parent).await?;
    let Some(parent_romfile) = parent_romfile else {
        return Ok(());
    };
    let roms: Vec<Rom> = find_roms_by_game_id_no_parents(connection, game.id)
        .await
        .into_iter()
        .filter(|rom| rom.romfile_id == Some(parent_romfile.id))
        .collect();
    if roms.is_empty() {
        return Ok(());
    }
    print_subheader(progress_bar, &format!("Processing \"{}\"", game.name));

    let (game_path, mut game_romfile) = find_game_archive(connection, system, game).await?;
    let entries = CommonRomfile::from_path(&parent_path)?
        .parse_archive(progress_bar, None)
        .await?;
    let mut transaction = begin_transaction(connection).await;
    for rom in &roms {
        let (entry_name, _, _) =
            rom_entries(&entries, rom)
                .into_iter()
                .next()
                .with_context(|| {
                    format!(
                        "Failed to find \"{}\" in \"{}\"",
                        rom.name,
                        parent_path.display()
                    )
                })?;
        copy_files_between_archives(
            progress_bar,
            &parent_path,
            &game_path,
            &[&entry_name],
            &[&rom.name],
        )
        .await?;
        let romfile_id = find_or_create_archive_romfile(
            &mut transaction,
            progress_bar,
            &game_path,
            &mut game_romfile,
        )
        .await?;
        update_rom_romfile(&mut transaction, rom.id, Some(romfile_id)).await;
    }
    prune_archive(&mut transaction, progress_bar, parent_romfile.id).await?;
    prune_archive(&mut transaction, progress_bar, game_romfile.unwrap().id).await?;
    commit_transaction(transaction).await;
    Ok(())
}

/// Deletes the entries of an archive no ROM resolves to any more, and the
/// archive itself once no ROM is left in it, then records its new size.
async fn prune_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile_id: i64,
) -> Result<()> {
    let romfile = find_romfile_by_id(connection, romfile_id).await;
    let common_romfile = romfile.as_common(connection).await?;
    let roms = find_roms_by_romfile_id(connection, romfile_id).await;
    if roms.is_empty() {
        if common_romfile.path.is_file() {
            common_romfile.delete(progress_bar, false).await?;
        }
        delete_romfile_by_id(connection, romfile_id).await;
        return Ok(());
    }
    let archive_romfiles = common_romfile.as_archive(progress_bar, None).await?;
    let entries: Vec<(String, u64, String)> = archive_romfiles
        .iter()
        .map(|entry| (entry.path.clone(), entry.size, entry.crc.clone()))
        .collect();
    let used: HashSet<String> = roms
        .iter()
        .filter_map(|rom| rom_entries(&entries, rom).into_iter().next())
        .map(|(path, _, _)| path)
        .collect();
    for archive_romfile in archive_romfiles
        .iter()
        .filter(|entry| !used.contains(&entry.path))
    {
        archive_romfile.delete_file(progress_bar).await?;
    }
    common_romfile
        .update(connection, progress_bar, romfile_id)
        .await?;
    Ok(())
}

async fn delete_rom(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    archive_romfile: &Option<Romfile>,
) -> Result<()> {
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

#[cfg(test)]
pub(crate) mod merged_set;
#[cfg(test)]
mod test_full_non_merged_to_non_merged;
#[cfg(test)]
mod test_merged_to_non_merged;
#[cfg(test)]
mod test_non_merged_to_merged;
#[cfg(test)]
mod test_non_merged_to_split;
#[cfg(test)]
mod test_nothing_to_do;
#[cfg(test)]
mod test_split_to_full_non_merged;
#[cfg(test)]
mod test_split_to_merged;
#[cfg(test)]
mod test_split_to_non_merged;
