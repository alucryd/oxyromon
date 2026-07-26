use super::chdman::{
    CHD_HUNK_SIZE_RANGE, ChdCdCompressionAlgorithm, ChdDvdCompressionAlgorithm,
    ChdHdCompressionAlgorithm, ChdLdCompressionAlgorithm,
};
use super::database::*;
use super::dolphin::{RVZ_BLOCK_SIZE_RANGE, RVZ_COMPRESSION_LEVEL_RANGE, RvzCompressionAlgorithm};
use super::model::Setting;
use super::progress::*;
use super::prompt::{prompt_for_system_like, prompt_for_systems_like};
use super::sevenzip::{SEVENZIP_COMPRESSION_LEVEL_RANGE, ZIP_COMPRESSION_LEVEL_RANGE};
use super::util::*;
use cfg_if::cfg_if;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use phf::phf_map;
use simple_error::SimpleResult;
use simple_error::try_with;
use sqlx::sqlite::SqliteConnection;
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use strum::{Display, EnumIter, EnumString, VariantNames};

#[derive(Display, PartialEq, EnumIter, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum HashAlgorithm {
    Crc,
    Md5,
    Sha1,
}

#[derive(PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum SubfolderScheme {
    None,
    Alpha,
}

#[derive(PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum PreferredVersion {
    None,
    New,
    Old,
}

#[derive(PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum PreferredRegion {
    None,
    Broad,
    Narrow,
}

#[derive(PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ArcadeRomType {
    Bios,
    Clone,
    Parent,
}

const BOOLEANS: &[&str] = &[
    "CHD_PARENTS",
    "GROUP_SUBSYSTEMS",
    "PREFER_PARENTS",
    "REGIONS_ONE_STRICT",
    "RVZ_SCRUB",
    "SEVENZIP_SOLID_COMPRESSION",
];
const CHOICES: phf::Map<&str, &[&str]> = phf_map! {
    "PREFER_REGIONS" => PreferredRegion::VARIANTS,
    "PREFER_VERSIONS" => PreferredVersion::VARIANTS,
    "REGIONS_ALL_SUBFOLDERS" => SubfolderScheme::VARIANTS,
    "REGIONS_ONE_SUBFOLDERS" => SubfolderScheme::VARIANTS,
    "RVZ_COMPRESSION_ALGORITHM" => RvzCompressionAlgorithm::VARIANTS,
};
const CHOICE_LISTS: phf::Map<&str, &[&str]> = phf_map! {
    "CHD_CD_COMPRESSION_ALGORITHMS" => ChdCdCompressionAlgorithm::VARIANTS,
    "CHD_DVD_COMPRESSION_ALGORITHMS" => ChdDvdCompressionAlgorithm::VARIANTS,
    "CHD_HD_COMPRESSION_ALGORITHMS" => ChdHdCompressionAlgorithm::VARIANTS,
    "CHD_LD_COMPRESSION_ALGORITHMS" => ChdLdCompressionAlgorithm::VARIANTS,
    "REGIONS_ALL_ARCADE" => ArcadeRomType::VARIANTS,
    "REGIONS_ONE_ARCADE" => ArcadeRomType::VARIANTS,
};
const INTEGERS: phf::Map<&str, &[usize; 2]> = phf_map! {
    "CHD_CD_HUNK_SIZE" => &CHD_HUNK_SIZE_RANGE,
    "CHD_DVD_HUNK_SIZE" => &CHD_HUNK_SIZE_RANGE,
    "CHD_HD_HUNK_SIZE" => &CHD_HUNK_SIZE_RANGE,
    "CHD_LD_HUNK_SIZE" => &CHD_HUNK_SIZE_RANGE,
    "RVZ_BLOCK_SIZE" => &RVZ_BLOCK_SIZE_RANGE,
    "RVZ_COMPRESSION_LEVEL" => &RVZ_COMPRESSION_LEVEL_RANGE,
    "SEVENZIP_COMPRESSION_LEVEL" => &SEVENZIP_COMPRESSION_LEVEL_RANGE,
    "ZIP_COMPRESSION_LEVEL" => &ZIP_COMPRESSION_LEVEL_RANGE,
};
const LISTS: &[&str] = &[
    "DISCARD_FLAGS",
    "DISCARD_RELEASES",
    "LANGUAGES",
    "PREFER_FLAGS",
    "REGIONS_ALL",
    "REGIONS_ONE",
];
const PATHS: &[&str] = &["ROM_DIRECTORY", "TMP_DIRECTORY"];

const NULLABLES: &[&str] = &[
    "CHD_CD_HUNK_SIZE",
    "CHD_CD_COMPRESSION_ALGORITHMS",
    "CHD_DVD_HUNK_SIZE",
    "CHD_DVD_COMPRESSION_ALGORITHMS",
    "DISCARD_FLAGS",
    "DISCARD_RELEASES",
    "LANGUAGES",
    "PREFER_FLAGS",
    "REGIONS_ALL",
    "REGIONS_ALL_ARCADE",
    "REGIONS_ONE",
    "REGIONS_ONE_ARCADE",
    "SEVENZIP_COMPRESSION_LEVEL",
    "ZIP_COMPRESSION_LEVEL",
];

const SORTED_LISTS: &[&str] = &[
    "CHD_CD_COMPRESSION_ALGORITHMS",
    "CHD_DVD_COMPRESSION_ALGORITHMS",
    "REGIONS_ONE",
];
const LIST_SEPARATOR: &str = "|";

pub static PS3_DISC_SFB: &str = "PS3_DISC.SFB";

pub fn subcommand() -> Command {
    Command::new("config")
        .about("Query and modify the oxyromon settings")
        .arg(
            Arg::new("SYSTEM")
                .short('n')
                .long("system")
                .help("Select a system by name (supports % globs)")
                .required(false)
                .num_args(1)
                .value_name("NAME"),
        )
        .arg(
            Arg::new("LIST")
                .short('l')
                .long("list")
                .help("Print the whole configuration")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with_all(["GET", "SET", "UNSET", "ADD", "REMOVE"]),
        )
        .arg(
            Arg::new("GET")
                .short('g')
                .long("get")
                .help("Print a single setting")
                .required(false)
                .num_args(1)
                .value_name("KEY")
                .conflicts_with_all(["LIST", "SET", "UNSET", "ADD", "REMOVE"]),
        )
        .arg(
            Arg::new("SET")
                .short('s')
                .long("set")
                .help("Set a single setting")
                .required(false)
                .num_args(2)
                .value_names(["KEY", "VALUE"])
                .conflicts_with_all(["LIST", "GET", "UNSET", "ADD", "REMOVE"]),
        )
        .arg(
            Arg::new("UNSET")
                .short('u')
                .long("unset")
                .help("Unset a single setting")
                .required(false)
                .num_args(1)
                .value_name("KEY")
                .conflicts_with_all(["LIST", "GET", "SET", "ADD", "REMOVE"]),
        )
        .arg(
            Arg::new("ADD")
                .short('a')
                .long("add")
                .help("Add an entry to a list")
                .required(false)
                .num_args(2)
                .value_names(["KEY", "VALUE"])
                .conflicts_with_all(["LIST", "GET", "SET", "UNSET", "REMOVE"]),
        )
        .arg(
            Arg::new("REMOVE")
                .short('r')
                .long("remove")
                .help("Remove an entry from a list")
                .required(false)
                .num_args(2)
                .value_names(["KEY", "VALUE"])
                .conflicts_with_all(["LIST", "GET", "SET", "UNSET", "ADD"]),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let is_editing = matches.contains_id("SET")
        || matches.contains_id("UNSET")
        || matches.contains_id("ADD")
        || matches.contains_id("REMOVE");

    let system_ids: Vec<Option<i64>> = match matches.get_one::<String>("SYSTEM") {
        None => vec![None],
        Some(system_name) => {
            if is_editing {
                let systems = prompt_for_systems_like(connection, system_name).await?;
                if systems.is_empty() {
                    print_warning(progress_bar, "No matching system found");
                    return Ok(());
                }
                systems.into_iter().map(|s| Some(s.id)).collect()
            } else {
                vec![Some(
                    prompt_for_system_like(connection, None, system_name)
                        .await?
                        .id,
                )]
            }
        }
    };

    for system_id in system_ids {
        if matches.get_flag("LIST") {
            list_settings(connection, progress_bar, system_id).await;
        } else if matches.contains_id("GET") {
            print_setting(
                connection,
                progress_bar,
                matches.get_one::<String>("GET").unwrap(),
                system_id,
            )
            .await;
        } else if matches.contains_id("SET") {
            if let [key, value] = matches
                .get_many::<String>("SET")
                .unwrap()
                .collect::<Vec<_>>()
                .as_slice()
            {
                set_setting(connection, progress_bar, key, value, system_id).await?;
            };
        } else if matches.contains_id("UNSET") {
            unset_setting(
                connection,
                progress_bar,
                matches.get_one::<String>("UNSET").unwrap(),
                system_id,
            )
            .await?;
        } else if matches.contains_id("ADD") {
            if let [key, value] = matches
                .get_many::<String>("ADD")
                .unwrap()
                .collect::<Vec<_>>()
                .as_slice()
            {
                add_to_list(connection, progress_bar, key, value, system_id).await;
            };
        } else if matches.contains_id("REMOVE")
            && let [key, value] = matches
                .get_many::<String>("REMOVE")
                .unwrap()
                .collect::<Vec<_>>()
                .as_slice()
        {
            remove_from_list(connection, progress_bar, key, value, system_id).await;
        };
    }

    Ok(())
}

pub async fn get_setting(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> Option<Setting> {
    if let Some(id) = system_id
        && let Some(setting) = find_setting_by_key(connection, key, Some(id)).await
    {
        return Some(setting);
    }
    find_setting_by_key(connection, key, None).await
}

pub async fn list_settings(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_id: Option<i64>,
) {
    let settings = if let Some(id) = system_id {
        let mut merged: std::collections::HashMap<String, Setting> =
            find_settings(connection, None)
                .await
                .into_iter()
                .map(|s| (s.key.clone(), s))
                .collect();
        for setting in find_settings(connection, Some(id)).await {
            merged.insert(setting.key.clone(), setting);
        }
        let mut result: Vec<Setting> = merged.into_values().collect();
        result.sort_by(|a, b| a.key.cmp(&b.key));
        result
    } else {
        find_settings(connection, None).await
    };
    for setting in settings {
        print_info(
            progress_bar,
            &format!("{} = {}", setting.key, setting.value.unwrap_or_default()),
        );
    }
}

async fn print_setting(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    key: &str,
    system_id: Option<i64>,
) {
    let setting = get_setting(connection, key, system_id).await.unwrap();
    print_info(
        progress_bar,
        &format!("{} = {}", setting.key, setting.value.unwrap_or_default()),
    );
}

pub async fn set_setting(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    key: &str,
    value: &str,
    system_id: Option<i64>,
) -> SimpleResult<()> {
    if PATHS.contains(&key) {
        let p = get_canonicalized_path(&value.to_owned()).await?;
        create_directory(progress_bar, &p, false).await?;
        set_directory(connection, key, &p, system_id).await;
    } else if BOOLEANS.contains(&key) {
        let b: bool = try_with!(FromStr::from_str(value), "Failed to parse bool");
        set_bool(connection, key, b, system_id).await;
    } else if CHOICES.keys().any(|&s| s == key) {
        if CHOICES.get(key).unwrap().contains(&value) {
            set_string(connection, key, value, system_id).await;
        } else {
            print_warning(
                progress_bar,
                &format!("Valid choices: {:?}", CHOICES.get(key).unwrap()),
            );
        }
    } else if INTEGERS.keys().any(|&i| i == key) {
        let i: usize = try_with!(FromStr::from_str(value), "Failed to parse integer");
        if INTEGERS.get(key).unwrap()[0] <= i && i <= INTEGERS.get(key).unwrap()[1] {
            set_integer(connection, key, i, system_id).await;
        } else {
            print_warning(
                progress_bar,
                &format!("Valid range: {:?}", INTEGERS.get(key).unwrap()),
            );
        }
    } else if LISTS.contains(&key) {
        print_warning(
            progress_bar,
            "Lists can't be set directly, use --add or --remove instead",
        );
    } else {
        print_error(progress_bar, &format!("Unknown setting: {}", key));
    }
    Ok(())
}

pub async fn unset_setting(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    key: &str,
    system_id: Option<i64>,
) -> SimpleResult<()> {
    if NULLABLES.contains(&key) {
        if let Some(setting) = find_setting_by_key(connection, key, system_id).await {
            update_setting(connection, setting.id, None).await;
        };
    } else {
        print_error(
            progress_bar,
            &format!("Setting \"{}\" cannot be unset", key),
        );
    }
    Ok(())
}

pub async fn get_bool(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> bool {
    get_setting(connection, key, system_id)
        .await
        .unwrap()
        .value
        .unwrap()
        .parse()
        .unwrap()
}

pub async fn set_bool(
    connection: &mut SqliteConnection,
    key: &str,
    value: bool,
    system_id: Option<i64>,
) {
    let setting = find_setting_by_key(connection, key, system_id).await;
    let value = value.to_string();
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value)).await,
        None => create_setting(connection, key, Some(value), system_id).await,
    };
}

pub async fn get_integer(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> Option<usize> {
    get_setting(connection, key, system_id)
        .await
        .unwrap()
        .value
        .map(|value| value.parse().unwrap())
}

async fn set_integer(
    connection: &mut SqliteConnection,
    key: &str,
    value: usize,
    system_id: Option<i64>,
) {
    let setting = find_setting_by_key(connection, key, system_id).await;
    let value = value.to_string();
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value)).await,
        None => create_setting(connection, key, Some(value), system_id).await,
    };
}

pub async fn get_list(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> Vec<String> {
    match get_setting(connection, key, system_id).await {
        Some(setting) => match setting.value {
            Some(value) => value.split(LIST_SEPARATOR).map(|s| s.to_owned()).collect(),
            None => vec![],
        },
        None => vec![],
    }
}

pub async fn add_to_list(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    key: &str,
    value: &str,
    system_id: Option<i64>,
) {
    if LISTS.contains(&key) {
        let mut list = get_list(connection, key, system_id).await;
        if !list.contains(&String::from(value)) {
            list.push(value.to_owned());
            if !SORTED_LISTS.contains(&key) {
                list.sort();
            }
            set_list(connection, key, &list, system_id).await;
        } else {
            print_skip(progress_bar, "Value already in list");
        }
    } else if CHOICE_LISTS.keys().any(|&s| s == key) {
        if CHOICE_LISTS.get(key).unwrap().contains(&value) {
            let mut list = get_list(connection, key, system_id).await;
            if !list.contains(&String::from(value)) {
                list.push(value.to_owned());
                if !SORTED_LISTS.contains(&key) {
                    list.sort();
                }
                set_list(connection, key, &list, system_id).await;
            } else {
                print_skip(progress_bar, "Value already in list");
            }
        } else {
            print_warning(
                progress_bar,
                &format!("Valid choices: {:?}", CHOICE_LISTS.get(key).unwrap()),
            );
        }
    } else {
        print_error(progress_bar, "Only list settings support --add");
    }
}

pub async fn remove_from_list(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    key: &str,
    value: &str,
    system_id: Option<i64>,
) {
    if LISTS.contains(&key) || CHOICE_LISTS.keys().any(|&s| s == key) {
        let mut list = get_list(connection, key, system_id).await;
        if list.contains(&String::from(value)) {
            list.remove(list.iter().position(|v| v == value).unwrap());
            set_list(connection, key, &list, system_id).await;
        } else {
            print_warning(progress_bar, "Value not found in list");
        }
    } else {
        print_error(progress_bar, "Only list settings support --remove");
    }
}

async fn set_list(
    connection: &mut SqliteConnection,
    key: &str,
    value: &[String],
    system_id: Option<i64>,
) {
    let setting = find_setting_by_key(connection, key, system_id).await;
    let value = if value.is_empty() {
        None
    } else {
        Some(value.join(LIST_SEPARATOR))
    };
    match setting {
        Some(setting) => update_setting(connection, setting.id, value).await,
        None => create_setting(connection, key, value, system_id).await,
    };
}

pub async fn get_directory(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> Option<PathBuf> {
    match get_setting(connection, key, system_id).await {
        Some(p) => get_canonicalized_path(&p.value.unwrap()).await.ok(),
        None => None,
    }
}

pub async fn set_directory<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    key: &str,
    value: &P,
    system_id: Option<i64>,
) {
    let setting = find_setting_by_key(connection, key, system_id).await;
    let value = value.as_ref().as_os_str().to_str().unwrap().to_owned();
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value)).await,
        None => create_setting(connection, key, Some(value), system_id).await,
    };
}

pub async fn get_string(
    connection: &mut SqliteConnection,
    key: &str,
    system_id: Option<i64>,
) -> Option<String> {
    get_setting(connection, key, system_id).await.unwrap().value
}

pub async fn set_string(
    connection: &mut SqliteConnection,
    key: &str,
    value: &str,
    system_id: Option<i64>,
) {
    let setting = find_setting_by_key(connection, key, system_id).await;
    match setting {
        Some(setting) => update_setting(connection, setting.id, Some(value.to_string())).await,
        None => create_setting(connection, key, Some(value.to_string()), system_id).await,
    };
}

pub async fn get_rom_directory(connection: &mut SqliteConnection) -> PathBuf {
    match get_directory(connection, "ROM_DIRECTORY", None).await {
        Some(rom_directory) => rom_directory,
        None => {
            let rom_directory = match env::var("OXYROMON_ROM_DIRECTORY") {
                Ok(rom_directory) => PathBuf::from(rom_directory),
                Err(_) => dirs::home_dir().unwrap().join("Emulation"),
            };
            set_directory(connection, "ROM_DIRECTORY", &rom_directory, None).await;
            rom_directory
        }
    }
}

pub async fn get_tmp_directory(connection: &mut SqliteConnection) -> PathBuf {
    match get_directory(connection, "TMP_DIRECTORY", None).await {
        Some(tmp_directory) => tmp_directory,
        None => {
            let tmp_directory = match env::var("OXYROMON_TMP_DIRECTORY") {
                Ok(tmp_directory) => PathBuf::from(tmp_directory),
                Err(_) => env::temp_dir(),
            };
            set_directory(connection, "TMP_DIRECTORY", &tmp_directory, None).await;
            tmp_directory
        }
    }
}

cfg_if! {
    if #[cfg(test)] {
        use std::sync::LazyLock;
        use tokio::sync::Mutex;

        pub static MUTEX: LazyLock<Mutex<i32>> = LazyLock::new(|| Mutex::new(0));

        pub async fn set_rom_directory(connection: &mut SqliteConnection, rom_directory: PathBuf) -> PathBuf {
            set_directory(connection, "ROM_DIRECTORY", &rom_directory, None).await;
            rom_directory
        }

        pub async fn set_tmp_directory(connection: &mut SqliteConnection, tmp_directory: PathBuf) -> PathBuf {
            set_directory(connection, "TMP_DIRECTORY", &tmp_directory, None).await;
            tmp_directory
        }
    }
}

#[cfg(test)]
mod test_add_to_list;
#[cfg(test)]
mod test_add_to_list_already_exists;
#[cfg(test)]
mod test_bool;
#[cfg(test)]
mod test_directory;
#[cfg(test)]
mod test_list;
#[cfg(test)]
mod test_remove_from_list;
#[cfg(test)]
mod test_remove_from_list_does_not_exist;
#[cfg(test)]
mod test_set_new_directory_when_old_is_missing;
#[cfg(test)]
mod test_system_bool_fallback;
#[cfg(test)]
mod test_system_bool_override;
