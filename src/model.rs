#[cfg(feature = "server")]
use async_graphql::{Enum, SimpleObject};
use num_derive::FromPrimitive;
use serde::Deserialize;
use sqlx::{FromRow, Type};

#[derive(Clone, Copy, FromPrimitive, Type, Eq, PartialEq)]
#[cfg_attr(feature = "server", derive(Enum))]
#[repr(i8)]
pub enum Merging {
    Split = 0,
    NonMerged = 1,
    FullNonMerged = 2,
    Merged = 3,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct System {
    pub id: i64,
    pub name: String,
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
    pub bios: bool,
    pub regions: String,
    pub sorting: i64,
    pub complete: bool,
    pub system_id: i64,
    pub parent_id: Option<i64>,
    pub bios_id: Option<i64>,
}

#[derive(FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub bios: bool,
    pub size: i64,
    pub crc: String,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub rom_status: Option<String>,
    pub game_id: i64,
    pub romfile_id: Option<i64>,
    pub parent_id: Option<i64>,
}

#[derive(FromRow, PartialEq)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Romfile {
    pub id: i64,
    pub path: String,
    pub size: i64,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Setting {
    pub id: i64,
    pub key: String,
    pub value: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileXml {
    #[serde(rename = "datfile")]
    pub systems: Vec<SystemXml>,
}

#[derive(Deserialize)]
pub struct DatfileXml {
    #[serde(rename = "header")]
    pub system: SystemXml,
    #[serde(rename = "game")]
    pub games: Vec<GameXml>,
}

#[derive(Deserialize)]
pub struct SystemXml {
    pub name: String,
    pub description: String,
    pub version: String,
    pub clrmamepro: Option<ClrMameProXml>,
    pub url: Option<String>,
}

#[derive(Deserialize)]
pub struct ClrMameProXml {
    pub header: Option<String>,
}

#[derive(Deserialize)]
pub struct GameXml {
    pub name: String,
    pub description: String,
    pub comment: Option<String>,
    pub cloneof: Option<String>,
    pub romof: Option<String>,
    pub isbios: Option<String>,
    #[serde(rename = "rom", default)]
    pub roms: Vec<RomXml>,
}

#[derive(Deserialize)]
pub struct RomXml {
    pub name: String,
    pub merge: Option<String>,
    pub size: i64,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
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
    pub start_offset: String,
    pub data: Vec<DataXml>,
}

#[derive(Deserialize)]
pub struct DataXml {
    pub offset: String,
    pub value: String,
}
