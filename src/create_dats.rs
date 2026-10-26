use super::common::to_hex;
use super::crc32::Crc32;
use super::mimetype::{ISO_EXTENSION, WUA_EXTENSION, WUX_EXTENSION};
use super::model::*;
use super::progress::*;
use super::util::*;
use super::wux::has_extension;
use anyhow::{Context, Result};
use chrono::prelude::*;
use clap::value_parser;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use md5::Md5;
use quick_xml::se;
use serde::Serialize;
use sha1::{Digest, Sha1};
use sqlx::sqlite::SqliteConnection;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;

pub const DOCTYPE: &[&str] = &[
    "<?xml version=\"1.0\"?>\n",
    "<!DOCTYPE datafile PUBLIC \"-//Logiqx//DTD ROM Management Datafile//EN\" \"http://www.logiqx.com/Dats/datafile.dtd\">\n",
];

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
        .arg(
            Arg::new("OUTPUT")
                .short('o')
                .long("output")
                .help("Customize the output directory")
                .required(false)
                .num_args(1),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let directories = matches
        .get_many::<PathBuf>("DIRECTORIES")
        .unwrap()
        .cloned()
        .collect::<Vec<PathBuf>>();
    for directory in directories {
        print_header(
            progress_bar,
            &format!(
                "Processing \"{}\"",
                directory.file_name().unwrap().to_str().unwrap()
            ),
        );
        create_dat(
            connection,
            progress_bar,
            directory,
            matches.get_one::<PathBuf>("OUTPUT"),
            matches.get_one::<String>("NAME"),
            matches.get_one::<String>("DESCRIPTION"),
            matches.get_one::<String>("VERSION"),
            matches.get_one::<String>("AUTHOR"),
            matches.get_one::<String>("URL"),
        )
        .await?;
        print_separator(progress_bar);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_dat<P: AsRef<Path>, Q: AsRef<Path>>(
    _connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    input_directory: P,
    output_directory: Option<Q>,
    name: Option<&String>,
    description: Option<&String>,
    version: Option<&String>,
    author: Option<&String>,
    url: Option<&String>,
) -> Result<()> {
    let system_name = input_directory
        .as_ref()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let system_date = format!("{}", Local::now().format("%Y%m%d-%H%M%S"));
    let system_version = version.map(String::to_owned).unwrap_or(system_date.clone());
    let system_xml = SystemXml {
        name: name.map(String::to_owned).unwrap_or(system_name.clone()),
        description: description
            .map(String::to_owned)
            .unwrap_or(system_name.clone()),
        version: system_version.clone(),
        date: Some(system_date.clone()),
        author: author.map(String::to_owned),
        url: url.map(String::to_owned),
        clrmamepros: vec![],
    };

    // One game per file, except that a WUX or a lossless WUA is listed as the
    // disc it holds, an ISO as Redump lists it, and a NUS title's folder, an
    // update or DLC as Nintendo's servers deliver it, as one game whose ROMs
    // are its files, as No-Intro lists them.
    let mut games_xml: Vec<GameXml> = vec![];
    let mut walker = WalkDir::new(&input_directory).into_iter();
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        // the update partitions lossless WUAs share, not games of their own
        if path.is_dir() && entry.file_name() == wux_rs::STORE_DIR {
            walker.skip_current_dir();
            continue;
        }
        if path.is_dir() && entry.depth() > 0 && wux_rs::tmd_path(path).is_some() {
            walker.skip_current_dir();
            let game_name = path.file_name().unwrap().to_str().unwrap().to_string();
            print_subheader(progress_bar, &format!("Processing \"{}\"", game_name));
            let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file())
                .collect();
            files.sort();
            let mut roms = Vec::with_capacity(files.len());
            for file in files {
                let rom_name = file.file_name().unwrap().to_str().unwrap().to_string();
                roms.push(rom_xml(progress_bar, &file, rom_name)?);
            }
            games_xml.push(game_xml(game_name, roms));
        } else if path.is_file() {
            let file_name = path.file_name().unwrap().to_str().unwrap().to_string();
            print_subheader(progress_bar, &format!("Processing \"{}\"", file_name));
            let game_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let is = |extension| has_extension(path, extension);
            let disc_name = format!("{game_name}.{ISO_EXTENSION}");
            // What a lossless WUA keeps of its disc needs no reading.
            let wua = match is(WUA_EXTENSION) {
                true => wux_rs::disc_info(path)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()))?,
                false => None,
            };
            let rom = match wua {
                Some(info) => disc_rom_xml(disc_name, &info),
                None if is(WUX_EXTENSION) => rom_xml(progress_bar, path, disc_name)?,
                None => rom_xml(progress_bar, path, file_name)?,
            };
            games_xml.push(game_xml(game_name, vec![rom]));
        }
    }

    let datfile_xml = DatfileXml {
        system: system_xml,
        games: games_xml,
        machines: vec![],
    };

    let mut buffer = String::new();
    let mut serializer = se::Serializer::new(&mut buffer);
    serializer.indent(' ', 2);
    datfile_xml
        .serialize(serializer)
        .context("Failed to serialize DAT file")?;

    let output_directory = match output_directory {
        Some(directory) => {
            if !directory.as_ref().is_dir() {
                create_directory(progress_bar, &directory.as_ref(), true).await?;
            }
            get_canonicalized_path(&directory.as_ref()).await?
        }
        None => input_directory.as_ref().parent().unwrap().to_path_buf(),
    };

    let mut dat_file = create_file(
        progress_bar,
        &output_directory.join(format!("{} ({}).dat", system_name, system_version)),
        false,
    )
    .await?;

    for doctype in DOCTYPE {
        dat_file
            .write_all(doctype.as_bytes())
            .await
            .context("Failed to write DAT file")?;
    }
    dat_file
        .write_all(buffer.as_bytes())
        .await
        .context("Failed to write DAT file")?;
    // tokio writes in the background: wait for it, or the DAT may be cut short
    dat_file.flush().await.context("Failed to write DAT file")?;

    Ok(())
}

fn game_xml(name: String, roms: Vec<RomXml>) -> GameXml {
    GameXml {
        description: name.clone(),
        name,
        roms,
        disks: vec![],
        isbios: false,
        isdevice: false,
        cloneof: None,
        romof: None,
        comment: None,
    }
}

/// A lossless WUA's disc as a ROM named `name`, from what the WUA keeps.
fn disc_rom_xml(name: String, info: &wux_rs::DiscInfo) -> RomXml {
    RomXml {
        name,
        size: info.size as i64,
        crc: Some(format!("{:08x}", info.crc32)),
        md5: Some(to_hex(&info.md5)),
        sha1: Some(to_hex(&info.sha1)),
        merge: None,
        status: None,
    }
}

/// `path` as a ROM named `name`, hashed in one pass; a WUX as the WUD it holds,
/// decompressed as it is read.
fn rom_xml(progress_bar: &ProgressBar, path: &Path, name: String) -> Result<RomXml> {
    let mut reader: Box<dyn Read> = match has_extension(path, WUX_EXTENSION) {
        true => Box::new(
            wux_rs::Image::open(path)
                .with_context(|| format!("Failed to open \"{}\"", path.display()))?,
        ),
        false => Box::new(open_file_sync(&path)?),
    };
    progress_bar.reset();
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_message("Computing CRC, MD5 and SHA1");
    let (mut crc, mut md5, mut sha1) = (Crc32::new(), Md5::new(), Sha1::new());
    let mut buffer = vec![0; 1 << 20];
    let mut size = 0;
    loop {
        let n = reader
            .read(&mut buffer)
            .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
        if n == 0 {
            break;
        }
        Digest::update(&mut crc, &buffer[..n]);
        Digest::update(&mut md5, &buffer[..n]);
        Digest::update(&mut sha1, &buffer[..n]);
        size += n as u64;
        progress_bar.inc(n as u64);
    }
    progress_bar.set_message("");
    Ok(RomXml {
        name,
        size: size as i64,
        crc: Some(to_hex(&crc.finalize())),
        md5: Some(to_hex(&md5.finalize())),
        sha1: Some(to_hex(&sha1.finalize())),
        merge: None,
        status: None,
    })
}

#[cfg(test)]
mod test_dat;
#[cfg(test)]
mod test_wua;
#[cfg(test)]
mod test_wux_and_nus;
