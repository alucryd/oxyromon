use super::database::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use cfg_if::cfg_if;
use clap::{App, Arg, ArgMatches, SubCommand};
use sqlx::sqlite::SqliteConnection;
use std::str::FromStr;

cfg_if! {
    if #[cfg(test)] {
        use async_std::sync::Mutex;

        static mut ROM_DIRECTORY: Option<PathBuf> = None;
        static mut TMP_DIRECTORY: Option<PathBuf> = None;

        lazy_static! {
            pub static ref MUTEX: Mutex<i32> = Mutex::new(0);
        }
    } else {
        use once_cell::sync::OnceCell;
        use std::env;

        static ROM_DIRECTORY: OnceCell<PathBuf> = OnceCell::new();
        static TMP_DIRECTORY: OnceCell<PathBuf> = OnceCell::new();
    }
}

const BOOLEANS: &[&str] = &[];
const LISTS: &[&str] = &[
    "DISCARD_FLAGS",
    "DISCARD_RELEASES",
    "REGIONS_ALL",
    "REGIONS_ONE",
];
const PATHS: &[&str] = &["ROM_DIRECTORY", "TMP_DIRECTORY"];

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
            Arg::with_name("ADD")
                .short("a")
                .long("add")
                .help("Adds an entry to a list")
                .required(false)
                .takes_value(true)
                .multiple(true)
                .number_of_values(2)
                .value_names(&["KEY", "VALUE"]),
        )
        .arg(
            Arg::with_name("REMOVE")
                .short("r")
                .long("remove")
                .help("Removes an entry from a list")
                .required(false)
                .takes_value(true)
                .multiple(true)
                .number_of_values(2)
                .value_names(&["KEY", "VALUE"]),
        )
}

pub async fn main(connection: &mut SqliteConnection, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    // make sure rom and tmp directories are initialized
    get_rom_directory(connection).await;
    get_tmp_directory(connection).await;

    if matches.is_present("LIST") {
        list_settings(connection).await;
    };

    if matches.is_present("GET") {
        get_setting(connection, matches.value_of("GET").unwrap()).await;
    }

    if matches.is_present("SET") {
        let key_value: Vec<&str> = matches.values_of("SET").unwrap().collect();
        set_setting(
            connection,
            key_value.get(0).unwrap(),
            key_value.get(1).unwrap(),
        )
        .await?;
    }

    if matches.is_present("ADD") {
        let key_value: Vec<&str> = matches.values_of("ADD").unwrap().collect();
        add_to_list(
            connection,
            key_value.get(0).unwrap(),
            key_value.get(1).unwrap(),
        )
        .await;
    }

    if matches.is_present("REMOVE") {
        let key_value: Vec<&str> = matches.values_of("REMOVE").unwrap().collect();
        remove_from_list(
            connection,
            key_value.get(0).unwrap(),
            key_value.get(1).unwrap(),
        )
        .await;
    }

    Ok(())
}

async fn list_settings(connection: &mut SqliteConnection) {
    for setting in find_settings(connection).await {
        println!("{} = {}", setting.key, setting.value.unwrap_or_default());
    }
}

async fn get_setting(connection: &mut SqliteConnection, key: &str) {
    let setting = find_setting_by_key(connection, key).await.unwrap();
    println!("{} = {}", setting.key, setting.value.unwrap_or_default());
}

async fn set_setting(
    connection: &mut SqliteConnection,
    key: &str,
    value: &str,
) -> SimpleResult<()> {
    if PATHS.contains(&key) {
        let p = get_canonicalized_path(value).await?;
        create_directory(&p).await?;
        set_directory(connection, key, &p).await;
    } else if BOOLEANS.contains(&key) {
        let b: bool = try_with!(FromStr::from_str(value), "Failed to parse bool");
        set_bool(connection, key, b).await;
    } else if LISTS.contains(&key) {
        println!("Lists can't be set directly, please use ADD or REMOVE instead");
    } else {
        println!("Unsupported setting");
    }
    Ok(())
}

async fn get_bool(connection: &mut SqliteConnection, key: &str) -> bool {
    FromStr::from_str(
        &find_setting_by_key(connection, key)
            .await
            .unwrap()
            .value
            .unwrap(),
    )
    .unwrap()
}

async fn set_bool(connection: &mut SqliteConnection, key: &str, value: bool) {
    let setting = find_setting_by_key(connection, key).await;
    let value = value.to_string();
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value)).await,
        None => create_setting(connection, key, Some(value)).await,
    };
}

pub async fn get_list(connection: &mut SqliteConnection, key: &str) -> Vec<String> {
    match find_setting_by_key(connection, key).await {
        Some(setting) => match setting.value {
            Some(value) => value.split(',').map(|s| s.to_owned()).collect(),
            None => Vec::new(),
        },
        None => Vec::new(),
    }
}

pub async fn add_to_list(connection: &mut SqliteConnection, key: &str, value: &str) {
    if LISTS.contains(&key) {
        let mut list = get_list(connection, key).await;
        if !list.contains(&String::from(value)) {
            list.push(value.to_owned());
            set_list(connection, key, &list).await;
        } else {
            println!("Value already in list");
        }
    } else {
        println!("Only list settings are supported");
    }
}

pub async fn remove_from_list(connection: &mut SqliteConnection, key: &str, value: &str) {
    if LISTS.contains(&key) {
        let mut list = get_list(connection, key).await;
        if list.contains(&String::from(value)) {
            list.remove(list.iter().position(|v| v == value).unwrap());
            set_list(connection, key, &list).await;
        } else {
            println!("Value not in list");
        }
    } else {
        println!("Only list settings are supported");
    }
}

async fn set_list(connection: &mut SqliteConnection, key: &str, value: &[String]) {
    let setting = find_setting_by_key(connection, key).await;
    let value = if value.is_empty() {
        None
    } else {
        Some(value.join(","))
    };
    match setting {
        Some(setting) => update_setting(connection, setting.id, value).await,
        None => create_setting(connection, key, value).await,
    };
}

pub async fn get_directory(connection: &mut SqliteConnection, key: &str) -> Option<PathBuf> {
    match find_setting_by_key(connection, key).await {
        Some(p) => match get_canonicalized_path(&p.value.unwrap()).await {
            Ok(path) => Some(path),
            Err(_) => None,
        },
        None => None,
    }
}

pub async fn set_directory<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    key: &str,
    value: &P,
) {
    let setting = find_setting_by_key(connection, key).await;
    let value = value.as_ref().as_os_str().to_str().unwrap().to_owned();
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value)).await,
        None => create_setting(connection, key, Some(value)).await,
    };
}

cfg_if::cfg_if! {
    if #[cfg(test)] {
        pub async fn get_rom_directory(_: &mut SqliteConnection) -> &'static PathBuf {
            unsafe {
                ROM_DIRECTORY.as_ref().unwrap()
            }
        }

        pub fn set_rom_directory(rom_directory: PathBuf) -> &'static PathBuf {
            unsafe {
                ROM_DIRECTORY.replace(rom_directory);
                ROM_DIRECTORY.as_ref().unwrap()
            }
        }

        pub async fn get_tmp_directory(_: &mut SqliteConnection) -> &'static PathBuf {
            unsafe {
                TMP_DIRECTORY.as_ref().unwrap()
            }
        }

        pub fn set_tmp_directory(tmp_directory: PathBuf) -> &'static PathBuf {
            unsafe {
                TMP_DIRECTORY.replace(tmp_directory);
                TMP_DIRECTORY.as_ref().unwrap()
            }
        }
    } else {
        pub async fn get_rom_directory(connection: &mut SqliteConnection) -> &'static PathBuf {
            match ROM_DIRECTORY.get() {
                Some(rom_directory) => rom_directory,
                None => {
                    let rom_directory = match get_directory(connection, "ROM_DIRECTORY").await {
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

        pub async fn get_tmp_directory(connection: &mut SqliteConnection) -> &'static PathBuf {
            match TMP_DIRECTORY.get() {
                Some(tmp_directory) => tmp_directory,
                None => {
                    let tmp_directory = match get_directory(connection, "TMP_DIRECTORY").await {
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

#[cfg(test)]
mod test {
    use super::*;
    use async_std::fs;
    use async_std::path::{Path, PathBuf};
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_bool() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "TEST_BOOLEAN";

        // when
        set_bool(&mut connection, key, true).await;
        let bool = get_bool(&mut connection, key).await;

        // then
        assert_eq!(bool, true);
    }

    #[async_std::test]
    async fn test_list() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "DISCARD_FLAGS";

        // when
        set_list(
            &mut connection,
            key,
            &[String::from("item1"), String::from("item2")],
        )
        .await;

        let list = get_list(&mut connection, key).await;

        // then
        assert_eq!(list.len(), 2);
        assert_eq!(list.get(0).unwrap(), "item1");
        assert_eq!(list.get(1).unwrap(), "item2");
    }

    #[async_std::test]
    async fn test_add_to_list() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "DISCARD_FLAGS";

        set_list(&mut connection, key, &[String::from("item1")]).await;

        // when
        add_to_list(&mut connection, key, "item2").await;
        let list = get_list(&mut connection, key).await;

        // then
        assert_eq!(list.len(), 2);
        assert_eq!(list.get(0).unwrap(), "item1");
        assert_eq!(list.get(1).unwrap(), "item2");
    }

    #[async_std::test]
    async fn test_add_to_list_already_exists() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "DISCARD_FLAGS";

        set_list(&mut connection, key, &[String::from("item1")]).await;

        // when
        add_to_list(&mut connection, key, "item1").await;
        let list = get_list(&mut connection, key).await;

        // then
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap(), "item1");
    }

    #[async_std::test]
    async fn test_remove_from_list() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "DISCARD_FLAGS";

        set_list(&mut connection, key, &[String::from("item1")]).await;

        // when
        remove_from_list(&mut connection, key, "item1").await;
        let list = get_list(&mut connection, key).await;

        // then
        assert_eq!(list.len(), 0);
    }

    #[async_std::test]
    async fn test_remove_from_list_does_not_exist() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "DISCARD_FLAGS";

        set_list(&mut connection, key, &[String::from("item1")]).await;

        // when
        remove_from_list(&mut connection, key, "item2").await;
        let list = get_list(&mut connection, key).await;

        // then
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap(), "item1");
    }

    #[async_std::test]
    async fn test_directory() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let test_directory = get_canonicalized_path("test").await.unwrap();
        let key = "TEST_DIRECTORY";

        // when
        set_directory(&mut connection, key, &test_directory).await;

        let directory = get_directory(&mut connection, key).await.unwrap();

        // then
        assert_eq!(directory, test_directory);
    }

    #[async_std::test]
    async fn test_set_new_directory_when_old_is_missing() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let old_directory = PathBuf::from(&tmp_directory.path()).join("old");
        create_directory(&old_directory).await.unwrap();
        set_directory(&mut connection, "TEST_DIRECTORY", &old_directory).await;
        fs::remove_dir_all(&old_directory).await.unwrap();

        // when
        get_directory(&mut connection, "TEST_DIRECTORY").await;
        let new_directory = PathBuf::from(&tmp_directory.path()).join("new");
        create_directory(&new_directory).await.unwrap();
        set_directory(&mut connection, "TEST_DIRECTORY", &new_directory).await;

        // then
        let directory = get_directory(&mut connection, "TEST_DIRECTORY").await;
        assert!(directory.is_some());
        assert!(&directory.unwrap().as_os_str() == &new_directory.as_os_str());
    }
}
