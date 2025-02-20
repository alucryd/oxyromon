use super::database::*;
use super::model::*;
use dialoguer::{Confirm, Editor, FuzzySelect, MultiSelect};
use simple_error::SimpleResult;
use sqlx::sqlite::SqliteConnection;
use std::path::PathBuf;
use strsim::jaro_winkler;

pub async fn prompt_for_systems(
    connection: &mut SqliteConnection,
    url: Option<&str>,
    arcade_only: bool,
    empty_only: bool,
    all: bool,
) -> SimpleResult<Vec<System>> {
    let systems = if arcade_only {
        find_arcade_systems(connection).await
    } else if empty_only {
        find_empty_systems(connection).await
    } else {
        match url {
            Some(url) => find_systems_by_url(connection, url).await,
            None => find_systems(connection).await,
        }
    };

    if all || systems.is_empty() {
        return Ok(systems);
    }

    let indices = multiselect(
        &systems
            .iter()
            .map(|system| &system.name)
            .collect::<Vec<&String>>(),
        "Please select systems",
        None,
        None,
    )?;
    Ok(systems
        .into_iter()
        .enumerate()
        .filter(|(i, _)| indices.contains(i))
        .map(|(_, system)| system)
        .collect())
}

pub async fn prompt_for_system(
    connection: &mut SqliteConnection,
    default: Option<usize>,
) -> SimpleResult<System> {
    let mut systems = find_systems(connection).await;
    match systems.len() {
        0 => bail!("No available system"),
        1 => Ok(systems.remove(0)),
        _ => {
            let index = select(
                &systems
                    .iter()
                    .map(|system| &system.name)
                    .collect::<Vec<&String>>(),
                "Please select a system",
                default,
                None,
            )?;
            Ok(systems.remove(index))
        }
    }
}

pub async fn prompt_for_system_like(
    connection: &mut SqliteConnection,
    default: Option<usize>,
    name: &str,
) -> SimpleResult<System> {
    let mut systems = find_systems_by_name_like(connection, name).await;
    match systems.len() {
        0 => bail!("No available system"),
        1 => Ok(systems.remove(0)),
        _ => {
            let index = select(
                &systems
                    .iter()
                    .map(|system| &system.name)
                    .collect::<Vec<&String>>(),
                "Please select a system",
                default,
                None,
            )?;
            Ok(systems.remove(index))
        }
    }
}

pub fn prompt_for_games(games: Vec<Game>, all: bool) -> SimpleResult<Vec<Game>> {
    if all || games.is_empty() {
        return Ok(games);
    }

    let indices = multiselect(
        &games
            .iter()
            .map(|game| &game.name)
            .collect::<Vec<&String>>(),
        "Please select games",
        None,
        Some(10),
    )?;
    Ok(games
        .into_iter()
        .enumerate()
        .filter(|(i, _)| indices.contains(i))
        .map(|(_, game)| game)
        .collect())
}

pub fn prompt_for_game(games: &[Game], default: Option<usize>) -> SimpleResult<Option<&Game>> {
    match games.len() {
        0 => bail!("No available rom"),
        1 => Ok(games.first()),
        _ => {
            let index = select_opt(
                &games
                    .iter()
                    .map(|game| &game.name)
                    .collect::<Vec<&String>>(),
                "Please select a game",
                default,
                Some(10),
            )?;
            Ok(index.map(|i| games.get(i).unwrap()))
        }
    }
}

pub fn prompt_for_rom(roms: &[Rom], default: Option<usize>) -> SimpleResult<Option<&Rom>> {
    match roms.len() {
        0 => bail!("No available rom"),
        1 => Ok(roms.first()),
        _ => {
            let index = select_opt(
                &roms.iter().map(|rom| &rom.name).collect::<Vec<&String>>(),
                "Please select a ROM",
                default,
                Some(10),
            )?;
            Ok(index.map(|i| roms.get(i).unwrap()))
        }
    }
}

pub fn prompt_for_rom_game(roms_games: &mut Vec<(Rom, Game)>) -> SimpleResult<Option<(Rom, Game)>> {
    let mut items = roms_games
        .iter()
        .map(|(rom, game)| format!("{} ({})", &rom.name, &game.name))
        .collect::<Vec<String>>();
    items.insert(0, String::from("None"));
    let index = select_opt(&items, "Please select a ROM", Some(0), Some(10))?;
    Ok(match index {
        Some(0) => None,
        Some(_) => index.map(|i| roms_games.remove(i - 1)),
        None => None,
    })
}

pub fn prompt_for_rom_game_system(
    roms_games_systems: &mut Vec<(Rom, Game, System)>,
) -> SimpleResult<Option<(Rom, Game, System)>> {
    let mut items = roms_games_systems
        .iter()
        .map(|(rom, game, system)| format!("{} ({}) [{}]", &rom.name, &game.name, &system.name))
        .collect::<Vec<String>>();
    items.insert(0, String::from("None"));
    let index = select_opt(&items, "Please select a ROM", Some(0), Some(10))?;
    Ok(match index {
        Some(0) => None,
        Some(_) => index.map(|i| roms_games_systems.remove(i - 1)),
        None => None,
    })
}

pub async fn prompt_for_parent_romfile(
    connection: &mut SqliteConnection,
    game: &Game,
    extension: &str,
) -> SimpleResult<Option<Romfile>> {
    let mut romfiles = find_romfiles_by_system_id_and_extension_and_no_parent_id(
        connection,
        game.system_id,
        extension,
    )
    .await;
    romfiles.sort_by(|a, b| {
        jaro_winkler(&b.path.to_lowercase(), &game.name.to_lowercase())
            .partial_cmp(&jaro_winkler(
                &a.path.to_lowercase(),
                &game.name.to_lowercase(),
            ))
            .unwrap()
    });
    let index = match romfiles.len() {
        0 => None,
        _ => select_opt(
            &romfiles
                .iter()
                .map(|romfile| {
                    PathBuf::from(&romfile.path)
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string()
                })
                .collect::<Vec<String>>(),
            "Please select a ROM file",
            None,
            None,
        )?,
    };
    Ok(index.map(|index| romfiles.remove(index)))
}

pub fn prompt_for_name(prompt: &str) -> SimpleResult<Option<String>> {
    editor(prompt)
}

pub fn confirm(default: bool) -> SimpleResult<bool> {
    Ok(try_with!(
        Confirm::new()
            .with_prompt("Proceed?")
            .default(default)
            .interact(),
        "Failed to get user input"
    ))
}

pub fn editor(prompt: &str) -> SimpleResult<Option<String>> {
    Ok(try_with!(
        Editor::new().edit(prompt),
        "Failed to get user input"
    ))
}

pub fn select<T: ToString>(
    items: &[T],
    prompt: &str,
    default: Option<usize>,
    max_length: Option<usize>,
) -> SimpleResult<usize> {
    let mut select = FuzzySelect::new();
    select = select.items(items).with_prompt(prompt);
    if let Some(default) = default {
        select = select.default(default);
    }
    if let Some(max_length) = max_length {
        select = select.max_length(max_length);
    }
    Ok(try_with!(select.interact(), "Failed to get user input"))
}

pub fn select_opt<T: ToString>(
    items: &[T],
    prompt: &str,
    default: Option<usize>,
    max_length: Option<usize>,
) -> SimpleResult<Option<usize>> {
    let mut select = FuzzySelect::new();
    select = select.items(items).with_prompt(prompt);
    if let Some(default) = default {
        select = select.default(default);
    }
    if let Some(max_length) = max_length {
        select = select.max_length(max_length);
    }
    Ok(try_with!(select.interact_opt(), "Failed to get user input"))
}

pub fn multiselect<T: ToString>(
    items: &[T],
    prompt: &str,
    defaults: Option<&[bool]>,
    max_length: Option<usize>,
) -> SimpleResult<Vec<usize>> {
    let mut multiselect = MultiSelect::new();
    multiselect = multiselect.items(items).with_prompt(prompt);
    if let Some(defaults) = defaults {
        multiselect = multiselect.defaults(defaults);
    }
    if let Some(max_length) = max_length {
        multiselect = multiselect.max_length(max_length);
    }
    Ok(try_with!(
        multiselect.interact(),
        "Failed to get user input"
    ))
}
