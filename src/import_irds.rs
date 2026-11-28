use super::common::to_hex;
use super::config::get_data_directory;
use super::database::*;
use super::import_dats::reimport_orphan_romfiles;
use super::import_keys::save_ps3_keys;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use clap::value_parser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use ps3_rs::Ird;
use sqlx::sqlite::SqliteConnection;
use std::path::{Path, PathBuf};
use strsim::jaro_winkler;

pub fn subcommand() -> Command {
    Command::new("import-irds")
        .about("Parse and import PlayStation 3 IRD files into oxyromon")
        .arg(
            Arg::new("IRDS")
                .help("Set the IRD files to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("INFO")
                .short('i')
                .long("info")
                .help("Show the IRD information and exit")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of already imported IRD files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select the system to import into, matching the game and ROM automatically")
                .required(false)
                .num_args(1)
                .value_parser(value_parser!(String)),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let ird_paths: Vec<&PathBuf> = matches.get_many::<PathBuf>("IRDS").unwrap().collect();
    // A system given on the command line (or by the server) skips the prompt and
    // matches the game and ROM automatically, so the whole import is unattended.
    let (system, headless) = match matches.get_one::<String>("SYSTEM") {
        Some(name) => {
            let systems = find_systems_by_name_like(connection, name).await;
            let system = systems
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("No system matching \"{}\"", name))?;
            (system, true)
        }
        None => (
            prompt_for_system_like(connection, None, "%PlayStation 3%").await?,
            false,
        ),
    };
    let mut games = find_wanted_games_by_system_id(connection, system.id).await;

    // In unattended mode there is nothing to fall back to, so a system with no
    // matching games is a hard error instead of a silent no-op.
    if headless && games.is_empty() {
        bail!(
            "No games found for system \"{}\", import its DAT first",
            system.name
        );
    }

    for ird_path in ird_paths {
        let (ird, data) = match parse_ird(ird_path).await {
            Ok(parsed) => parsed,
            Err(error) => match error.downcast_ref::<ps3_rs::Error>() {
                // an IRD of another version, which the others need not wait on
                Some(ps3_rs::Error::Unsupported(_)) => {
                    print_warning(progress_bar, &format!("{error:#}"));
                    continue;
                }
                _ => return Err(error),
            },
        };

        print_header(progress_bar, &format!("IRD: {}", ird.game_name));
        print_info(progress_bar, &format!("IRD Version: {}", ird.version));
        print_info(progress_bar, &format!("Game ID: {}", ird.game_id));
        print_info(
            progress_bar,
            &format!("Update Version: {}", ird.update_version),
        );
        print_info(progress_bar, &format!("Game Version: {}", ird.game_version));
        print_info(progress_bar, &format!("App Version: {}", ird.app_version));
        print_info(
            progress_bar,
            &format!("Regions: {}", ird.region_hashes.len()),
        );
        print_info(progress_bar, &format!("Files: {}", ird.file_hashes.len()));

        if !matches.get_flag("INFO") {
            // its disc key, for an ISO of the disc, whatever game it matches;
            // tests keep away from the data directory
            if !cfg!(test) && save_ps3_keys(&get_data_directory()?, &[ird.disc_key()])? > 0 {
                print_info(progress_bar, "Disc key kept");
            }
            games.sort_by(|a, b| {
                jaro_winkler(&b.name.to_lowercase(), &ird.game_name.to_lowercase())
                    .partial_cmp(&jaro_winkler(
                        &a.name.to_lowercase(),
                        &ird.game_name.to_lowercase(),
                    ))
                    .unwrap()
            });
            let game = if headless {
                Some(unattended_game(&games, &ird.game_name)?)
            } else {
                prompt_for_game(&games, None)?
            };
            if let Some(game) = game {
                // an IRD imported before IRDs were kept is kept now
                if game.jbfolder
                    && !matches.get_flag("FORCE")
                    && find_ird_by_game_id(connection, game.id).await.is_some()
                {
                    print_skip(progress_bar, "IRD already exists");
                    continue;
                }
                // Resolve the parent ROM the same way: the single candidate when
                // unattended, an explicit pick otherwise.
                let roms = find_roms_by_game_id_no_parents(connection, game.id).await;
                let parent_rom = if headless {
                    roms.first()
                } else {
                    prompt_for_rom(&roms, None)?
                };
                import_ird(connection, progress_bar, game, &ird, &data, parent_rom).await?;
            }
        }
        print_separator(progress_bar);
    }

    compute_system_completion(connection, progress_bar, &system).await?;

    Ok(())
}

/// The game an IRD belongs to, when there is no one to ask: the one whose name,
/// without its region and other tags, is the IRD's title. A fuzzy best match
/// would always find one, and write the IRD onto the wrong game when it is not
/// in the DAT.
fn unattended_game<'a>(games: &'a [Game], title: &str) -> Result<&'a Game> {
    let key = title_key(title);
    let matching: Vec<&Game> = games
        .iter()
        .filter(|game| title_key(&game.name) == key)
        .collect();
    match matching.as_slice() {
        [game] => Ok(game),
        [] => bail!("No game is named \"{title}\"; import the IRD from the CLI to pick one"),
        _ => bail!(
            "Several games are named \"{title}\" ({}); import the IRD from the CLI to pick one",
            matching
                .iter()
                .map(|game| game.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A title as it compares: without `(...)` and `[...]` tags, punctuation or
/// case, so that "Uncharted: Drake's Fortune" is "Uncharted - Drake's Fortune
/// (USA) (En,Fr,Es)".
fn title_key(title: &str) -> String {
    let mut depth = 0usize;
    let mut key = String::new();
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphanumeric() => key.extend(c.to_lowercase()),
            _ => {}
        }
    }
    key
}

/// An IRD and its bytes as read, which the game keeps.
pub async fn parse_ird<P: AsRef<Path>>(path: &P) -> Result<(Ird, Vec<u8>)> {
    let path = path.as_ref();
    let data = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
    let ird = Ird::parse(&data)
        .with_context(|| format!("Failed to read \"{}\" as an IRD", path.display()))?;
    Ok((ird, data))
}

pub async fn import_ird(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
    ird: &Ird,
    data: &[u8],
    parent_rom: Option<&Rom>,
) -> Result<()> {
    if parent_rom.is_none() {
        return Ok(());
    }

    let mut transaction = begin_transaction(connection).await;

    let files = ird.files()?;
    if files.len() != ird.file_hashes.len() {
        bail!(
            "Files count differ, expected {}, actual {}",
            ird.file_hashes.len(),
            files.len()
        );
    }

    // convert files into roms
    let mut orphan_romfile_ids: Vec<i64> = vec![];
    for file in files {
        let size = file.size() as i64;
        let md5 = match ird.file_hash(file.lba()) {
            Some(md5) => to_hex(&md5),
            None => bail!("No hash for \"{}\" at sector {}", file.path, file.lba()),
        };
        let rom_id = match find_rom_by_name_and_game_id(&mut transaction, &file.path, game.id).await
        {
            Some(rom) => {
                update_rom(
                    &mut transaction,
                    rom.id,
                    &file.path,
                    size,
                    &md5,
                    game.id,
                    parent_rom.as_ref().map(|rom| rom.id),
                    false,
                )
                .await;
                if (size != rom.size || Some(&md5) != rom.md5.as_ref())
                    && let Some(romfile_id) = rom.romfile_id
                {
                    orphan_romfile_ids.push(romfile_id);
                    update_rom_romfile(&mut transaction, rom.id, None).await;
                }
                rom.id
            }
            None => {
                create_rom(
                    &mut transaction,
                    &file.path,
                    size,
                    &md5,
                    game.id,
                    parent_rom.as_ref().map(|rom| rom.id),
                    false,
                )
                .await
            }
        };
        // a system update other discs share, imported already
        if file.path == ps3_rs::UPDATE_PATH
            && let Some(romfile_id) = find_child_romfile_id_by_name_and_size_and_md5(
                &mut transaction,
                &file.path,
                size,
                &md5,
                Some(game.system_id),
            )
            .await
        {
            update_rom_romfile(&mut transaction, rom_id, Some(romfile_id)).await;
        }
    }

    // mark game as jbfolder, keeping its IRD to rebuild its ISO from
    update_game_jbfolder(&mut transaction, game.id, true).await;
    create_or_replace_ird(&mut transaction, game.id, data).await;

    // reimport orphan romfiles
    if !orphan_romfile_ids.is_empty() {
        print_subheader(progress_bar, "Processing orphan romfiles");
        reimport_orphan_romfiles(
            &mut transaction,
            progress_bar,
            game.system_id,
            orphan_romfile_ids,
            &[],
        )
        .await?;
    }

    commit_transaction(transaction).await;

    Ok(())
}

#[cfg(test)]
mod tests;
