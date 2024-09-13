use super::config::HashAlgorithm;
use super::database::*;
use super::import_dats::reimport_orphan_romfiles;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use cdfs::{DirectoryEntry, ExtraAttributes, ISO9660Reader, ISODirectory, ISOFile, ISO9660};
use clap::{Arg, ArgAction, ArgMatches, Command};
use flate2::read::GzDecoder;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::str;
use strsim::jaro_winkler;

const GZIP_MAGIC: &[u8] = &[31, 139];
const IRD_MAGIC: &[u8] = &[51, 73, 82, 68];
const IRD_VERSION: u8 = 9;

pub fn subcommand() -> Command {
    Command::new("import-irds")
        .about("Parse and import PlayStation 3 IRD files into oxyromon")
        .arg(
            Arg::new("IRDS")
                .help("Set the IRD files to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("INFO")
                .short('i')
                .long("info")
                .help("Show the IRD information and exit")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of already imported IRD files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let ird_paths: Vec<&PathBuf> = matches.get_many::<PathBuf>("IRDS").unwrap().collect();
    let system = prompt_for_system_like(connection, None, "%PlayStation 3%").await?;
    let mut games = find_wanted_games_by_system_id(connection, system.id).await;

    for ird_path in ird_paths {
        let (irdfile, mut header) = parse_ird(ird_path).await?;

        progress_bar.println(format!("IRD Version: {}", &irdfile.version));
        progress_bar.println(format!("Game ID: {}", &irdfile.game_id));
        progress_bar.println(format!("Game Name: {}", &irdfile.game_name));
        progress_bar.println(format!("Update Version: {}", &irdfile.update_version));
        progress_bar.println(format!("Game Version: {}", &irdfile.game_version));
        progress_bar.println(format!("App Version: {}", &irdfile.app_version));
        progress_bar.println(format!("Regions: {}", &irdfile.regions_count));
        progress_bar.println(format!("Files: {}", &irdfile.files_count));

        if irdfile.version != IRD_VERSION {
            progress_bar.println("IRD version unsupported");
            continue;
        }

        if !matches.get_flag("INFO") {
            games.sort_by(|a, b| {
                jaro_winkler(&b.name.to_lowercase(), &irdfile.game_name.to_lowercase())
                    .partial_cmp(&jaro_winkler(
                        &a.name.to_lowercase(),
                        &irdfile.game_name.to_lowercase(),
                    ))
                    .unwrap()
            });
            if let Some(game) = prompt_for_game(&games, None)? {
                if game.jbfolder && !matches.get_flag("FORCE") {
                    progress_bar.println("IRD already exists");
                    continue;
                }
                import_ird(connection, progress_bar, game, &irdfile, &mut header).await?;
            }
        }
        progress_bar.println("");
    }

    compute_system_incompletion(connection, progress_bar, &system).await;

    Ok(())
}

pub async fn parse_ird<P: AsRef<Path>>(ird_path: &P) -> SimpleResult<(Irdfile, Vec<u8>)> {
    let mut reader = get_reader_sync(&ird_path)?;
    let mut magic = [0u8; 2];
    reader.read_exact(&mut magic).unwrap();
    drop(reader);

    let mut reader = if magic == GZIP_MAGIC {
        Box::new(GzDecoder::new(get_reader_sync(&ird_path)?)) as Box<dyn Read>
    } else {
        Box::new(get_reader_sync(&ird_path)?) as Box<dyn Read>
    };

    // parse magic
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).unwrap();

    if magic != IRD_MAGIC {
        bail!("Not an IRD file");
    }

    // parse version
    let mut version = [0u8];
    reader.read_exact(&mut version).unwrap();

    // parse game id
    let mut game_id = [0u8; 9];
    reader.read_exact(&mut game_id).unwrap();

    // parse game name
    let mut game_name_length = [0u8];
    reader.read_exact(&mut game_name_length).unwrap();
    let mut game_name = vec![0u8; game_name_length[0] as usize];
    reader.read_exact(&mut game_name).unwrap();

    // parse update version
    let mut update_version = [0u8; 4];
    reader.read_exact(&mut update_version).unwrap();

    // parse game version
    let mut game_version = [0u8; 5];
    reader.read_exact(&mut game_version).unwrap();

    // parse app version
    let mut app_version = [0u8; 5];
    reader.read_exact(&mut app_version).unwrap();

    // parse header
    let mut gzipped_header_length = [0u8; 4];
    reader.read_exact(&mut gzipped_header_length).unwrap();
    let gzipped_header_length = u32::from_le_bytes(gzipped_header_length);
    let mut gzipped_header = vec![0u8; gzipped_header_length as usize];
    reader.read_exact(&mut gzipped_header).unwrap();
    let mut gzipped_header_decoder = GzDecoder::new(io::Cursor::new(gzipped_header));
    let mut header: Vec<u8> = Vec::new();
    gzipped_header_decoder.read_to_end(&mut header).unwrap();

    // parse footer
    let mut footer_length = [0u8; 4];
    reader.read_exact(&mut footer_length).unwrap();
    let footer_length = u32::from_le_bytes(footer_length);
    let mut footer = vec![0u8; footer_length as usize];
    reader.read_exact(&mut footer).unwrap();

    // parse region hashes
    let mut regions_count = [0u8];
    reader.read_exact(&mut regions_count).unwrap();
    let regions_count = regions_count[0] as usize;
    let mut regions_hashes: Vec<String> = Vec::with_capacity(regions_count);
    while regions_hashes.len() < regions_count {
        let mut region_hash = [0u8; 16];
        reader.read_exact(&mut region_hash).unwrap();
        #[allow(clippy::format_collect)]
        regions_hashes.push(region_hash.iter().map(|b| format!("{:02x}", b)).collect());
    }

    // parse file hashes
    let mut files_count = [0u8; 4];
    reader.read_exact(&mut files_count).unwrap();
    let files_count = u32::from_le_bytes(files_count) as usize;
    let mut files_hashes: HashMap<u64, String> = HashMap::with_capacity(files_count);
    while files_hashes.len() < files_count {
        let mut sector = [0u8; 8];
        reader.read_exact(&mut sector).unwrap();
        let sector = u64::from_le_bytes(sector);
        let mut file_hash = [0u8; 16];
        reader.read_exact(&mut file_hash).unwrap();
        files_hashes.insert(
            sector,
            #[allow(clippy::format_collect)]
            file_hash.iter().map(|b| format!("{:02x}", b)).collect(),
        );
    }

    Ok((
        Irdfile {
            version: version[0],
            game_id: str::from_utf8(&game_id).unwrap().trim_end().to_string(),
            game_name: str::from_utf8(&game_name).unwrap().trim_end().to_string(),
            update_version: str::from_utf8(&update_version)
                .unwrap()
                .trim_end()
                .to_string(),
            game_version: str::from_utf8(&game_version)
                .unwrap()
                .trim_end()
                .to_string(),
            app_version: str::from_utf8(&app_version).unwrap().trim_end().to_string(),
            regions_count,
            regions_hashes,
            files_count,
            files_hashes,
        },
        header,
    ))
}

fn walk_directory<T: ISO9660Reader>(
    directory: &ISODirectory<T>,
    prefix: &str,
) -> HashMap<String, Vec<ISOFile<T>>> {
    let mut files: HashMap<String, Vec<ISOFile<T>>> = HashMap::new();
    let mut directories: Vec<ISODirectory<T>> = Vec::new();
    for entry in directory.contents() {
        if let Ok(entry) = entry {
            match entry {
                DirectoryEntry::Directory(directory) => {
                    if directory.identifier != "." && directory.identifier != ".." {
                        directories.push(directory);
                    }
                }
                DirectoryEntry::File(file) => {
                    let path = format!("{}{}", prefix, file.identifier);
                    if files.contains_key(&path) {
                        files.get_mut(&path).unwrap().push(file);
                    } else {
                        files.insert(path, Vec::from([file]));
                    }
                }
                _ => {}
            }
        }
    }
    for directory in directories {
        files.extend(walk_directory(
            &directory,
            &format!("{}{}/", prefix, directory.identifier),
        ));
    }
    files
}

pub async fn import_ird(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
    irdfile: &Irdfile,
    header: &mut [u8],
) -> SimpleResult<()> {
    let mut roms = find_roms_by_game_id_no_parents(connection, game.id).await;
    let parent_rom = prompt_for_rom(&mut roms, None)?;
    if parent_rom.is_none() {
        return Ok(());
    }

    let mut transaction = begin_transaction(connection).await;

    // parse ISO header
    let iso = ISO9660::new(Cursor::new(header)).unwrap();
    let files = walk_directory(iso.root(), "");

    if files.len() != irdfile.files_count {
        bail!(
            "Files count differ, expected {}, actual {}",
            irdfile.files_count,
            files.len()
        );
    }

    // convert files into roms
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    for file in files {
        let size = file.1.iter().map(|file| file.size()).sum::<u32>() as i64;
        let location = file.1.first().unwrap().header().extent_loc as u64;
        match find_rom_by_name_and_game_id(&mut transaction, &file.0, game.id).await {
            Some(rom) => {
                update_rom(
                    &mut transaction,
                    rom.id,
                    &file.0,
                    size,
                    irdfile.files_hashes.get(&location).unwrap(),
                    game.id,
                    parent_rom.as_ref().map(|rom| rom.id),
                )
                .await;
                if size != rom.size
                    || irdfile.files_hashes.get(&location).unwrap() != rom.md5.as_ref().unwrap()
                {
                    if let Some(romfile_id) = rom.romfile_id {
                        orphan_romfile_ids.push(romfile_id);
                        update_rom_romfile(&mut transaction, rom.id, None).await;
                    }
                }
                rom.id
            }
            None => {
                create_rom(
                    &mut transaction,
                    &file.0,
                    size,
                    irdfile.files_hashes.get(&location).unwrap(),
                    game.id,
                    parent_rom.as_ref().map(|rom| rom.id),
                )
                .await
            }
        };
    }

    // mark game as jbfolder
    update_game_jbfolder(&mut transaction, game.id, true).await;

    // reimport orphan romfiles
    if !orphan_romfile_ids.is_empty() {
        progress_bar.println("Processing orphan romfiles");
        reimport_orphan_romfiles(
            &mut transaction,
            progress_bar,
            game.system_id,
            orphan_romfile_ids,
            &HashAlgorithm::Md5,
        )
        .await?;
    }

    commit_transaction(transaction).await;

    Ok(())
}

#[cfg(test)]
mod test_ird;
