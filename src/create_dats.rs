use super::common::*;
use super::config::*;
use super::model::*;
use super::SimpleResult;
use chrono::prelude::*;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use quick_xml::se;
use rust_embed::RustEmbed;
use serde::Serialize;
use sqlx::sqlite::SqliteConnection;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use walkdir::WalkDir;

#[derive(RustEmbed)]
#[folder = "data/"]
struct Assets;

pub const DOCTYPE: &[&str] = &["<?xml version=\"1.0\"?>", "<!DOCTYPE datafile PUBLIC \"-//Logiqx//DTD ROM Management Datafile//EN\" \"http://www.logiqx.com/Dats/datafile.dtd\">"];

pub fn subcommand() -> Command {
    Command::new("create-dats")
        .about("Create DAT files from directories")
        .arg(
            Arg::new("DIRECTORIES")
                .help("Set the directories to process")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("NAME")
                .short('n')
                .long("name")
                .help("Customize the DAT name")
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("DESCRIPTION")
                .short('d')
                .long("description")
                .help("Customize the DAT description")
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("VERSION")
                .short('v')
                .long("version")
                .help("Customize the DAT version")
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("AUTHOR")
                .short('a')
                .long("author")
                .help("Customize the DAT author")
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("URL")
                .short('u')
                .long("url")
                .help("Customize the DAT URL")
                .required(false)
                .num_args(1),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let directories = matches
        .get_many::<PathBuf>("DIRECTORIES")
        .unwrap()
        .cloned()
        .collect::<Vec<PathBuf>>();
    for directory in directories {
        create_dat(
            connection,
            progress_bar,
            directory,
            matches.get_one::<String>("NAME"),
            matches.get_one::<String>("DESCRIPTION"),
            matches.get_one::<String>("VERSION"),
            matches.get_one::<String>("AUTHOR"),
            matches.get_one::<String>("URL"),
        )
        .await?;
    }
    Ok(())
}

pub async fn create_dat<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    directory: P,
    name: Option<&String>,
    description: Option<&String>,
    version: Option<&String>,
    author: Option<&String>,
    url: Option<&String>,
) -> SimpleResult<()> {
    let system_xml = SystemXml {
        name: name.map(String::to_owned).unwrap_or(
            directory
                .as_ref()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
        ),
        description: description.map(String::to_owned).unwrap_or_default(),
        version: version
            .map(String::to_owned)
            .unwrap_or(format!("{}", Local::now().format("%Y%m%d"))),
        date: format!("{}", Local::now().format("%Y%m%d")),
        author: author.map(String::to_owned).unwrap_or_default(),
        url: url.map(String::to_owned),
        clrmamepros: Vec::new(),
    };

    let mut games_xml: Vec<GameXml> = Vec::new();
    let walker = WalkDir::new(&directory).into_iter();
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            let romfile = CommonRomfile::from_path(&entry.path())?;
            let rom_xml = RomXml {
                name: romfile
                    .path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
                size: romfile.get_size().await? as i64,
                crc: Some(
                    romfile
                        .get_hash_and_size(connection, progress_bar, 1, 1, &HashAlgorithm::Crc)
                        .await?
                        .0,
                ),
                md5: Some(
                    romfile
                        .get_hash_and_size(connection, progress_bar, 1, 1, &HashAlgorithm::Md5)
                        .await?
                        .0,
                ),
                sha1: Some(
                    romfile
                        .get_hash_and_size(connection, progress_bar, 1, 1, &HashAlgorithm::Sha1)
                        .await?
                        .0,
                ),
                merge: None,
                status: None,
            };
            let game_xml = GameXml {
                name: romfile
                    .path
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
                description: String::new(),
                roms: vec![rom_xml],
                isbios: false,
                isdevice: false,
                cloneof: None,
                romof: None,
                comment: None,
            };
            games_xml.push(game_xml);
        }
    }

    let datfile_xml = DatfileXml {
        system: system_xml,
        games: games_xml,
    };

    let mut buffer = String::new();
    let mut serializer = se::Serializer::new(&mut buffer);
    serializer.indent(' ', 2);
    try_with!(
        datfile_xml.serialize(serializer),
        "Failed to serialize DAT file"
    );

    progress_bar.println(DOCTYPE[0]);
    progress_bar.println(DOCTYPE[1]);
    progress_bar.println(buffer);

    Ok(())
}
