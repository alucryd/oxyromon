use super::crud::find_systems;
use super::model::{Rom, System};
use clap::ArgMatches;
use diesel::SqliteConnection;
use regex::Regex;
use std::io;
use std::str::FromStr;

pub fn prompt_for_systems(connection: &SqliteConnection, all: bool) -> Vec<System> {
    let mut systems = find_systems(&connection);
    systems.sort_by(|a, b| a.name.cmp(&b.name));

    if all {
        return systems;
    }

    println!("Please select systems (space separated):");
    for (i, system) in systems.iter().enumerate() {
        println!("[{}] {}", i, system.name);
    }
    println!("[*] All");

    let mut system_indices: Vec<usize> = vec![systems.len()];
    let mut input = String::new();
    let input_validation = Regex::new(r"(\*|[0-9 ]+)").unwrap();

    while system_indices.iter().any(|i| i >= &systems.len()) {
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            println!("Please select valid systems (space separated):");
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

pub fn prompt_for_system(connection: &SqliteConnection) -> System {
    let mut systems = find_systems(&connection);
    systems.sort_by(|a, b| a.name.cmp(&b.name));

    println!("Please select a system:");
    for (i, system) in systems.iter().enumerate() {
        println!("[{}] {}", i, system.name);
    }

    let mut system_index: usize = systems.len();
    let mut input = String::new();
    let input_validation = Regex::new(r"[0-9]+").unwrap();

    while system_index >= systems.len() {
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            println!("Please select a valid system:");
            continue;
        }

        system_index = FromStr::from_str(&input).expect("Not a valid number");
    }

    systems.remove(system_index)
}

pub fn prompt_for_rom(roms: &mut Vec<Rom>) -> Rom {
    println!("Please select a rom:");
    for (i, rom) in roms.iter().enumerate() {
        println!("[{}] {}", i, rom.name);
    }

    let mut rom_index: usize = roms.len();
    let mut input = String::new();
    let input_validation = Regex::new(r"[0-9]+").unwrap();

    while rom_index >= roms.len() {
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to get input");
        input = input.trim().to_owned();

        if !input_validation.is_match(&input) {
            println!("Please select a valid rom:");
            continue;
        }

        rom_index = FromStr::from_str(&input).expect("Not a valid number");
    }

    roms.remove(rom_index)
}

pub fn prompt_for_yes_no(matches: &ArgMatches) -> bool {
    println!("Proceed? (y|N)");
    let mut input = String::new();
    if matches.is_present("YES") {
        input = String::from("y");
    } else {
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to get input");
        input = input.trim().to_lowercase().to_owned();
    }
    input == "y"
}
