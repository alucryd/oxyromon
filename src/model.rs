use anyhow::Result;
#[cfg(feature = "server")]
use async_graphql::{Enum, SimpleObject};
use num_derive::FromPrimitive;
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{FromRow, Type};
use std::collections::HashMap;

#[derive(Clone, Copy, FromPrimitive, Type, Eq, PartialEq)]
#[cfg_attr(feature = "server", derive(Enum))]
#[repr(i8)]
pub enum Merging {
    Split = 0,
    NonMerged = 1,
    FullNonMerged = 2,
    Merged = 3,
    FullMerged = 4,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
// Some fields are only read by the GraphQL layer.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct System {
    pub id: i64,
    pub name: String,
    pub custom_name: Option<String>,
    pub custom_extension: Option<String>,
    pub description: String,
    pub version: String,
    pub url: Option<String>,
    pub completion: i64,
    pub arcade: bool,
    pub merging: i64,
    /// Games in the system that are complete (not trashed).
    pub games_complete: i64,
    /// Games in the system that are not trashed.
    pub games_total: i64,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Header {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub size: i64,
    pub system_id: i64,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Rule {
    pub id: i64,
    pub start_byte: i64,
    pub hex_value: String,
    pub header_id: i64,
}

#[derive(FromPrimitive, Type)]
#[cfg_attr(feature = "server", derive(Clone, Copy, Enum, Eq, PartialEq))]
#[repr(i8)]
pub enum Sorting {
    AllRegions = 0,
    OneRegion = 1,
    Ignored = 2,
}

#[derive(FromPrimitive, Type)]
#[cfg_attr(feature = "server", derive(Clone, Copy, Enum, Eq, PartialEq))]
#[repr(i8)]
pub enum Completion {
    None = 0,
    Partial = 1,
    Full = 2,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Game {
    pub id: i64,
    pub name: String,
    // Resolved by hand in `query.rs`, which omits it when it would only repeat
    // `name`. For a console DAT that is nearly every game, and the duplicate
    // accounts for a third of the response for a large system.
    #[cfg_attr(feature = "server", graphql(skip))]
    pub description: String,
    pub comment: Option<String>,
    pub external_id: Option<String>,
    pub device: bool,
    pub bios: bool,
    pub jbfolder: bool,
    pub regions: String,
    pub sorting: i64,
    pub completion: i64,
    pub system_id: i64,
    pub parent_id: Option<i64>,
    pub bios_id: Option<i64>,
    pub playlist_id: Option<i64>,
    // The DAT's categories, joined with `|`, for `DISCARD_CATEGORIES`.
    #[cfg_attr(feature = "server", graphql(skip))]
    pub categories: Option<String>,
}

#[cfg(feature = "server")]
#[derive(Clone, SimpleObject)]
pub struct Dependency {
    pub name: String,
    pub version: Option<String>,
}

#[cfg(feature = "server")]
#[derive(Clone, FromRow, SimpleObject)]
pub struct GameInformation {
    pub title: String,
    pub regions: Vec<String>,
    pub languages: Vec<String>,
    pub release: Option<String>,
    pub flags: Vec<String>,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub bios: bool,
    pub disk: bool,
    pub size: i64,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub rom_status: Option<String>,
    pub game_id: i64,
    pub romfile_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub original: bool,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Patch {
    pub id: i64,
    pub name: String,
    pub index: i64,
    pub rom_id: i64,
    pub romfile_id: i64,
}

#[derive(FromPrimitive, Type)]
#[cfg_attr(feature = "server", derive(Clone, Copy, Enum, Eq, PartialEq))]
#[repr(i8)]
pub enum RomfileType {
    Romfile = 0,
    Playlist = 1,
    Patch = 2,
}

#[derive(FromRow, PartialEq, Eq)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Romfile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub parent_id: Option<i64>,
    pub romfile_type: i64,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub struct Setting {
    pub id: i64,
    pub key: String,
    pub value: Option<String>,
    pub system_id: Option<i64>,
}

/// How a clone list title's search term finds games
#[derive(Clone, Copy, FromPrimitive, Eq, PartialEq)]
#[repr(i8)]
pub enum CloneListNameType {
    /// The title `INFER_CLONES` groups games by
    Short = 0,
    /// The full name
    Full = 1,
    /// A regex searched in the name, ignoring case
    Regex = 2,
}

/// A title of a clone list group, or one of its filters: when the game's
/// regions and languages include all those listed and its name matches the
/// pattern, the filter's group and priority apply instead of the title's
#[derive(FromRow)]
pub struct CloneListTitle {
    pub id: i64,
    pub group_name: String,
    pub search_term: String,
    pub name_type: i64,
    pub priority: i64,
    pub match_regions: Option<String>,
    pub match_languages: Option<String>,
    pub match_string: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileXml {
    #[serde(alias = "datfile")]
    pub systems: Vec<SystemXml>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "datafile")]
pub struct DatfileXml {
    /// absent from MAME's -listxml output, which `parse_dat` fills in
    #[serde(rename = "header", default)]
    pub system: SystemXml,
    /// only set by MAME's -listxml output, on its `<mame>` root
    #[serde(rename = "@build", default, skip_serializing)]
    pub build: Option<String>,
    #[serde(rename = "game", default)]
    pub games: Vec<GameXml>,
    #[serde(rename = "machine", default)]
    pub machines: Vec<GameXml>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename = "header")]
pub struct SystemXml {
    pub name: String,
    pub description: String,
    pub version: String,
    pub date: Option<String>,
    pub author: Option<String>,
    #[serde(rename = "clrmamepro", default)]
    pub clrmamepros: Vec<ClrMameProXml>,
    pub url: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "clrmamepro")]
pub struct ClrMameProXml {
    #[serde(rename = "@header")]
    pub header: Option<String>,
}

fn string_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(s == "yes")
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "game")]
pub struct GameXml {
    #[serde(rename = "@name")]
    pub name: String,
    pub description: String,
    #[serde(skip_serializing)]
    pub comment: Option<String>,
    // No-Intro's schema allows any number, Redump DATs give one
    #[serde(rename = "category", default, skip_serializing)]
    pub categories: Vec<String>,
    #[serde(rename = "@cloneof", skip_serializing)]
    pub cloneof: Option<String>,
    #[serde(rename = "@romof", skip_serializing)]
    pub romof: Option<String>,
    #[serde(
        rename = "@isdevice",
        deserialize_with = "string_to_bool",
        default,
        skip_serializing
    )]
    pub isdevice: bool,
    #[serde(
        rename = "@isbios",
        deserialize_with = "string_to_bool",
        default,
        skip_serializing
    )]
    pub isbios: bool,
    #[serde(rename = "rom", default)]
    pub roms: Vec<RomXml>,
    #[serde(rename = "disk", default)]
    pub disks: Vec<RomXml>,
}

fn empty_string_to_zero<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<i64>().or_else(|_| Ok(0))
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "rom")]
pub struct RomXml {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@merge", skip_serializing)]
    pub merge: Option<String>,
    #[serde(rename = "@size", default, deserialize_with = "empty_string_to_zero")]
    pub size: i64,
    #[serde(rename = "@crc")]
    pub crc: Option<String>,
    #[serde(rename = "@md5")]
    pub md5: Option<String>,
    #[serde(rename = "@sha1")]
    pub sha1: Option<String>,
    #[serde(rename = "@status", skip_serializing)]
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct DetectorXml {
    pub name: String,
    pub version: String,
    pub rule: RuleXml,
}

#[derive(Deserialize)]
pub struct RuleXml {
    #[serde(rename = "@start_offset")]
    pub start_offset: String,
    pub data: Vec<DataXml>,
}

#[derive(Deserialize)]
pub struct DataXml {
    #[serde(rename = "@offset")]
    pub offset: String,
    #[serde(rename = "@value")]
    pub value: String,
}

pub struct Irdfile {
    pub version: u8,
    pub game_id: String,
    pub game_name: String,
    pub update_version: String,
    pub game_version: String,
    pub app_version: String,
    pub regions_count: usize,
    // parsed from the IRD file but not consumed yet
    #[allow(dead_code)]
    pub regions_hashes: Vec<String>,
    pub files_count: usize,
    pub files_hashes: HashMap<u64, String>,
}

/// A Retool clone list, of which `import-clone-lists` reads the groups' titles
/// and supersets
#[derive(Deserialize)]
pub struct CloneListJson {
    pub description: CloneListDescriptionJson,
    #[serde(default)]
    pub variants: Vec<CloneListVariantJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneListDescriptionJson {
    pub name: String,
    pub last_updated: String,
}

#[derive(Deserialize)]
pub struct CloneListVariantJson {
    pub group: String,
    #[serde(default)]
    pub titles: Vec<CloneListVariantTitleJson>,
    #[serde(default)]
    pub supersets: Vec<CloneListVariantTitleJson>,
    #[serde(default)]
    pub compilations: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneListVariantTitleJson {
    pub search_term: String,
    pub name_type: Option<String>,
    pub priority: Option<i64>,
    #[serde(default)]
    pub filters: Vec<CloneListFilterJson>,
}

#[derive(Deserialize)]
pub struct CloneListFilterJson {
    pub conditions: CloneListConditionsJson,
    pub results: CloneListResultsJson,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneListConditionsJson {
    pub match_regions: Option<Vec<String>>,
    pub match_languages: Option<Vec<String>>,
    pub match_string: Option<String>,
    pub region_order: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct CloneListResultsJson {
    pub group: Option<String>,
    pub priority: Option<i64>,
}
