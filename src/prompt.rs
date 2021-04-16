use super::database::*;
use super::model::*;
use dialoguer::{Confirm, MultiSelect, Select};
use simple_error::SimpleResult;
use sqlx::SqliteConnection;

pub async fn prompt_for_systems(
    connection: &mut SqliteConnection,
    all: bool,
) -> SimpleResult<Vec<System>> {
    let systems = find_systems(connection).await;
    if all {
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

pub fn prompt_for_roms(roms: Vec<Rom>, all: bool) -> SimpleResult<Vec<Rom>> {
    if all {
        return Ok(roms);
    }
    let indices = multiselect(
        &roms
            .iter()
            .map(|rom| rom.name.as_str())
            .collect::<Vec<&str>>(),
            None,
    )?;
    Ok(roms
        .into_iter()
        .enumerate()
        .filter(|(i, _)| indices.contains(i))
        .map(|(_, rom)| rom)
        .collect())
}

pub fn prompt_for_rom(roms: &mut Vec<Rom>) -> SimpleResult<Rom> {
    let index = select(
        &roms
            .iter()
            .map(|rom| rom.name.as_str())
            .collect::<Vec<&str>>(),
        None,
    )?;
    Ok(roms.remove(index))
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
    select.paged(true).items(&items);
    if let Some(default) = default {
        select.default(default);
    }
    Ok(try_with!(select.interact(), "Failed to get user input"))
}

pub fn multiselect(items: &[&str], defaults: Option<&[bool]>) -> SimpleResult<Vec<usize>> {
    let mut multiselect = MultiSelect::new();
    multiselect.paged(true).items(&items);
    if let Some(defaults) = defaults {
        multiselect.defaults(defaults);
    }
    Ok(try_with!(
        multiselect.interact(),
        "Failed to get user input"
    ))
}
