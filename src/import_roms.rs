use crate::chdman::{AsChd, ChdType};

use super::chdman;
use super::chdman::{ToRdsk, ToRiff};
use super::common::*;
use super::config::*;
use super::ctrtool;
use super::database::*;
use super::dolphin;
use super::dolphin::AsRvz;
use super::maxcso;
use super::maxcso::AsXso;
use super::mimetype::*;
use super::model::*;
use super::nsz;
use super::nsz::AsNsz;
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::{ArchiveFile, AsArchive};
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use strum::IntoEnumIterator;
use walkdir::WalkDir;

pub fn subcommand() -> Command {
    Command::new("import-roms")
        .about("Validate and import ROM files or directories into oxyromon")
        .arg(
            Arg::new("ROMS")
                .help("Set the ROM files or directories to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select systems by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("TRASH")
                .short('t')
                .long("trash")
                .help("Trash invalid ROM files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of existing ROM files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("UNATTENDED")
                .short('u')
                .long("unattended")
                .help("Skip ROM files that require human intervention")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("EXTRACT")
                .short('x')
                .long("extract")
                .help("Extract top-level archives before importing their contents")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let mut systems: Vec<System> = vec![];
    if let Some(system_names) = matches.get_many::<String>("SYSTEM") {
        for system_name in system_names {
            systems.append(&mut find_systems_by_name_like(connection, system_name).await);
        }
    }
    systems.dedup_by_key(|system| system.id);
    let mut systems: Vec<Option<System>> = systems.into_iter().map(Some).collect();
    if systems.is_empty() {
        systems.push(None);
    }

    let trash = matches.get_flag("TRASH");
    let force = matches.get_flag("FORCE");
    let unattended = matches.get_flag("UNATTENDED");

    let mut system_ids: HashSet<i64> = HashSet::new();
    let mut game_ids: HashSet<i64> = HashSet::new();

    for path in matches.get_many::<PathBuf>("ROMS").unwrap() {
        let tmp_directory = create_tmp_directory(connection).await?;
        let mut path = get_canonicalized_path(&path).await?;
        let mimetype = get_mimetype(&path).await?;
        if matches.get_flag("EXTRACT")
            && mimetype.is_some()
            && ARCHIVE_EXTENSIONS.contains(&mimetype.as_ref().unwrap().extension())
        {
            for archive_romfile in CommonRomfile::from_path(&path)?
                .as_archives(progress_bar)
                .await?
            {
                archive_romfile
                    .to_common(progress_bar, &tmp_directory.path())
                    .await?;
            }
            path = tmp_directory.path().to_path_buf();
        }
        for system in &systems {
            if let Some(system) = system {
                progress_bar.println(format!("Searching in \"{}\"", &system.name));
            }
            let header = match system {
                Some(system) => find_header_by_system_id(connection, system.id).await,
                None => None,
            };
            if path.is_dir() {
                if path.join(PS3_DISC_SFB).is_file() {
                    progress_bar.println(format!(
                        "Processing \"{}\"",
                        &path.file_name().unwrap().to_str().unwrap()
                    ));
                    match system.as_ref() {
                        Some(system) => {
                            import_jbfolder(connection, progress_bar, system, &path, unattended)
                                .await?
                        }
                        None => {
                            let system =
                                prompt_for_system_like(connection, None, "%PlayStation 3%").await?;
                            import_jbfolder(connection, progress_bar, &system, &path, unattended)
                                .await?;
                        }
                    }
                } else {
                    let walker = WalkDir::new(&path).into_iter();
                    for entry in walker.filter_map(|e| e.ok()) {
                        if entry.path().is_file() {
                            let (new_system_ids, new_game_ids) = import_rom(
                                connection,
                                progress_bar,
                                &system.as_ref(),
                                &header,
                                &entry.path(),
                                trash,
                                force,
                                unattended,
                            )
                            .await?;
                            system_ids.extend(new_system_ids);
                            game_ids.extend(new_game_ids);
                        }
                    }
                }
            } else {
                let (new_system_ids, new_game_ids) = import_rom(
                    connection,
                    progress_bar,
                    &system.as_ref(),
                    &header,
                    &path,
                    trash,
                    force,
                    unattended,
                )
                .await?;
                system_ids.extend(new_system_ids);
                game_ids.extend(new_game_ids);
            }
            progress_bar.println("");
        }
    }

    for system_id in system_ids {
        let system = find_system_by_id(connection, system_id).await;
        if system.arcade {
            compute_arcade_system_completion(connection, progress_bar, &system).await;
        } else {
            compute_system_completion(connection, progress_bar, &system).await;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn import_rom<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    header: &Option<Header>,
    path: &P,
    trash: bool,
    force: bool,
    unattended: bool,
) -> SimpleResult<(HashSet<i64>, HashSet<i64>)> {
    progress_bar.println(format!(
        "Processing \"{}\"",
        path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut transaction = begin_transaction(connection).await;
    let mut system_ids: HashSet<i64> = HashSet::new();
    let mut game_ids: HashSet<i64> = HashSet::new();

    // abort if the romfile is already in the database
    let romfile = CommonRomfile::from_path(path)?;
    if let Ok(relative_path) = romfile.get_relative_path(&mut transaction).await {
        if !force
            && find_romfile_by_path(
                &mut transaction,
                relative_path.as_os_str().to_str().unwrap(),
            )
            .await
            .is_some()
        {
            progress_bar.println("Already in database");
            return Ok((system_ids, game_ids));
        }
    }

    let mimetype = get_mimetype(&romfile.path).await?;
    let extension = match mimetype {
        Some(mimetype) => mimetype.extension().to_string(),
        None => romfile
            .path
            .extension()
            .unwrap_or(&OsString::new())
            .to_str()
            .unwrap()
            .to_lowercase(),
    };

    if ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
        if sevenzip::get_version().await.is_err() {
            progress_bar.println("Please install sevenzip");
            return Ok((system_ids, game_ids));
        }
        let (new_system_ids, new_game_ids) = import_archive(
            &mut transaction,
            progress_bar,
            system,
            header,
            &game_ids,
            romfile,
            &extension,
            trash,
            unattended,
        )
        .await?;
        system_ids.extend(new_system_ids);
        game_ids.extend(new_game_ids);
    } else if CHD_EXTENSION == extension {
        if chdman::get_version().await.is_err() {
            progress_bar.println("Please install chdman");
            return Ok((system_ids, game_ids));
        }
        if let Some(ids) = import_chd(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?
        {
            system_ids.insert(ids[0]);
            game_ids.insert(ids[1]);
        };
    } else if CIA_EXTENSION == extension {
        if ctrtool::get_version().await.is_err() {
            progress_bar.println("Please install ctrtool");
            return Ok((system_ids, game_ids));
        }
        let (new_system_ids, new_game_ids) = import_cia(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?;
        system_ids.extend(new_system_ids);
        game_ids.extend(new_game_ids);
    } else if CSO_EXTENSION == extension {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            return Ok((system_ids, game_ids));
        }
        if let Some(ids) = import_cso(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?
        {
            system_ids.insert(ids[0]);
            game_ids.insert(ids[1]);
        };
    } else if NSZ_EXTENSION == extension {
        if nsz::get_version().await.is_err() {
            progress_bar.println("Please install nsz");
            return Ok((system_ids, game_ids));
        }
        if let Some(ids) = import_nsz(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?
        {
            system_ids.insert(ids[0]);
            game_ids.insert(ids[1]);
        };
    } else if RVZ_EXTENSION == extension {
        if dolphin::get_version().await.is_err() {
            progress_bar.println("Please install dolphin-tool");
            return Ok((system_ids, game_ids));
        }
        if let Some(ids) = import_rvz(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?
        {
            system_ids.insert(ids[0]);
            game_ids.insert(ids[1]);
        };
    } else if ZSO_EXTENSION == extension {
        if maxcso::get_version().await.is_err() {
            progress_bar.println("Please install maxcso");
            return Ok((system_ids, game_ids));
        }
        if let Some(ids) = import_zso(
            &mut transaction,
            progress_bar,
            system,
            &game_ids,
            romfile,
            trash,
            unattended,
        )
        .await?
        {
            system_ids.insert(ids[0]);
            game_ids.insert(ids[1]);
        };
    } else if let Some(ids) = import_other(
        &mut transaction,
        progress_bar,
        system,
        header,
        &game_ids,
        romfile,
        trash,
        unattended,
    )
    .await?
    {
        system_ids.insert(ids[0]);
        game_ids.insert(ids[1]);
    };

    commit_transaction(transaction).await;

    Ok((system_ids, game_ids))
}

async fn import_jbfolder<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    path: &P,
    unattended: bool,
) -> SimpleResult<()> {
    let sfb_romfile_path = path.as_ref().join(PS3_DISC_SFB);

    // abort if the romfile is already in the database
    if find_romfile_by_path(connection, sfb_romfile_path.as_os_str().to_str().unwrap())
        .await
        .is_some()
    {
        progress_bar.println("Already in database");
        return Ok(());
    }

    let mut transaction = begin_transaction(connection).await;

    // find the correct game
    let original_romfile = CommonRomfile::from_path(&sfb_romfile_path)?;
    let (md5, size) = original_romfile
        .get_hash_and_size(&mut transaction, progress_bar, 1, 1, &HashAlgorithm::Md5)
        .await?;
    if let Some((sfb_rom, game)) = find_sfb_rom_by_md5(
        &mut transaction,
        progress_bar,
        system,
        size,
        &md5,
        unattended,
    )
    .await?
    {
        let system_directory = get_system_directory(&mut transaction, system).await?;

        let walker = WalkDir::new(path.as_ref()).into_iter();
        for entry in walker.filter_map(|e| e.ok()) {
            if entry.path().is_file() {
                progress_bar.println(format!(
                    "Processing \"{}\"",
                    &entry.path().as_os_str().to_str().unwrap()
                ));
                // force MD5 as IRD files only provide those
                let original_romfile = CommonRomfile::from_path(&entry.path())?;
                let (md5, size) = original_romfile
                    .get_hash_and_size(&mut transaction, progress_bar, 1, 1, &HashAlgorithm::Md5)
                    .await?;

                let rom: Option<&Rom>;
                let roms = find_roms_without_romfile_by_size_and_md5_and_parent_id(
                    &mut transaction,
                    size,
                    &md5,
                    sfb_rom.parent_id.unwrap(),
                )
                .await;

                // abort if no match
                if roms.is_empty() {
                    if count_roms_with_romfile_by_size_and_md5_and_parent_id(
                        &mut transaction,
                        size,
                        &md5,
                        sfb_rom.parent_id.unwrap(),
                    )
                    .await
                        > 0
                    {
                        progress_bar.println("Already imported");
                    } else {
                        progress_bar.println("No match");
                    }
                    continue;
                }

                // select the first rom if there is only one
                if roms.len() == 1 {
                    rom = roms.first();
                    progress_bar.println(format!("Matches \"{}\"", rom.as_ref().unwrap().name));
                // select the first rom that matches the file name if there multiple matches
                } else if let Some(rom_index) = roms.iter().position(|rom| {
                    entry
                        .path()
                        .as_os_str()
                        .to_str()
                        .unwrap()
                        .ends_with(&rom.name)
                }) {
                    rom = roms.get(rom_index);
                    progress_bar.println(format!("Matches \"{}\"", rom.as_ref().unwrap().name));
                } else {
                    // skip if unattended
                    if unattended {
                        progress_bar.println("Multiple matches, skipping");
                    }
                    // let the user select the rom if all else fails
                    rom = prompt_for_rom(&roms, None)?;
                }

                if let Some(rom) = rom {
                    // abort if rom already has a file
                    if rom.romfile_id.is_some() {
                        let romfile =
                            find_romfile_by_id(&mut transaction, rom.romfile_id.unwrap()).await;
                        progress_bar.println(format!("Duplicate of \"{}\"", romfile.path));
                        continue;
                    }

                    // put arcade roms in subdirectories as their names aren't unique
                    let new_path = system_directory.join(&game.name).join(&rom.name);

                    // move file if needed
                    rename_file(progress_bar, &entry.path(), &new_path, false).await?;

                    // persist in database
                    create_or_update_romfile(&mut transaction, &new_path, &[rom]).await?;

                    // remove directories if empty
                    let mut directory = entry.path().parent().unwrap();
                    while directory.read_dir().unwrap().next().is_none() {
                        remove_directory(progress_bar, &directory, false).await?;
                        if directory == entry.path() {
                            break;
                        }
                        directory = directory.parent().unwrap();
                    }
                }
            }
        }
    } else {
        progress_bar.println("No match");
    }

    commit_transaction(transaction).await;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn import_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    header: &Option<Header>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    romfile_extension: &str,
    trash: bool,
    unattended: bool,
) -> SimpleResult<(HashSet<i64>, HashSet<i64>)> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let archive_romfiles = romfile.as_archives(progress_bar).await?;
    let romfiles_count = archive_romfiles.len();

    let mut roms_games_systems_archive_romfiles: Vec<(
        Rom,
        Game,
        System,
        sevenzip::ArchiveRomfile,
    )> = vec![];
    let mut new_system_ids: HashSet<i64> = HashSet::new();
    let mut new_game_ids: HashSet<i64> = HashSet::new();

    for archive_romfile in archive_romfiles {
        progress_bar.println(format!(
            "Processing \"{} ({})\"",
            &archive_romfile.path,
            archive_romfile
                .romfile
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        ));

        let mut matched = false;
        for hash_algorithm in HashAlgorithm::iter() {
            let (hash, size) = match header {
                Some(header) => {
                    let romfile = archive_romfile
                        .to_common(progress_bar, &tmp_directory.path())
                        .await?;
                    let (hash, size) = romfile
                        .get_headered_hash_and_size(
                            connection,
                            progress_bar,
                            header,
                            1,
                            1,
                            &hash_algorithm,
                        )
                        .await?;
                    romfile.delete(progress_bar, true).await?;
                    (hash, size)
                }
                None => {
                    archive_romfile
                        .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                        .await?
                }
            };

            let mut game_names: Vec<&str> = vec![];
            game_names.push(
                archive_romfile
                    .romfile
                    .path
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap(),
            );

            let rom_path = Path::new(&archive_romfile.path);
            if let Some(path) = rom_path.parent() {
                if let Some(file_name) = path.file_name() {
                    let game_name = file_name.to_str().unwrap();
                    if !game_name.is_empty() {
                        game_names.push(game_name);
                    }
                }
            }
            let rom_name = rom_path.file_name().unwrap().to_str();

            let mut rom_game_system = find_rom_by_size_and_hash(
                connection,
                progress_bar,
                size,
                &hash,
                system,
                game_ids,
                game_names.as_slice(),
                rom_name,
                &hash_algorithm,
                unattended,
            )
            .await?;
            // MAME's CHD DATs have no size information
            if rom_game_system.is_none() && hash_algorithm == HashAlgorithm::Sha1 {
                rom_game_system = find_rom_by_size_and_hash(
                    connection,
                    progress_bar,
                    0,
                    &hash,
                    system,
                    game_ids,
                    game_names.as_slice(),
                    rom_name,
                    &hash_algorithm,
                    unattended,
                )
                .await?;
            }
            if let Some((rom, game, system)) = rom_game_system {
                matched = true;
                new_system_ids.insert(system.id);
                new_game_ids.insert(game.id);
                roms_games_systems_archive_romfiles.push((rom, game, system, archive_romfile));
                break;
            }
        }
        if !matched && trash && romfiles_count == 1 {
            move_to_trash(connection, progress_bar, &romfile).await?;
        }
    }

    // archive contains a single full game with no invalid file
    if roms_games_systems_archive_romfiles.len() == romfiles_count && new_game_ids.len() == 1 {
        let rom_ids: HashSet<i64> =
            find_roms_by_game_id_no_parents(connection, *new_game_ids.iter().last().unwrap())
                .await
                .into_par_iter()
                .map(|rom| rom.id)
                .collect();
        if rom_ids
            .difference(
                &roms_games_systems_archive_romfiles
                    .par_iter()
                    .map(|(rom, _, _, _)| rom.id)
                    .collect(),
            )
            .count()
            == 0
        {
            let game = &roms_games_systems_archive_romfiles.first().unwrap().1;
            let system = &roms_games_systems_archive_romfiles.first().unwrap().2;
            let system_directory = get_system_directory(connection, system).await?;

            for (rom, _game, _system, archive_romfile) in &roms_games_systems_archive_romfiles {
                if archive_romfile.path != rom.name {
                    archive_romfile.rename_file(progress_bar, &rom.name).await?;
                }
            }

            let new_path = match roms_games_systems_archive_romfiles.len() {
                1 => {
                    let rom = &roms_games_systems_archive_romfiles.first().unwrap().0;
                    let rom_extension = Path::new(&rom.name)
                        .extension()
                        .unwrap_or(&OsString::new())
                        .to_str()
                        .unwrap()
                        .to_lowercase();
                    if system.arcade || PS3_EXTENSIONS.contains(&rom_extension.as_str()) {
                        system_directory.join(format!("{}.{}", &game.name, &romfile_extension))
                    } else {
                        system_directory
                            .join(&rom.name)
                            .with_extension(romfile_extension)
                    }
                }
                _ => system_directory.join(format!("{}.{}", &game.name, &romfile_extension)),
            };

            // move file
            romfile.rename(progress_bar, &new_path, false).await?;

            // persist in database
            create_or_update_romfile(
                connection,
                &new_path,
                &roms_games_systems_archive_romfiles
                    .iter()
                    .map(|(rom, _, _, _)| rom)
                    .collect::<Vec<&Rom>>(),
            )
            .await?;

            return Ok((new_system_ids, new_game_ids));
        }
    }

    // all other cases
    for (rom, game, system, archive_romfile) in roms_games_systems_archive_romfiles {
        let original_romfile = archive_romfile
            .to_common(progress_bar, &tmp_directory.path())
            .await?;

        let system_directory = get_system_directory(connection, &system).await?;

        let new_path;
        // put arcade roms and JB folders in subdirectories
        if system.arcade || game.jbfolder {
            let game = find_game_by_id(connection, rom.game_id).await;
            new_path = system_directory.join(game.name).join(&rom.name);
        } else {
            new_path = system_directory.join(&rom.name);
        }

        // move file
        copy_file(progress_bar, &original_romfile.path, &new_path, false).await?;

        // persist in database
        create_or_update_romfile(connection, &new_path, &[&rom]).await?;
    }

    Ok((new_system_ids, new_game_ids))
}

#[allow(clippy::too_many_arguments)]
async fn import_chd(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let chd_romfile = romfile.as_chd().await?;

    match chd_romfile.chd_type {
        ChdType::Cd => {
            if chd_romfile.track_count > 1
                && chdman::get_version()
                    .await?
                    .as_str()
                    .cmp(chdman::MIN_SPLITBIN_VERSION)
                    == Ordering::Less
            {
                progress_bar.println(format!(
                    "Older chdman versions don't support splitbin, please update to {} or newer",
                    chdman::MIN_SPLITBIN_VERSION
                ));
                return Ok(None);
            }
            let cue_bin_romfile = chd_romfile
                .to_cue_bin(progress_bar, &tmp_directory.path(), None, &[], true)
                .await?;

            let mut roms_games_systems: Vec<(Rom, Game, System)> = vec![];
            for bin_romfile in &cue_bin_romfile.bin_romfiles {
                for hash_algorithm in HashAlgorithm::iter() {
                    let (hash, size) = bin_romfile
                        .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                        .await?;
                    if let Some(rom_game_system) = find_rom_by_size_and_hash(
                        connection,
                        progress_bar,
                        size,
                        &hash,
                        system,
                        game_ids,
                        &[],
                        None,
                        &hash_algorithm,
                        unattended,
                    )
                    .await?
                    {
                        roms_games_systems.push(rom_game_system);
                        break;
                    }
                }
            }
            let roms = roms_games_systems
                .iter()
                .map(|rom_game_system| &rom_game_system.0)
                .collect::<Vec<&Rom>>();
            let mut games = roms_games_systems
                .iter()
                .map(|rom_game_system| &rom_game_system.1)
                .collect::<Vec<&Game>>();
            games.dedup_by_key(|game| game.id);
            let mut systems = roms_games_systems
                .iter()
                .map(|rom_game_system| &rom_game_system.2)
                .collect::<Vec<&System>>();
            systems.dedup_by_key(|system| system.id);

            if roms.len() == cue_bin_romfile.bin_romfiles.len()
                && games.len() == 1
                && systems.len() == 1
            {
                let system = systems.first().unwrap();
                let system_directory = get_system_directory(connection, system).await?;

                let game = games.first().unwrap();
                let new_chd_path =
                    system_directory.join(format!("{}.{}", game.name, CHD_EXTENSION));

                // move chd if needed
                chd_romfile
                    .romfile
                    .rename(progress_bar, &new_chd_path, false)
                    .await?;

                // persist in database
                create_or_update_romfile(connection, &new_chd_path, &roms).await?;

                Ok(Some([system.id, game.id]))
            } else {
                progress_bar.println("CRC mismatch");
                if trash {
                    move_to_trash(connection, progress_bar, &chd_romfile.romfile).await?;
                }
                Ok(None)
            }
        }
        ChdType::Dvd => {
            let iso_romfile = chd_romfile
                .to_iso(progress_bar, &tmp_directory.path())
                .await?;
            for hash_algorithm in HashAlgorithm::iter() {
                let (hash, size) = iso_romfile
                    .romfile
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?;
                iso_romfile.romfile.delete(progress_bar, true).await?;
                if let Some((rom, game, system)) = find_rom_by_size_and_hash(
                    connection,
                    progress_bar,
                    size,
                    &hash,
                    system,
                    game_ids,
                    &[],
                    None,
                    &hash_algorithm,
                    unattended,
                )
                .await?
                {
                    let system_directory = get_system_directory(connection, &system).await?;

                    let new_chd_path = system_directory
                        .join(&rom.name)
                        .with_extension(CHD_EXTENSION);

                    // move CHD if needed
                    chd_romfile
                        .romfile
                        .rename(progress_bar, &new_chd_path, false)
                        .await?;

                    // persist in database
                    create_or_update_romfile(connection, &new_chd_path, &[&rom]).await?;

                    return Ok(Some([system.id, game.id]));
                }
            }
            if trash {
                move_to_trash(connection, progress_bar, &chd_romfile.romfile).await?;
            }
            Ok(None)
        }
        ChdType::Hd => {
            let rdsk_romfile = chd_romfile
                .to_rdsk(progress_bar, &tmp_directory.path())
                .await?;
            for hash_algorithm in HashAlgorithm::iter() {
                let (hash, size) = rdsk_romfile
                    .romfile
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?;
                rdsk_romfile.romfile.delete(progress_bar, true).await?;
                let mut rom_game_system = find_rom_by_size_and_hash(
                    connection,
                    progress_bar,
                    size,
                    &hash,
                    system,
                    game_ids,
                    &[],
                    None,
                    &hash_algorithm,
                    unattended,
                )
                .await?;
                // MAME's CHD DATs have no size information
                if rom_game_system.is_none() && hash_algorithm == HashAlgorithm::Sha1 {
                    rom_game_system = find_rom_by_size_and_hash(
                        connection,
                        progress_bar,
                        0,
                        &hash,
                        system,
                        game_ids,
                        &[],
                        None,
                        &hash_algorithm,
                        unattended,
                    )
                    .await?;
                }
                if let Some((rom, game, system)) = rom_game_system {
                    let system_directory = get_system_directory(connection, &system).await?;

                    let new_chd_path = system_directory
                        .join(&rom.name)
                        .with_extension(CHD_EXTENSION);

                    // move CHD if needed
                    chd_romfile
                        .romfile
                        .rename(progress_bar, &new_chd_path, false)
                        .await?;

                    // persist in database
                    create_or_update_romfile(connection, &new_chd_path, &[&rom]).await?;

                    return Ok(Some([system.id, game.id]));
                }
            }
            if trash {
                move_to_trash(connection, progress_bar, &chd_romfile.romfile).await?;
            }
            Ok(None)
        }
        ChdType::Ld => {
            let riff_romfile = chd_romfile
                .to_riff(progress_bar, &tmp_directory.path())
                .await?;
            for hash_algorithm in HashAlgorithm::iter() {
                let (hash, size) = riff_romfile
                    .romfile
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?;
                riff_romfile.romfile.delete(progress_bar, true).await?;
                let mut rom_game_system = find_rom_by_size_and_hash(
                    connection,
                    progress_bar,
                    size,
                    &hash,
                    system,
                    game_ids,
                    &[],
                    None,
                    &hash_algorithm,
                    unattended,
                )
                .await?;
                // MAME's CHD DATs have no size information
                if rom_game_system.is_none() && hash_algorithm == HashAlgorithm::Sha1 {
                    rom_game_system = find_rom_by_size_and_hash(
                        connection,
                        progress_bar,
                        0,
                        &hash,
                        system,
                        game_ids,
                        &[],
                        None,
                        &hash_algorithm,
                        unattended,
                    )
                    .await?;
                }
                if let Some((rom, game, system)) = rom_game_system {
                    let system_directory = get_system_directory(connection, &system).await?;

                    let new_chd_path = system_directory
                        .join(&rom.name)
                        .with_extension(CHD_EXTENSION);

                    // move CHD if needed
                    chd_romfile
                        .romfile
                        .rename(progress_bar, &new_chd_path, false)
                        .await?;

                    // persist in database
                    create_or_update_romfile(connection, &new_chd_path, &[&rom]).await?;

                    return Ok(Some([system.id, game.id]));
                }
            }
            if trash {
                move_to_trash(connection, progress_bar, &chd_romfile.romfile).await?;
            }
            Ok(None)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn import_cia(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<(HashSet<i64>, HashSet<i64>)> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let cia_infos = ctrtool::parse_cia(progress_bar, &romfile.path).await?;

    let mut roms_games_systems_cia_infos: Vec<(Rom, Game, System, &ctrtool::ArchiveInfo)> = vec![];
    let mut new_system_ids: HashSet<i64> = HashSet::new();
    let mut new_game_ids: HashSet<i64> = HashSet::new();

    let extracted_files =
        ctrtool::extract_files_from_cia(progress_bar, &romfile.path, &tmp_directory.path()).await?;

    for (cia_info, extracted_path) in cia_infos.iter().zip(extracted_files) {
        progress_bar.println(format!(
            "Processing \"{} ({})\"",
            &cia_info.path,
            romfile.path.file_name().unwrap().to_str().unwrap()
        ));

        let extracted_romfile = CommonRomfile::from_path(&extracted_path)?;
        for hash_algorithm in HashAlgorithm::iter() {
            let (hash, size) = extracted_romfile
                .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                .await?;
            extracted_romfile.delete(progress_bar, true).await?;

            let path = Path::new(&cia_info.path);
            let mut game_names: Vec<&str> = vec![];
            game_names.push(romfile.path.file_stem().unwrap().to_str().unwrap());
            if let Some(path) = path.parent() {
                let game_name = path.as_os_str().to_str().unwrap();
                if !game_name.is_empty() {
                    game_names.push(game_name);
                }
            }
            let rom_name = path.file_name().unwrap().to_str();

            if let Some((rom, game, system)) = find_rom_by_size_and_hash(
                connection,
                progress_bar,
                size,
                &hash,
                system,
                game_ids,
                game_names.as_slice(),
                rom_name,
                &hash_algorithm,
                unattended,
            )
            .await?
            {
                new_system_ids.insert(system.id);
                new_game_ids.insert(game.id);
                roms_games_systems_cia_infos.push((rom, game, system, cia_info));
                break;
            }
        }
    }

    // archive contains a single full game with no invalid file
    if roms_games_systems_cia_infos.len() == cia_infos.len() && new_game_ids.len() == 1 {
        let rom_ids: HashSet<i64> =
            find_roms_by_game_id_no_parents(connection, *new_game_ids.iter().last().unwrap())
                .await
                .into_par_iter()
                .map(|rom| rom.id)
                .collect();
        if rom_ids.is_superset(
            &roms_games_systems_cia_infos
                .par_iter()
                .map(|(rom, _, _, _)| rom.id)
                .collect(),
        ) {
            let game = &roms_games_systems_cia_infos.first().unwrap().1;
            let system = &roms_games_systems_cia_infos.first().unwrap().2;
            let system_directory = get_system_directory(connection, system).await?;

            let new_path = system_directory.join(format!("{}.cia", &game.name));

            // move file
            romfile.rename(progress_bar, &new_path, false).await?;

            // persist in database
            create_or_update_romfile(
                connection,
                &new_path,
                &roms_games_systems_cia_infos
                    .iter()
                    .map(|(rom, _, _, _)| rom)
                    .collect::<Vec<&Rom>>(),
            )
            .await?;

            return Ok((new_system_ids, new_game_ids));
        }
    }

    if trash {
        move_to_trash(connection, progress_bar, &romfile).await?;
    }

    Ok((new_system_ids, new_game_ids))
}

#[allow(clippy::too_many_arguments)]
async fn import_cso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    let cso_romfile = romfile.as_xso().await?;
    for hash_algorithm in HashAlgorithm::iter() {
        let (hash, size) = cso_romfile
            .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
            .await?;
        if let Some((rom, game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            system,
            game_ids,
            &[],
            None,
            &hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;
            let new_path = system_directory
                .join(&rom.name)
                .with_extension(CSO_EXTENSION);
            // move CSO if needed
            cso_romfile
                .romfile
                .rename(progress_bar, &new_path, false)
                .await?;
            // persist in database
            create_or_update_romfile(connection, &new_path, &[&rom]).await?;
            return Ok(Some([system.id, game.id]));
        }
    }
    if trash {
        move_to_trash(connection, progress_bar, &cso_romfile.romfile).await?;
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn import_nsz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    let nsz_romfile = romfile.as_nsz()?;
    for hash_algorithm in HashAlgorithm::iter() {
        let (hash, size) = nsz_romfile
            .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
            .await?;
        if let Some((rom, game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            system,
            game_ids,
            &[],
            None,
            &hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;
            let new_nsz_path = system_directory
                .join(&rom.name)
                .with_extension(NSZ_EXTENSION);
            // move NSZ if needed
            nsz_romfile
                .romfile
                .rename(progress_bar, &new_nsz_path, false)
                .await?;
            // persist in database
            create_or_update_romfile(connection, &new_nsz_path, &[&rom]).await?;
            return Ok(Some([system.id, game.id]));
        }
    }
    if trash {
        move_to_trash(connection, progress_bar, &nsz_romfile.romfile).await?;
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn import_rvz(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    let rvz_romfile = romfile.as_rvz()?;
    for hash_algorithm in HashAlgorithm::iter() {
        let (hash, size) = rvz_romfile
            .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
            .await?;
        if let Some((rom, game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            system,
            game_ids,
            &[],
            None,
            &hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;
            let new_rvz_path = system_directory
                .join(&rom.name)
                .with_extension(RVZ_EXTENSION);
            // move RVZ if needed
            rvz_romfile
                .romfile
                .rename(progress_bar, &new_rvz_path, false)
                .await?;
            // persist in database
            create_or_update_romfile(connection, &new_rvz_path, &[&rom]).await?;
            return Ok(Some([system.id, game.id]));
        }
    }
    if trash {
        move_to_trash(connection, progress_bar, &rvz_romfile.romfile).await?;
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn import_zso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    let zso_romfile = romfile.as_xso().await?;
    for hash_algorithm in HashAlgorithm::iter() {
        let (hash, size) = zso_romfile
            .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
            .await?;
        if let Some((rom, game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            system,
            game_ids,
            &[],
            None,
            &hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;
            let new_zso_path = system_directory
                .join(&rom.name)
                .with_extension(ZSO_EXTENSION);
            // move ZSO if needed
            zso_romfile
                .romfile
                .rename(progress_bar, &new_zso_path, false)
                .await?;
            // persist in database
            create_or_update_romfile(connection, &new_zso_path, &[&rom]).await?;
            return Ok(Some([system.id, game.id]));
        }
    }
    if trash {
        move_to_trash(connection, progress_bar, &zso_romfile.romfile).await?;
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn import_other(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &Option<&System>,
    header: &Option<Header>,
    game_ids: &HashSet<i64>,
    romfile: CommonRomfile,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<[i64; 2]>> {
    for hash_algorithm in HashAlgorithm::iter() {
        let (hash, size) = match header {
            Some(header) => {
                romfile
                    .get_headered_hash_and_size(
                        connection,
                        progress_bar,
                        header,
                        1,
                        1,
                        &hash_algorithm,
                    )
                    .await?
            }
            None => {
                romfile
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?
            }
        };
        if let Some((rom, game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            system,
            game_ids,
            &[],
            None,
            &hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;
            let new_path;
            // put arcade roms and JB folders in subdirectories
            if system.arcade || game.jbfolder {
                let game = find_game_by_id(connection, rom.game_id).await;
                new_path = system_directory.join(game.name).join(&rom.name);
            } else {
                new_path = system_directory.join(&rom.name);
            }
            // move file if needed
            romfile.rename(progress_bar, &new_path, false).await?;
            // persist in database
            create_or_update_romfile(connection, &new_path, &[&rom]).await?;
            return Ok(Some([system.id, game.id]));
        }
    }
    if trash {
        move_to_trash(connection, progress_bar, &romfile).await?;
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
async fn find_rom_by_size_and_hash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    size: u64,
    hash: &str,
    system: &Option<&System>,
    game_ids: &HashSet<i64>,
    game_names: &[&str],
    rom_name: Option<&str>,
    hash_algorithm: &HashAlgorithm,
    unattended: bool,
) -> SimpleResult<Option<(Rom, Game, System)>> {
    let mut rom_game_system: Option<(Rom, Game, System)> = None;
    let mut roms: Vec<Rom> = vec![];

    // first try matching with game and rom names
    if !game_names.is_empty() && rom_name.is_some() {
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_name_and_size_and_crc_and_game_names_and_system_id(
                        connection,
                        rom_name.unwrap(),
                        size,
                        hash,
                        game_names,
                        system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_name_and_size_and_crc_and_game_names(
                        connection,
                        rom_name.unwrap(),
                        size,
                        hash,
                        game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Md5 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_name_and_size_and_md5_and_game_names_and_system_id(
                        connection,
                        rom_name.unwrap(),
                        size,
                        hash,
                        game_names,
                        system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_name_and_size_and_md5_and_game_names(
                        connection,
                        rom_name.unwrap(),
                        size,
                        hash,
                        game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Sha1 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_name_and_size_and_sha1_and_game_names_and_system_id(
                    connection,
                    rom_name.unwrap(),
                    size,
                    hash,
                    game_names,
                    system.id,
                )
                .await
                .into_iter()
                .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_name_and_size_and_sha1_and_game_names(
                        connection,
                        rom_name.unwrap(),
                        size,
                        hash,
                        game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
        };
    }

    // then with game name only
    if roms.is_empty() && !game_names.is_empty() {
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_crc_and_game_names_and_system_id(
                        connection, size, hash, game_names, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_crc_and_game_names(
                        connection, size, hash, game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Md5 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_md5_and_game_names_and_system_id(
                        connection, size, hash, game_names, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_md5_and_game_names(
                        connection, size, hash, game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Sha1 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_sha1_and_game_names_and_system_id(
                        connection, size, hash, game_names, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_sha1_and_game_names(
                        connection, size, hash, game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
        };
    }

    // finally without any
    if roms.is_empty() {
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_crc_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_crc(connection, size, hash)
                        .await
                        .into_iter()
                        .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Md5 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_md5_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_md5(connection, size, hash)
                        .await
                        .into_iter()
                        .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Sha1 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_sha1_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_sha1(connection, size, hash)
                        .await
                        .into_iter()
                        .for_each(|rom| roms.push(rom))
                }
            }
        };
    }

    // abort if no match
    if roms.is_empty() {
        let rom_count = match hash_algorithm {
            HashAlgorithm::Crc => match system {
                Some(system) => {
                    count_roms_with_romfile_by_size_and_crc_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                }
                None => count_roms_with_romfile_by_size_and_crc(connection, size, hash).await,
            },
            HashAlgorithm::Md5 => match system {
                Some(system) => {
                    count_roms_with_romfile_by_size_and_md5_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                }
                None => count_roms_with_romfile_by_size_and_md5(connection, size, hash).await,
            },
            HashAlgorithm::Sha1 => match system {
                Some(system) => {
                    count_roms_with_romfile_by_size_and_sha1_and_system_id(
                        connection, size, hash, system.id,
                    )
                    .await
                }
                None => count_roms_with_romfile_by_size_and_sha1(connection, size, hash).await,
            },
        };
        if rom_count > 0 {
            progress_bar.println("Already imported");
        } else {
            progress_bar.println("No match");
        }
        return Ok(None);
    }

    // select the first rom if there is only one
    if roms.len() == 1 {
        let rom = roms.remove(0);
        let game = find_game_by_id(connection, rom.game_id).await;
        let system = find_system_by_id(connection, game.system_id).await;
        progress_bar.println(format!("Matches \"{}\"", &rom.name));
        rom_game_system = Some((rom, game, system));
    // select the first rom from a game that's been previously imported during this session
    } else if roms.iter().any(|rom| game_ids.contains(&rom.game_id)) {
        let rom = roms
            .into_iter()
            .find(|rom| game_ids.contains(&rom.game_id))
            .unwrap();
        let game = find_game_by_id(connection, rom.game_id).await;
        let system = find_system_by_id(connection, game.system_id).await;
        progress_bar.println(format!("Matches \"{}\"", &rom.name));
        rom_game_system = Some((rom, game, system));
    // skip if unattended
    } else if unattended {
        progress_bar.println("Multiple matches, skipping");
    } else if system.is_some() {
        let mut roms_games: Vec<(Rom, Game)> = vec![];
        for rom in roms {
            let game = find_game_by_id(connection, rom.game_id).await;
            roms_games.push((rom, game));
        }
        if let Some((rom, game)) = prompt_for_rom_game(&mut roms_games)? {
            let system = find_system_by_id(connection, game.system_id).await;
            rom_game_system = Some((rom, game, system));
        };
    } else {
        let mut roms_games_systems: Vec<(Rom, Game, System)> = vec![];
        for rom in roms {
            let game = find_game_by_id(connection, rom.game_id).await;
            let system = find_system_by_id(connection, game.system_id).await;
            roms_games_systems.push((rom, game, system));
        }
        rom_game_system = prompt_for_rom_game_system(&mut roms_games_systems)?;
    }

    // abort if rom already has a file
    if rom_game_system.is_some() && rom_game_system.as_ref().unwrap().0.romfile_id.is_some() {
        let romfile = find_romfile_by_id(
            connection,
            rom_game_system.as_ref().unwrap().0.romfile_id.unwrap(),
        )
        .await;
        progress_bar.println(format!("Duplicate of \"{}\"", romfile.path));
        return Ok(None);
    }

    Ok(rom_game_system)
}

async fn find_sfb_rom_by_md5(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    size: u64,
    md5: &str,
    unattended: bool,
) -> SimpleResult<Option<(Rom, Game)>> {
    let mut rom_game: Option<(Rom, Game)> = None;
    let mut roms = find_roms_without_romfile_by_name_and_size_and_md5_and_system_id(
        connection,
        PS3_DISC_SFB,
        size,
        md5,
        system.id,
    )
    .await;

    // abort if no match
    if roms.is_empty() {
        if count_roms_with_romfile_by_name_and_size_and_md5_and_system_id(
            connection,
            PS3_DISC_SFB,
            size,
            md5,
            system.id,
        )
        .await
            > 0
        {
            progress_bar.println("Already imported");
        } else {
            progress_bar.println("No match");
        }
        return Ok(None);
    }

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        let rom = roms.remove(0);
        let game = find_game_by_id(connection, rom.game_id).await;
        progress_bar.println(format!("Matches \"{}\"", &rom.name));
        rom_game = Some((rom, game));
    // skip if unattended
    } else if unattended {
        progress_bar.println("Multiple matches, skipping");
    } else {
        let mut roms_games: Vec<(Rom, Game)> = vec![];
        for rom in roms {
            let game = find_game_by_id(connection, rom.game_id).await;
            roms_games.push((rom, game));
        }
        rom_game = prompt_for_rom_game(&mut roms_games)?;
    }

    // abort if rom already has a file
    if rom_game.is_some() && rom_game.as_ref().unwrap().0.romfile_id.is_some() {
        let romfile =
            find_romfile_by_id(connection, rom_game.as_ref().unwrap().0.romfile_id.unwrap()).await;
        progress_bar.println(format!("Duplicate of \"{}\"", romfile.path));
        return Ok(None);
    }

    Ok(rom_game)
}

async fn create_or_update_romfile<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    romfile_path: &P,
    roms: &[&Rom],
) -> SimpleResult<()> {
    let romfile = CommonRomfile::from_path(&romfile_path)?;
    let relative_path = romfile.get_relative_path(connection).await?;
    let existing_romfile =
        find_romfile_by_path(connection, relative_path.as_os_str().to_str().unwrap()).await;
    let romfile_id = match existing_romfile {
        Some(existing_romfile) => {
            romfile.update(connection, existing_romfile.id).await?;
            existing_romfile.id
        }
        None => romfile.create(connection, RomfileType::Romfile).await?,
    };
    for rom in roms {
        update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
    }
    Ok(())
}

async fn move_to_trash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &CommonRomfile,
) -> SimpleResult<()> {
    let new_path = get_trash_directory(connection, None)
        .await?
        .join(romfile.path.file_name().unwrap());
    let new_romfile = romfile.rename(progress_bar, &new_path, false).await?;
    match find_romfile_by_path(connection, new_path.as_os_str().to_str().unwrap()).await {
        Some(romfile) => {
            new_romfile.update(connection, romfile.id).await?;
        }
        None => {
            new_romfile.create(connection, RomfileType::Romfile).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test_cia;
#[cfg(test)]
mod test_cso;
#[cfg(test)]
mod test_iso_chd;
#[cfg(test)]
mod test_multiple_tracks_chd;
#[cfg(test)]
mod test_original;
#[cfg(test)]
mod test_original_headered;
#[cfg(test)]
mod test_rvz;
#[cfg(test)]
mod test_sevenzip_multiple_files_full_game;
#[cfg(test)]
mod test_sevenzip_multiple_files_headered_mixed_games;
#[cfg(test)]
mod test_sevenzip_multiple_files_mixed_games;
#[cfg(test)]
mod test_sevenzip_multiple_files_partial_game;
#[cfg(test)]
mod test_sevenzip_single_file;
#[cfg(test)]
mod test_sevenzip_single_file_headered;
#[cfg(test)]
mod test_single_track_chd;
#[cfg(test)]
mod test_zip_single_file;
#[cfg(test)]
mod test_zip_single_file_extract;
#[cfg(test)]
mod test_zip_special_characters;
#[cfg(test)]
mod test_zso;
