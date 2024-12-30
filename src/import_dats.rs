use super::common::*;
use super::config::*;
use super::database::*;
use super::import_roms::import_rom;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use quick_xml::de;
use rayon::prelude::*;
use rust_embed::RustEmbed;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::tosec::{TOSECName, TOSECToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use simple_error::SimpleError;
use sqlx::sqlite::SqliteConnection;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use vec_drain_where::VecDrainWhereExt;
use zip::ZipArchive;

#[derive(RustEmbed)]
#[folder = "data/"]
struct Assets;

pub fn subcommand() -> Command {
    Command::new("import-dats")
        .about("Parse and import Logiqx DAT files into oxyromon")
        .arg(
            Arg::new("DATS")
                .help("Set the DAT files to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("INFO")
                .short('i')
                .long("info")
                .help("Show the DAT information and exit")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("SKIP_HEADER")
                .short('s')
                .long("skip-header")
                .help("Skip parsing the header even if the system has one")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of outdated DAT files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("NAME")
                .short('n')
                .long("name")
                .help("Customize the system name")
                .required(false)
                .num_args(1),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let (zip_paths, mut dat_paths): (Vec<PathBuf>, Vec<PathBuf>) = matches
        .get_many::<PathBuf>("DATS")
        .unwrap()
        .cloned()
        .partition(|path| {
            path.extension().unwrap().to_str().unwrap().to_lowercase() == ZIP_EXTENSION
        });

    let tmp_directory = create_tmp_directory(connection).await?;
    for zip_path in zip_paths {
        let mut reader = get_reader_sync(&zip_path)?;
        let mut zip_archive = try_with!(ZipArchive::new(&mut reader), "Failed to read ZIP");
        try_with!(zip_archive.extract(&tmp_directory), "Failed to extract ZIP");
        for file_name in zip_archive.file_names() {
            if file_name.ends_with(DAT_EXTENSION) {
                dat_paths.push(tmp_directory.path().join(file_name));
            }
        }
    }

    let custom_name = matches.get_one::<String>("NAME");

    if custom_name.is_some() {
        if dat_paths.len() > 1 {
            progress_bar.println("Custom system name requires a single DAT file");
            return Ok(());
        }
        if find_system_by_name(connection, custom_name.unwrap())
            .await
            .is_some()
        {
            progress_bar.println("Custom system name must not match a known system name");
            return Ok(());
        }
    }

    for dat_path in dat_paths {
        progress_bar.println(format!(
            "Processing \"{}\"",
            &dat_path.file_name().unwrap().to_str().unwrap()
        ));
        let (datfile_xml, detector_xml) = parse_dat(
            progress_bar,
            &get_canonicalized_path(&dat_path).await?,
            matches.get_flag("SKIP_HEADER"),
        )
        .await?;
        if !matches.get_flag("INFO") {
            import_dat(
                connection,
                progress_bar,
                &datfile_xml,
                &detector_xml,
                custom_name,
                matches.get_flag("FORCE"),
            )
            .await?;
        }
        progress_bar.println("");
    }

    Ok(())
}

pub async fn parse_dat<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    dat_path: &P,
    skip_header: bool,
) -> SimpleResult<(DatfileXml, Option<DetectorXml>)> {
    let datfile_xml: DatfileXml = try_with!(
        de::from_reader(&mut get_reader_sync(dat_path)?),
        "Failed to deserialize DAT file"
    );

    // print information
    progress_bar.println(format!("System: {}", datfile_xml.system.name));
    progress_bar.println(format!("Version: {}", datfile_xml.system.version));
    if !datfile_xml.machines.is_empty() {
        progress_bar.println(format!("Games: {}", datfile_xml.machines.len()));
    } else {
        progress_bar.println(format!("Games: {}", datfile_xml.games.len()));
    }

    let mut detector_xml = None;
    if !skip_header {
        if let Some(clr_mame_pro_xml) = &datfile_xml
            .system
            .clrmamepros
            .iter()
            .find(|clrmamepro| clrmamepro.header.is_some())
        {
            progress_bar.println("Processing header");
            if let Some(header_file_name) = &clr_mame_pro_xml.header {
                let header_file_path = dat_path.as_ref().parent().unwrap().join(header_file_name);
                if header_file_path.is_file() {
                    let header_file = open_file_sync(&header_file_path.as_path())?;
                    let reader = io::BufReader::new(header_file);
                    detector_xml = de::from_reader(reader).expect("Failed to parse header file");
                } else {
                    let header_file = Assets::get(header_file_name).unwrap();
                    detector_xml = de::from_str(str::from_utf8(header_file.data.as_ref()).unwrap())
                        .expect("Failed to parse header file");
                }
            }
        }
    };

    Ok((datfile_xml, detector_xml))
}

pub async fn import_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    datfile_xml: &DatfileXml,
    detector_xml: &Option<DetectorXml>,
    custom_name: Option<&String>,
    force: bool,
) -> SimpleResult<()> {
    progress_bar.println("Processing system");

    let mut transaction = begin_transaction(connection).await;

    // persist system
    let system_id = match create_or_update_system(
        &mut transaction,
        progress_bar,
        &datfile_xml.system,
        custom_name,
        !datfile_xml.machines.is_empty(),
        force,
    )
    .await
    {
        Some(system_id) => system_id,
        None => return Ok(()),
    };

    // persist header
    if let Some(detector_xml) = detector_xml {
        create_or_update_header(&mut transaction, detector_xml, system_id).await;
    }

    // persist games
    progress_bar.set_style(get_count_progress_style());
    if !datfile_xml.machines.is_empty() {
        progress_bar.set_length(datfile_xml.machines.len() as u64);
    } else {
        progress_bar.set_length(datfile_xml.games.len() as u64);
    }

    let mut orphan_romfile_ids: Vec<i64> = vec![];
    progress_bar.println("Deleting old games");
    orphan_romfile_ids
        .append(&mut delete_old_games(&mut transaction, &datfile_xml.games, system_id).await);
    progress_bar.println("Processing games");
    orphan_romfile_ids.append(
        &mut create_or_update_games(
            &mut transaction,
            if !datfile_xml.machines.is_empty() {
                &datfile_xml.machines
            } else {
                &datfile_xml.games
            },
            system_id,
            !datfile_xml.machines.is_empty(),
            progress_bar,
        )
        .await?,
    );

    progress_bar.set_style(get_none_progress_style());
    progress_bar.reset();

    // reimport orphan romfiles
    if !orphan_romfile_ids.is_empty() {
        progress_bar.println("Processing orphan romfiles");
        orphan_romfile_ids.dedup();
        reimport_orphan_romfiles(
            &mut transaction,
            progress_bar,
            system_id,
            orphan_romfile_ids,
        )
        .await?;
    }

    // create necessary directories
    let system = find_system_by_id(&mut transaction, system_id).await;
    get_system_directory(&mut transaction, &system).await?;
    get_trash_directory(&mut transaction, Some(&system)).await?;

    // update games and systems completion
    if system.arcade {
        compute_arcade_system_completion(&mut transaction, progress_bar, &system).await;
        compute_arcade_system_incompletion(&mut transaction, progress_bar, &system).await;
    } else {
        compute_system_completion(&mut transaction, progress_bar, &system).await;
        compute_system_incompletion(&mut transaction, progress_bar, &system).await;
    }

    commit_transaction(transaction).await;

    Ok(())
}

fn get_regions_from_game_name(name: &str) -> SimpleResult<String> {
    match NoIntroName::try_parse(name) {
        Ok(v) => {
            for token in v.iter() {
                if let NoIntroToken::Region(_, regions) = token {
                    return Ok(Region::to_normalized_region_string(regions));
                }
            }
        }
        Err(_) => match TOSECName::try_parse(name) {
            Ok(v) => {
                for token in v.iter() {
                    if let TOSECToken::Region(_, regions) = token {
                        return Ok(Region::to_normalized_region_string(regions));
                    }
                }
            }
            Err(e) => {
                return Err(SimpleError::with("Failed to parse name", e));
            }
        },
    };
    Ok(String::from(""))
}

async fn create_or_update_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_xml: &SystemXml,
    custom_name: Option<&String>,
    arcade: bool,
    force: bool,
) -> Option<i64> {
    match find_system_by_name(connection, &system_xml.name).await {
        Some(system) => {
            if is_update(progress_bar, &system.version, &system_xml.version) || force {
                update_system_from_xml(connection, system.id, system_xml, custom_name, arcade)
                    .await;
                Some(system.id)
            } else {
                None
            }
        }
        None => Some(create_system_from_xml(connection, system_xml, custom_name, arcade).await),
    }
}

async fn create_or_update_header(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let header_id = match find_header_by_system_id(connection, system_id).await {
        Some(header) => {
            update_header_from_xml(connection, header.id, detector_xml, system_id).await;
            delete_rules_by_header_id(connection, header.id).await;
            header.id
        }
        None => create_header_from_xml(connection, detector_xml, system_id).await,
    };
    for data_xml in &detector_xml.rule.data {
        create_rule_from_xml(connection, data_xml, header_id).await;
    }
}

async fn create_or_update_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
    arcade: bool,
    progress_bar: &ProgressBar,
) -> SimpleResult<Vec<i64>> {
    let mut orphan_romfile_ids: Vec<i64> = vec![];
    let (mut parent_games_xml, mut child_games_xml): (Vec<&GameXml>, Vec<&GameXml>) = games_xml
        .par_iter()
        .partition(|game_xml| game_xml.cloneof.is_none() && game_xml.romof.is_none());
    for game_xml in &parent_games_xml {
        let game = find_game_by_name_and_bios_and_system_id(
            connection,
            &game_xml.name,
            game_xml.isbios,
            system_id,
        )
        .await;
        let roms = [
            game_xml.roms.iter().collect::<Vec<&RomXml>>(),
            game_xml.disks.iter().collect::<Vec<&RomXml>>(),
        ]
        .concat();
        let mut regions = String::new();
        if !arcade {
            match get_regions_from_game_name(&game_xml.name) {
                Ok(s) => regions.push_str(&s),
                Err(err) => {
                    progress_bar.println(err.as_str());
                    progress_bar.inc(1);
                    continue;
                }
            }
        }
        let game_id = match game {
            Some(game) => {
                update_game_from_xml(
                    connection, game.id, game_xml, &regions, system_id, None, None,
                )
                .await;
                game.id
            }
            None => {
                create_game_from_xml(connection, game_xml, &regions, system_id, None, None).await
            }
        };
        if !roms.is_empty() {
            orphan_romfile_ids.append(
                &mut create_or_update_roms(
                    connection,
                    progress_bar,
                    &roms,
                    game_xml.isbios,
                    game_id,
                )
                .await,
            );
        }
        orphan_romfile_ids.append(&mut delete_old_roms(connection, &roms, game_id).await);
        progress_bar.inc(1)
    }
    while !child_games_xml.is_empty() {
        let parent_game_names: Vec<&str> = parent_games_xml
            .par_iter()
            .map(|game_xml| game_xml.name.as_str())
            .collect();
        parent_games_xml = child_games_xml
            .e_drain_where(|&mut child_game_xml| {
                parent_game_names.contains(
                    &child_game_xml
                        .cloneof
                        .as_ref()
                        .or(child_game_xml.romof.as_ref())
                        .unwrap()
                        .as_str(),
                )
            })
            .collect();
        for game_xml in &parent_games_xml {
            let game = find_game_by_name_and_bios_and_system_id(
                connection,
                &game_xml.name,
                game_xml.isbios,
                system_id,
            )
            .await;
            let roms = [
                game_xml.roms.iter().collect::<Vec<&RomXml>>(),
                game_xml.disks.iter().collect::<Vec<&RomXml>>(),
            ]
            .concat();
            // sometimes romof refers to games that aren't bioses
            let parent_game = match game_xml.cloneof.as_ref() {
                Some(name) => {
                    find_game_by_name_and_bios_and_system_id(connection, name, false, system_id)
                        .await
                }
                None => match game_xml.romof.as_ref() {
                    Some(name) => {
                        find_game_by_name_and_bios_and_system_id(connection, name, false, system_id)
                            .await
                    }
                    None => None,
                },
            };
            let bios_game: Option<Game> = match game_xml.romof.as_ref() {
                Some(name) => {
                    find_game_by_name_and_bios_and_system_id(connection, name, true, system_id)
                        .await
                }
                None => None,
            };
            let mut regions = String::new();
            if !arcade {
                match get_regions_from_game_name(&game_xml.name) {
                    Ok(s) => regions.push_str(&s),
                    Err(err) => {
                        progress_bar.println(err.as_str());
                        progress_bar.inc(1);
                        continue;
                    }
                }
            }
            let game_id = match game {
                Some(game) => {
                    update_game_from_xml(
                        connection,
                        game.id,
                        game_xml,
                        &regions,
                        system_id,
                        parent_game.map(|game| game.id),
                        bios_game.map(|game| game.id),
                    )
                    .await;
                    game.id
                }
                None => {
                    create_game_from_xml(
                        connection,
                        game_xml,
                        &regions,
                        system_id,
                        parent_game.map(|game| game.id),
                        bios_game.map(|game| game.id),
                    )
                    .await
                }
            };
            if !roms.is_empty() {
                orphan_romfile_ids.append(
                    &mut create_or_update_roms(
                        connection,
                        progress_bar,
                        &roms,
                        game_xml.isbios,
                        game_id,
                    )
                    .await,
                );
            }
            orphan_romfile_ids.append(&mut delete_old_roms(connection, &roms, game_id).await);
            progress_bar.inc(1)
        }
    }
    Ok(orphan_romfile_ids)
}

async fn create_or_update_roms(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_xml: &[&RomXml],
    mut bios: bool,
    game_id: i64,
) -> Vec<i64> {
    let mut orphan_romfile_ids: Vec<i64> = vec![];
    for rom_xml in roms_xml {
        // skip nodump roms
        if rom_xml.status.is_some() && rom_xml.status.as_ref().unwrap() == "nodump" {
            continue;
        }
        // find parent rom if needed
        let mut parent_id = None;
        if rom_xml.merge.is_some() {
            let game = find_game_by_id(connection, game_id).await;
            let parent_rom = if let Some(parent_id) = game.parent_id {
                find_rom_by_size_and_crc_and_game_id(
                    connection,
                    rom_xml.size,
                    rom_xml.crc.as_ref().unwrap(),
                    parent_id,
                )
                .await
            } else {
                None
            };
            let bios_rom = if let Some(bios_id) = game.bios_id {
                find_rom_by_size_and_crc_and_game_id(
                    connection,
                    rom_xml.size,
                    rom_xml.crc.as_ref().unwrap(),
                    bios_id,
                )
                .await
            } else {
                None
            };
            if let Some(rom) = parent_rom.or(bios_rom) {
                bios = rom.bios;
                parent_id = rom.parent_id;
            } else {
                progress_bar.println(format!(
                    "Rom \"{}\" not found in game \"{}\" parent/bios, please fix your DAT file",
                    rom_xml.name, game.name
                ));
            }
        }
        match find_rom_by_name_and_game_id(connection, &rom_xml.name, game_id).await {
            Some(rom) => {
                update_rom_from_xml(connection, rom.id, rom_xml, bios, game_id, parent_id).await;
                if rom_xml.size != rom.size
                    || rom_xml.crc.as_ref().unwrap() != rom.crc.as_ref().unwrap()
                {
                    if let Some(romfile_id) = rom.romfile_id {
                        orphan_romfile_ids.push(romfile_id);
                        update_rom_romfile(connection, rom.id, None).await;
                    }
                }
                rom.id
            }
            None => create_rom_from_xml(connection, rom_xml, bios, game_id, parent_id).await,
        };
    }
    orphan_romfile_ids
}

async fn delete_old_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
) -> Vec<i64> {
    let mut orphan_romfile_ids: Vec<i64> = vec![];
    let game_names_xml: Vec<&String> = games_xml.iter().map(|game_xml| &game_xml.name).collect();
    let games: Vec<Game> = find_games_by_system_id(connection, system_id)
        .await
        .into_par_iter()
        .filter(|game| !game_names_xml.contains(&&game.name))
        .collect();
    for game in games {
        orphan_romfile_ids.extend(
            find_roms_by_game_id_no_parents(connection, game.id)
                .await
                .into_iter()
                .filter_map(|rom| rom.romfile_id),
        );
        delete_game_by_name_and_system_id(connection, &game.name, system_id).await;
    }
    orphan_romfile_ids
}

async fn delete_old_roms(
    connection: &mut SqliteConnection,
    roms_xml: &[&RomXml],
    game_id: i64,
) -> Vec<i64> {
    let rom_names_romfile_ids: Vec<(String, Option<i64>)> =
        find_roms_by_game_id_no_parents(connection, game_id)
            .await
            .into_par_iter()
            .map(|rom| (rom.name, rom.romfile_id))
            .collect();
    let mut orphan_romfile_ids: Vec<i64> = vec![];
    for (rom_name, rom_romfile_id) in &rom_names_romfile_ids {
        if !roms_xml.iter().any(|rom_xml| &rom_xml.name == rom_name) {
            delete_rom_by_name_and_game_id(connection, rom_name, game_id).await;
            if let Some(romfile_id) = rom_romfile_id {
                orphan_romfile_ids.push(*romfile_id);
            }
        }
    }
    orphan_romfile_ids
}

pub async fn reimport_orphan_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_id: i64,
    orphan_romfile_ids: Vec<i64>,
) -> SimpleResult<()> {
    let system = find_system_by_id(connection, system_id).await;
    let header = find_header_by_system_id(connection, system_id).await;
    for romfile_id in orphan_romfile_ids {
        let romfile = find_romfile_by_id(connection, romfile_id)
            .await
            .as_common(connection)
            .await?;
        delete_romfile_by_id(connection, romfile_id).await;
        if romfile.path.is_file() {
            let (_, game_ids) = import_rom(
                connection,
                progress_bar,
                &Some(&system),
                &header,
                &romfile.path,
                false,
                false,
                false,
                &None,
            )
            .await?;
            if game_ids.is_empty() {
                let new_path = get_trash_directory(connection, Some(&system))
                    .await?
                    .join(romfile.path.file_name().unwrap().to_str().unwrap());
                romfile
                    .rename(progress_bar, &new_path, false)
                    .await?
                    .create(connection, progress_bar, RomfileType::Romfile)
                    .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod test_dat;
#[cfg(test)]
mod test_dat_custom_name;
#[cfg(test)]
mod test_dat_custom_name_revert;
#[cfg(test)]
mod test_dat_headered;
#[cfg(test)]
mod test_dat_headered_duplicate_clrmamepro;
#[cfg(test)]
mod test_dat_headered_embedded;
#[cfg(test)]
mod test_dat_headered_skipped_header;
#[cfg(test)]
mod test_dat_mame;
#[cfg(test)]
mod test_dat_mame_chd;
#[cfg(test)]
mod test_dat_mame_mixed;
#[cfg(test)]
mod test_dat_outdated_forced;
#[cfg(test)]
mod test_dat_outdated_should_do_nothing;
#[cfg(test)]
mod test_dat_parent_clone;
#[cfg(test)]
mod test_dat_updated;
#[cfg(test)]
mod test_dat_updated_orphan_archive;
#[cfg(test)]
mod test_dat_updated_orphan_archive_mismatch;
#[cfg(test)]
mod test_dat_updated_orphan_chd;
#[cfg(test)]
mod test_dat_updated_orphan_chd_mismatch;
#[cfg(test)]
mod test_regions_france_germany;
#[cfg(test)]
mod test_regions_world;
