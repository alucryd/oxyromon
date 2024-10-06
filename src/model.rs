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
pub struct System {
    pub id: i64,
    pub name: String,
    pub custom_name: Option<String>,
    pub description: String,
    pub version: String,
    pub url: Option<String>,
    pub complete: bool,
    pub arcade: bool,
    pub merging: i64,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Header {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub size: i64,
    pub system_id: i64,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
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

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub comment: Option<String>,
    pub external_id: Option<String>,
    pub device: bool,
    pub bios: bool,
    pub jbfolder: bool,
    pub regions: String,
    pub sorting: i64,
    pub complete: bool,
    pub system_id: i64,
    pub parent_id: Option<i64>,
    pub bios_id: Option<i64>,
    pub playlist_id: Option<i64>,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
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
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub bios: bool,
    pub size: i64,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub rom_status: Option<String>,
    pub game_id: i64,
    pub romfile_id: Option<i64>,
    pub parent_id: Option<i64>,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
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

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Setting {
    pub id: i64,
    pub key: String,
    pub value: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileXml {
    #[serde(alias = "datfile")]
    pub systems: Vec<SystemXml>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "datafile")]
pub struct DatfileXml {
    #[serde(rename = "header")]
    pub system: SystemXml,
    #[serde(rename = "game", alias = "machine", default)]
    pub games: Vec<GameXml>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename = "header")]
pub struct SystemXml {
    pub name: String,
    pub description: String,
    pub version: String,
    pub date: String,
    pub author: String,
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
    pub comment: Option<String>,
    #[serde(rename = "@cloneof")]
    pub cloneof: Option<String>,
    #[serde(rename = "@romof")]
    pub romof: Option<String>,
    #[serde(rename = "@isdevice", deserialize_with = "string_to_bool", default)]
    pub isdevice: bool,
    #[serde(rename = "@isbios", deserialize_with = "string_to_bool", default)]
    pub isbios: bool,
    #[serde(rename = "rom", default)]
    pub roms: Vec<RomXml>,
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
    #[serde(rename = "@merge")]
    pub merge: Option<String>,
    #[serde(rename = "@size", deserialize_with = "empty_string_to_zero")]
    pub size: i64,
    #[serde(rename = "@crc")]
    pub crc: Option<String>,
    #[serde(rename = "@md5")]
    pub md5: Option<String>,
    #[serde(rename = "@sha1")]
    pub sha1: Option<String>,
    #[serde(rename = "@status")]
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
    pub regions_hashes: Vec<String>,
    pub files_count: usize,
    pub files_hashes: HashMap<u64, String>,
}
