#[cfg(feature = "server")]
use async_graphql::SimpleObject;
use serde::Deserialize;

#[derive(sqlx::FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct System {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub version: String,
    pub url: Option<String>,
}

#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
pub struct Header {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub start_byte: i64,
    pub size: i64,
    pub hex_value: String,
    pub system_id: i64,
}

#[derive(sqlx::FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub regions: String,
    pub system_id: i64,
    pub parent_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub crc: String,
    pub md5: String,
    pub sha1: String,
    pub rom_status: Option<String>,
    pub game_id: i64,
    pub romfile_id: Option<i64>,
}

#[derive(sqlx::FromRow, PartialEq)]
#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]
#[cfg_attr(feature = "server", graphql(complex))]
pub struct Romfile {
    pub id: i64,
    pub path: String,
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
    pub header: String,
}

#[derive(Deserialize)]
pub struct GameXml {
    pub name: String,
    pub description: String,
    pub cloneof: Option<String>,
    #[serde(rename = "rom", default)]
    pub roms: Vec<RomXml>,
}

#[derive(Deserialize)]
pub struct RomXml {
    pub name: String,
    pub size: i64,
    pub crc: String,
    pub md5: String,
    pub sha1: String,
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
    pub data: DataXml,
}

#[derive(Deserialize)]
pub struct DataXml {
    pub offset: String,
    pub value: String,
}
