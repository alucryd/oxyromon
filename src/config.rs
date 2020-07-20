use super::crud::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use diesel::SqliteConnection;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("config")
        .about("Queries and modifies the oxyromon settings")
        .arg(
            Arg::with_name("LIST")
                .short("l")
                .long("list")
                .help("Prints the whole configuration")
                .required(false)
                .conflicts_with_all(&["GET", "SET"]),
        )
        .arg(
            Arg::with_name("GET")
                .short("g")
                .long("get")
                .help("Prints a single setting")
                .required(false)
                .takes_value(true)
                .value_name("KEY"),
        )
        .arg(
            Arg::with_name("SET")
                .short("s")
                .long("set")
                .help("Configures a single setting")
                .required(false)
                .takes_value(true)
                .multiple(true)
                .number_of_values(2)
                .value_names(&["KEY", "VALUE"]),
        )
        .arg(
            Arg::with_name("DELETE")
                .short("d")
                .long("delete")
                .help("Deletes a single setting")
                .required(false)
                .takes_value(true)
                .value_name("KEY"),
        )
}

pub fn main(connection: &SqliteConnection, matches: &ArgMatches) -> SimpleResult<()> {
    if matches.is_present("LIST") {
        let settings = find_settings(connection);
        for setting in settings {
            println!("{} = {}", setting.key, setting.value.unwrap_or_default());
        }
    };

    if matches.is_present("GET") {
        let key = matches.value_of("GET").unwrap();
        let setting = find_setting_by_key(connection, key).unwrap();
        println!("{} = {}", setting.key, setting.value.unwrap_or_default());
    }

    if matches.is_present("SET") {
        let mut key_value: Vec<&str> = matches.values_of("SET").unwrap().collect();
        let key = key_value.remove(0);
        let value = key_value.remove(0);

        if key.ends_with("_DIRECTORY") {
            let p = get_canonicalized_path(value)?;
            create_directory(&p)?;
            set_directory(connection, key, &p);
        } else if key.starts_with("DISCARD_") {
            let b: bool = try_with!(FromStr::from_str(value), "Failed to parse bool");
            set_bool(connection, key, b);
        } else {
            set_str(connection, key, value);
        }
    }

    if matches.is_present("DELETE") {
        let key = matches.value_of("DELETE").unwrap();
        delete_setting_by_key(connection, key);
    }

    Ok(())
}

pub fn set_str(connection: &SqliteConnection, key: &str, value: &str) {
    let setting = find_setting_by_key(connection, key);
    match setting {
        Some(setting) => update_setting(connection, &setting, value),
        None => create_setting(connection, key, value),
    };
}

pub fn set_bool(connection: &SqliteConnection, key: &str, value: bool) {
    let setting = find_setting_by_key(connection, key);
    match setting {
        Some(setting) => update_setting(connection, &setting, &format!("{}", value)),
        None => create_setting(connection, key, &format!("{}", value)),
    };
}

pub fn get_bool(connection: &SqliteConnection, key: &str) -> bool {
    FromStr::from_str(&find_setting_by_key(connection, key).unwrap().value.unwrap()).unwrap()
}

pub fn set_directory(connection: &SqliteConnection, key: &str, value: &PathBuf) {
    let setting = find_setting_by_key(connection, key);
    match setting {
        Some(setting) => update_setting(connection, &setting, value.as_os_str().to_str().unwrap()),
        None => create_setting(connection, key, value.as_os_str().to_str().unwrap()),
    };
}

pub fn get_directory(connection: &SqliteConnection, key: &str) -> Option<PathBuf> {
    find_setting_by_key(&connection, key)
        .map(|p| get_canonicalized_path(&p.value.unwrap()).unwrap())
}

pub fn get_rom_directory(connection: &SqliteConnection) -> PathBuf {
    let rom_directory = get_directory(&connection, "ROM_DIRECTORY");
    match rom_directory {
        Some(rom_directory) => rom_directory,
        None => {
            let d = dirs::home_dir().unwrap().join("Emulation");
            set_directory(connection, "ROM_DIRECTORY", &d);
            d
        }
    }
}

pub fn get_tmp_directory(connection: &SqliteConnection) -> PathBuf {
    let tmp_directory = get_directory(&connection, "TMP_DIRECTORY");
    match tmp_directory {
        Some(tmp_directory) => tmp_directory,
        None => {
            let d = env::temp_dir();
            set_directory(connection, "TMP_DIRECTORY", &d);
            d
        }
    }
}
