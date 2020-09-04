use super::database::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::PathBuf;
use clap::{App, Arg, ArgMatches, SubCommand};
use once_cell::sync::OnceCell;
use sqlx::SqliteConnection;
use std::str::FromStr;

cfg_if::cfg_if! {
    if #[cfg(test)] {
        use async_std::sync::Mutex;

        static mut ROM_DIRECTORY: Option<PathBuf> = None;
        static mut TMP_DIRECTORY: Option<PathBuf> = None;
        pub static MUTEX: OnceCell<Mutex<i32>> = OnceCell::new();
    } else {
        use std::env;

        static ROM_DIRECTORY: OnceCell<PathBuf> = OnceCell::new();
        static TMP_DIRECTORY: OnceCell<PathBuf> = OnceCell::new();
    }
}

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

pub async fn main(connection: &mut SqliteConnection, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    // make sure rom and tmp directories are initialized
    get_rom_directory(connection).await;
    get_tmp_directory(connection).await;

    if matches.is_present("LIST") {
        let settings = find_settings(connection).await;
        for setting in settings {
            println!("{} = {}", setting.key, setting.value.unwrap_or_default());
        }
    };

    if matches.is_present("GET") {
        let key = matches.value_of("GET").unwrap();
        let setting = find_setting_by_key(connection, key).await.unwrap();
        println!("{} = {}", setting.key, setting.value.unwrap_or_default());
    }

    if matches.is_present("SET") {
        let mut key_value: Vec<&str> = matches.values_of("SET").unwrap().collect();
        let key = key_value.remove(0);
        let value = key_value.remove(0);

        if key.ends_with("_DIRECTORY") {
            let p = get_canonicalized_path(value).await?;
            create_directory(&p).await?;
            set_directory(connection, key, &p).await;
        } else if key.starts_with("DISCARD_") {
            let b: bool = try_with!(FromStr::from_str(value), "Failed to parse bool");
            set_bool(connection, key, b).await;
        } else {
            set_str(connection, key, value).await;
        }
    }

    if matches.is_present("DELETE") {
        let key = matches.value_of("DELETE").unwrap();
        delete_setting_by_key(connection, key).await;
    }

    Ok(())
}

pub async fn set_str(connection: &mut SqliteConnection, key: &str, value: &str) {
    let setting = find_setting_by_key(connection, key).await;
    match setting {
        Some(setting) => update_setting(connection, setting.id, value).await,
        None => create_setting(connection, key, value).await,
    };
}

pub async fn set_bool(connection: &mut SqliteConnection, key: &str, value: bool) {
    let setting = find_setting_by_key(connection, key).await;
    match setting {
        Some(setting) => update_setting(connection, setting.id, &format!("{}", value)).await,
        None => create_setting(connection, key, &format!("{}", value)).await,
    };
}

pub async fn get_bool(connection: &mut SqliteConnection, key: &str) -> bool {
    FromStr::from_str(
        &find_setting_by_key(connection, key)
            .await
            .unwrap()
            .value
            .unwrap(),
    )
    .unwrap()
}

pub async fn set_directory(connection: &mut SqliteConnection, key: &str, value: &PathBuf) {
    let setting = find_setting_by_key(connection, key).await;
    match setting {
        Some(setting) => {
            update_setting(connection, setting.id, value.as_os_str().to_str().unwrap()).await
        }
        None => create_setting(connection, key, value.as_os_str().to_str().unwrap()).await,
    };
}

pub async fn get_directory(connection: &mut SqliteConnection, key: &str) -> Option<PathBuf> {
    match find_setting_by_key(connection, &key).await {
        Some(p) => Some(get_canonicalized_path(&p.value.unwrap()).await.unwrap()),
        None => None,
    }
}

cfg_if::cfg_if! {
    if #[cfg(test)] {
        pub fn set_rom_directory(rom_directory: PathBuf) -> &'static PathBuf {
            unsafe {
                ROM_DIRECTORY.replace(rom_directory);
                ROM_DIRECTORY.as_ref().unwrap()
            }
        }

        pub async fn get_rom_directory(_connection: &mut SqliteConnection) -> &'static PathBuf {
            unsafe {
                ROM_DIRECTORY.as_ref().unwrap()
            }
        }

        pub fn set_tmp_directory(tmp_directory: PathBuf) -> &'static PathBuf {
            unsafe {
                TMP_DIRECTORY.replace(tmp_directory);
                TMP_DIRECTORY.as_ref().unwrap()
            }
        }

        pub async fn get_tmp_directory(_connection: &mut SqliteConnection) -> &'static PathBuf {
            unsafe {
                TMP_DIRECTORY.as_ref().unwrap()
            }
        }
    } else {
        pub async fn get_rom_directory(connection: &mut SqliteConnection) -> &PathBuf {
            match ROM_DIRECTORY.get() {
                Some(rom_directory) => rom_directory,
                None => {
                    let rom_directory = get_directory(connection, "ROM_DIRECTORY").await;
                    let rom_directory = match rom_directory {
                        Some(rom_directory) => rom_directory,
                        None => {
                            let rom_directory = PathBuf::from(dirs::home_dir().unwrap()).join("Emulation");
                            set_directory(connection, "ROM_DIRECTORY", &rom_directory).await;
                            rom_directory
                        }
                    };
                    ROM_DIRECTORY
                        .set(rom_directory)
                        .expect("Failed to set rom directory");
                    ROM_DIRECTORY.get().unwrap()
                }
            }
        }

        pub async fn get_tmp_directory(connection: &mut SqliteConnection) -> &PathBuf {
            match TMP_DIRECTORY.get() {
                Some(tmp_directory) => tmp_directory,
                None => {
                    let tmp_directory = get_directory(connection, "TMP_DIRECTORY").await;
                    let tmp_directory = match tmp_directory {
                        Some(tmp_directory) => tmp_directory,
                        None => {
                            let tmp_directory = PathBuf::from(env::temp_dir());
                            set_directory(connection, "TMP_DIRECTORY", &tmp_directory).await;
                            tmp_directory
                        }
                    };
                    TMP_DIRECTORY
                        .set(tmp_directory)
                        .expect("Failed to set tmp directory");
                    TMP_DIRECTORY.get().unwrap()
                }
            }
        }
    }
}
