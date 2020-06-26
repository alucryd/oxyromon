use super::schema::{games, headers, releases, romfiles, roms, systems};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Identifiable, PartialEq, Queryable)]
pub struct System {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "systems"]
pub struct SystemInput<'a> {
    pub name: &'a String,
    pub description: &'a String,
    pub version: &'a String,
}

#[derive(Deserialize)]
pub struct SystemXml {
    pub name: String,
    pub description: String,
    pub version: String,
    pub clrmamepro: Option<ClrMameProXml>,
}

impl<'a> From<&'a SystemXml> for SystemInput<'a> {
    fn from(system_xml: &'a SystemXml) -> Self {
        Self {
            name: &system_xml.name,
            description: &system_xml.description,
            version: &system_xml.version,
        }
    }
}

#[derive(Associations, Identifiable, PartialEq, Queryable)]
#[belongs_to(System)]
pub struct Header {
    pub id: Uuid,
    pub name: String,
    pub version: String,
    pub start: i32,
    pub size: i32,
    pub hex_value: String,
    pub system_id: Uuid,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "headers"]
pub struct HeaderInput<'a> {
    pub name: &'a String,
    pub version: &'a String,
    pub start: i32,
    pub size: i32,
    pub hex_value: &'a String,
    pub system_id: &'a Uuid,
}

type DetectorXmlSystemId<'a> = (&'a DetectorXml, &'a Uuid);
impl<'a> From<DetectorXmlSystemId<'a>> for HeaderInput<'a> {
    fn from(detector_xml_system_id: DetectorXmlSystemId<'a>) -> Self {
        Self {
            name: &detector_xml_system_id.0.name,
            version: &detector_xml_system_id.0.version,
            start: i32::from_str_radix(&detector_xml_system_id.0.rule.data.offset, 16).unwrap(),
            size: i32::from_str_radix(&detector_xml_system_id.0.rule.start_offset, 16).unwrap(),
            hex_value: &detector_xml_system_id.0.rule.data.value,
            system_id: detector_xml_system_id.1,
        }
    }
}

#[derive(Associations, Identifiable, PartialEq, Queryable)]
#[belongs_to(System)]
#[belongs_to(Game, foreign_key = "parent_id")]
pub struct Game {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub regions: String,
    pub system_id: Uuid,
    pub parent_id: Option<Uuid>,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "games"]
pub struct GameInput<'a> {
    pub name: &'a String,
    pub description: &'a String,
    pub regions: &'a String,
    pub system_id: &'a Uuid,
    pub parent_id: Option<&'a Uuid>,
}

#[derive(Deserialize)]
pub struct GameXml {
    pub name: String,
    pub description: String,
    pub cloneof: Option<String>,
    #[serde(rename = "release", default)]
    pub releases: Vec<ReleaseXml>,
    #[serde(rename = "rom", default)]
    pub roms: Vec<RomXml>,
}

type GameXmlSystemIdParentId<'a> = (&'a GameXml, &'a String, &'a Uuid, Option<&'a Uuid>);
impl<'a> From<GameXmlSystemIdParentId<'a>> for GameInput<'a> {
    fn from(game_xml_system_id_parent_id: GameXmlSystemIdParentId<'a>) -> Self {
        Self {
            name: &game_xml_system_id_parent_id.0.name,
            description: &game_xml_system_id_parent_id.0.description,
            regions: game_xml_system_id_parent_id.1,
            system_id: game_xml_system_id_parent_id.2,
            parent_id: game_xml_system_id_parent_id.3,
        }
    }
}

#[derive(Associations, Identifiable, PartialEq, Queryable)]
#[belongs_to(Game)]
pub struct Release {
    pub id: Uuid,
    pub name: String,
    pub region: String,
    pub game_id: Uuid,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "releases"]
pub struct ReleaseInput<'a> {
    pub name: &'a String,
    pub region: &'a String,
    pub game_id: &'a Uuid,
}

#[derive(Deserialize)]
pub struct ReleaseXml {
    pub name: String,
    pub region: String,
}

type ReleaseXmlGameId<'a> = (&'a ReleaseXml, &'a Uuid);
impl<'a> From<ReleaseXmlGameId<'a>> for ReleaseInput<'a> {
    fn from(release_xml_game_id: ReleaseXmlGameId<'a>) -> Self {
        Self {
            name: &release_xml_game_id.0.name,
            region: &release_xml_game_id.0.region,
            game_id: release_xml_game_id.1,
        }
    }
}

#[derive(Associations, Debug, Identifiable, PartialEq, Queryable)]
#[belongs_to(Game)]
#[belongs_to(Romfile)]
pub struct Rom {
    pub id: Uuid,
    pub name: String,
    pub size: i64,
    pub crc: String,
    pub md5: String,
    pub sha1: String,
    pub status: Option<String>,
    pub game_id: Uuid,
    pub romfile_id: Option<Uuid>,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "roms"]
pub struct RomInput<'a> {
    pub name: &'a String,
    pub size: i64,
    pub crc: &'a String,
    pub md5: &'a String,
    pub sha1: &'a String,
    pub status: Option<&'a String>,
    pub game_id: &'a Uuid,
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

type RomXmlGameId<'a> = (&'a RomXml, &'a Uuid);
impl<'a> From<RomXmlGameId<'a>> for RomInput<'a> {
    fn from(rom_xml: RomXmlGameId<'a>) -> Self {
        Self {
            name: &rom_xml.0.name,
            size: rom_xml.0.size,
            crc: &rom_xml.0.crc,
            md5: &rom_xml.0.md5,
            sha1: &rom_xml.0.sha1,
            status: rom_xml.0.status.as_ref(),
            game_id: rom_xml.1,
        }
    }
}

#[derive(Identifiable, PartialEq, Queryable)]
pub struct Romfile {
    pub id: Uuid,
    pub path: String,
}

#[derive(AsChangeset, Insertable)]
#[table_name = "romfiles"]
pub struct RomfileInput<'a> {
    pub path: &'a String,
}

#[derive(Deserialize)]
pub struct ClrMameProXml {
    pub header: String,
}

#[derive(Deserialize)]
pub struct DatfileXml {
    #[serde(rename = "header")]
    pub system: SystemXml,
    #[serde(rename = "game")]
    pub games: Vec<GameXml>,
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
