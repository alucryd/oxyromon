use super::database::*;
use super::model::*;
use async_std::io;
use clap::ArgMatches;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::str::FromStr;

pub async fn prompt_for_systems(
    connection: &mut SqliteConnection,
    all: bool,
    progress_bar: &ProgressBar,
) -> Vec<System> {
    let mut systems = find_systems(connection).await;
    systems.sort_by(|a, b| a.name.cmp(&b.name));

    if all {
        return systems;
    }

    progress_bar.println("Please select systems (space separated):");
    for (i, system) in systems.iter().enumerate() {
        progress_bar.println(format!("[{}] {}", i, system.name));
    }
    progress_bar.println("[*] All");

    let mut system_indices: Vec<usize> = vec![systems.len()];
    let mut input = String::new();
    let input_validation = Regex::new(r"(\*|[0-9 ]+)").unwrap();

    while system_indices.iter().any(|i| i >= &systems.len()) {
        io::stdin()
            .read_line(&mut input)
            .await
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            progress_bar.println("Please select valid systems (space separated):");
            continue;
        }

        if input == "*" {
            return systems;
        }

        system_indices = input
            .split(' ')
            .map(|i| FromStr::from_str(i).unwrap())
            .collect();
    }

    systems
        .into_iter()
        .enumerate()
        .filter(|(i, _)| system_indices.contains(i))
        .map(|(_, system)| system)
        .collect()
}

pub async fn prompt_for_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> System {
    let mut systems = find_systems(connection).await;
    systems.sort_by(|a, b| a.name.cmp(&b.name));

    progress_bar.println("Please select a system:");
    for (i, system) in systems.iter().enumerate() {
        progress_bar.println(format!("[{}] {}", i, system.name));
    }

    let mut system_index: usize = systems.len();
    let mut input = String::new();
    let input_validation = Regex::new(r"[0-9]+").unwrap();

    while system_index >= systems.len() {
        io::stdin()
            .read_line(&mut input)
            .await
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            progress_bar.println("Please select a valid system:");
            continue;
        }

        system_index = FromStr::from_str(&input).expect("Not a valid number");
    }

    systems.remove(system_index)
}

pub async fn prompt_for_roms(roms: Vec<Rom>, all: bool, progress_bar: &ProgressBar) -> Vec<Rom> {
    progress_bar.println("Please select ROMs (space separated):");
    for (i, rom) in roms.iter().enumerate() {
        progress_bar.println(format!("[{}] {}", i, rom.name));
    }

    if all {
        return roms;
    }

    let mut rom_indices: Vec<usize> = vec![roms.len()];
    let mut input = String::new();
    let input_validation = Regex::new(r"(\*|[0-9 ]+)").unwrap();

    while rom_indices.iter().any(|i| i >= &roms.len()) {
        io::stdin()
            .read_line(&mut input)
            .await
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            progress_bar.println("Please select valid ROMs (space separated):");
            continue;
        }

        if input == "*" {
            return roms;
        }

        rom_indices = input
            .split(' ')
            .map(|i| FromStr::from_str(i).unwrap())
            .collect();
    }

    roms.into_iter()
        .enumerate()
        .filter(|(i, _)| rom_indices.contains(i))
        .map(|(_, rom)| rom)
        .collect()
}

pub async fn prompt_for_rom(roms: &mut Vec<Rom>, progress_bar: &ProgressBar) -> Rom {
    progress_bar.println("Please select a rom:");
    for (i, rom) in roms.iter().enumerate() {
        progress_bar.println(format!("[{}] {}", i, rom.name));
    }

    let mut rom_index: usize = roms.len();
    let mut input = String::new();
    let input_validation = Regex::new(r"[0-9]+").unwrap();

    while rom_index >= roms.len() {
        io::stdin()
            .read_line(&mut input)
            .await
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            progress_bar.println("Please select a valid rom:");
            continue;
        }

        rom_index = FromStr::from_str(&input).expect("Not a valid number");
    }

    roms.remove(rom_index)
}

pub async fn prompt_for_yes_no(matches: &ArgMatches<'_>, progress_bar: &ProgressBar) -> bool {
    progress_bar.println("Proceed? (y|N)");
    let mut input = String::new();
    if matches.is_present("YES") {
        input = String::from("y");
    } else {
        io::stdin()
            .read_line(&mut input)
            .await
            .expect("Failed to get input");
        input = input.trim().to_lowercase();
    }
    input == "y"
}
