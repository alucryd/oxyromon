use super::crud::*;
use super::SimpleResult;
use super::util::*;
use clap::ArgMatches;
use diesel::SqliteConnection;
use std::path::Path;

pub fn config(connection: &SqliteConnection, matches: &ArgMatches) -> SimpleResult<()> {
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
            create_directory(&Path::new(value).to_path_buf())?;
        }

        let setting = find_setting_by_key(connection, key);
        match setting {
            Some(setting) => update_setting(connection, &setting, value),
            None => create_setting(connection, key, value),
        }
    }

    if matches.is_present("DELETE") {
        let key = matches.value_of("DELETE").unwrap();
        delete_setting_by_key(connection, key);
    }

    Ok(())
}
