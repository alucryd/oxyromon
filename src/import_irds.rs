use super::checksum::*;
use super::database::*;
use super::import_dats::reimport_orphan_romfiles;
use super::isoinfo;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgMatches, Command};
use flate2::read::GzDecoder;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::str;
use std::str::FromStr;
use strsim::jaro_winkler;

const GZIP_MAGIC: &[u8] = &[31, 139];
const IRD_MAGIC: &[u8] = &[51, 73, 82, 68];
const IRD_VERSION: u8 = 9;

pub fn subcommand<'a>() -> Command<'a> {
    Command::new("import-irds")
        .about("Parse and import PlayStation 3 IRD files into oxyromon")
        .arg(
            Arg::new("IRDS")
                .help("Set the IRD files to import")
                .required(true)
                .multiple_values(true)
                .index(1)
                .allow_invalid_utf8(true),
        )
        .arg(
            Arg::new("INFO")
                .short('i')
                .long("info")
                .help("Show the IRD information and exit")
                .required(false),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of already imported IRD files")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let ird_paths: Vec<String> = matches.values_of_lossy("IRDS").unwrap();
    let system = prompt_for_system_like(
        connection,
        matches
            .value_of("SYSTEM")
            .map(|s| FromStr::from_str(s).expect("Failed to parse number")),
        "%PlayStation 3%",
    )
    .await?;
    let mut games = find_wanted_games_by_system_id(connection, system.id).await;

    for ird_path in ird_paths {
        let mut reader = get_reader_sync(&ird_path)?;
        let mut magic = [0u8; 2];
        reader.read_exact(&mut magic).unwrap();
        try_with!(reader.seek(io::SeekFrom::Start(0)), "Failed to seek file");

        let (irdfile, mut header) = if magic == GZIP_MAGIC {
            let mut decoder = GzDecoder::new(&mut reader);
            parse_ird(&mut decoder).await?
        } else {
            parse_ird(&mut reader).await?
        };

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

        if !matches.is_present("INFO") {
            games.sort_by(|a, b| {
                jaro_winkler(&b.name, &irdfile.game_name)
                    .partial_cmp(&jaro_winkler(&a.name, &irdfile.game_name))
                    .unwrap()
            });
            if let Some(game) = prompt_for_game(&games)? {
                if game.jbfolder && !matches.is_present("FORCE") {
                    progress_bar.println("IRD already exists");
                    continue;
                }
                import_ird(connection, progress_bar, game, &irdfile, &mut header).await?;
            }
        }
        progress_bar.println("");
    }

    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(100);
    progress_bar.set_message("Computing system completion");
    update_jbfolder_games_by_system_id_mark_incomplete(connection, system.id).await;
    update_system_mark_complete(connection, system.id).await;
    update_system_mark_incomplete(connection, system.id).await;

    Ok(())
}

pub async fn parse_ird<R: io::Read>(reader: &mut R) -> SimpleResult<(Irdfile, Vec<u8>)> {
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

pub async fn import_ird(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
    irdfile: &Irdfile,
    header: &mut Vec<u8>,
) -> SimpleResult<()> {
    let mut header_file = create_tmp_file(connection).await?;
    header_file.write_all(header).unwrap();

    let mut roms = find_roms_by_game_id_no_parents(connection, game.id).await;
    let parent_rom = prompt_for_rom(&mut roms, None)?;
    if parent_rom.is_none() {
        return Ok(());
    }

    let mut transaction = begin_transaction(connection).await;

    // parse ISO header
    let files = isoinfo::parse_iso(progress_bar, &header_file.path())?;

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
        match find_rom_by_name_and_game_id(&mut transaction, &file.0, game.id).await {
            Some(rom) => {
                update_rom(
                    &mut transaction,
                    rom.id,
                    &file.0,
                    file.1,
                    irdfile.files_hashes.get(&file.2).unwrap(),
                    game.id,
                    parent_rom.as_ref().map(|rom| rom.id),
                )
                .await;
                if file.1 != rom.size
                    || irdfile.files_hashes.get(&file.2).unwrap() != rom.md5.as_ref().unwrap()
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
                    file.1,
                    irdfile.files_hashes.get(&file.2).unwrap(),
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
