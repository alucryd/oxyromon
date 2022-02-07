use super::database::*;
use super::model::*;
use dialoguer::{Confirm, MultiSelect, Select};
use simple_error::SimpleResult;
use sqlx::sqlite::SqliteConnection;

pub async fn prompt_for_systems(
    connection: &mut SqliteConnection,
    url: Option<&str>,
    arcade_only: bool,
    all: bool,
) -> SimpleResult<Vec<System>> {
    let systems = if arcade_only {
        find_arcade_systems(connection).await
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
            .map(|system| system.name.as_str())
            .collect::<Vec<&str>>(),
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
                    .map(|system| system.name.as_str())
                    .collect::<Vec<&str>>(),
                default,
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
            .map(|game| game.name.as_str())
            .collect::<Vec<&str>>(),
        None,
    )?;
    Ok(games
        .into_iter()
        .enumerate()
        .filter(|(i, _)| indices.contains(i))
        .map(|(_, game)| game)
        .collect())
}

pub fn prompt_for_rom(roms_games: &mut Vec<(Rom, Game)>) -> SimpleResult<Option<Rom>> {
    let mut items: Vec<String> = roms_games
        .iter()
        .map(|(rom, game)| format!("{} ({})", &rom.name, &game.name))
        .collect();
    items.push(String::from("None of the above"));
    let index = select(
        &items.iter().map(|item| &**item).collect::<Vec<&str>>(),
        None,
    )?;
    if index >= roms_games.len() {
        return Ok(None);
    }
    Ok(Some(roms_games.remove(index).0))
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

pub fn select(items: &[&str], default: Option<usize>) -> SimpleResult<usize> {
    let mut select = Select::new();
    select.items(items);
    if let Some(default) = default {
        select.default(default);
    }
    Ok(try_with!(select.interact(), "Failed to get user input"))
}

pub fn multiselect(items: &[&str], defaults: Option<&[bool]>) -> SimpleResult<Vec<usize>> {
    let mut multiselect = MultiSelect::new();
    multiselect.items(items);
    if let Some(defaults) = defaults {
        multiselect.defaults(defaults);
    }
    Ok(try_with!(
        multiselect.interact(),
        "Failed to get user input"
    ))
}
