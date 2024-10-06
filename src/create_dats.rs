use super::model::*;
use super::SimpleResult;
use chrono::prelude::*;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use quick_xml::se;
use rust_embed::RustEmbed;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::str;

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
                .help("Customize the DAT description")
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
}

pub async fn main(matches: &ArgMatches, progress_bar: &ProgressBar) -> SimpleResult<()> {
    let directories = matches
        .get_many::<PathBuf>("DIRECTORIES")
        .unwrap()
        .cloned()
        .collect::<Vec<PathBuf>>();
    for directory in directories {
        create_dat(
            directory,
            matches.get_one::<String>("NAME"),
            matches.get_one::<String>("DESCRIPTION"),
            matches.get_one::<String>("VERSION"),
            matches.get_one::<String>("AUTHOR"),
            progress_bar,
        )
        .await?;
    }
    Ok(())
}

pub async fn create_dat<P: AsRef<Path>>(
    directory: P,
    name: Option<&String>,
    description: Option<&String>,
    version: Option<&String>,
    author: Option<&String>,
    progress_bar: &ProgressBar,
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
        clrmamepros: Vec::new(),
        url: None,
    };
    let datfile_xml = DatfileXml {
        system: system_xml,
        games: Vec::new(),
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
