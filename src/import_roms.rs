#[cfg(feature = "chd")]
use super::chdman;
#[cfg(feature = "chd")]
use super::chdman::{ChdRomfile, ToCueBin};
use super::common::*;
use super::config::*;
#[cfg(feature = "cia")]
use super::ctrtool;
use super::database::*;
#[cfg(feature = "rvz")]
use super::dolphin;
#[cfg(any(feature = "cso", feature = "zso"))]
use super::maxcso;
use super::model::*;
#[cfg(feature = "nsz")]
use super::nsz;
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::ArchiveFile;
use super::util::*;
use super::SimpleResult;
use cfg_if::cfg_if;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use strum::VariantNames;
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
                .help("Prompt for a system")
                .required(false)
                .action(ArgAction::SetTrue),
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
            Arg::new("HASH")
                .short('a')
                .long("hash")
                .help("Set the hash algorithm")
                .required(false)
                .num_args(1)
                .value_parser(PossibleValuesParser::new(HashAlgorithm::VARIANTS)),
        )
        .arg(
            Arg::new("UNATTENDED")
                .short('u')
                .long("unattended")
                .help("Skip ROM files that require human intervention")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let romfile_paths: Vec<&PathBuf> = matches.get_many::<PathBuf>("ROMS").unwrap().collect();
    let system = if matches.get_flag("SYSTEM") {
        Some(prompt_for_system(connection, None).await?)
    } else {
        None
    };
    let header = if system.is_some() {
        find_header_by_system_id(connection, system.as_ref().unwrap().id).await
    } else {
        None
    };

    let hash_algorithm = match matches.get_one::<String>("HASH").map(String::as_str) {
        Some("crc") => HashAlgorithm::Crc,
        Some("md5") => HashAlgorithm::Md5,
        Some(&_) | None => {
            match find_setting_by_key(connection, "HASH_ALGORITHM")
                .await
                .unwrap()
                .value
                .as_deref()
            {
                Some("crc") => HashAlgorithm::Crc,
                Some("md5") => HashAlgorithm::Md5,
                Some(&_) | None => bail!("Not possible"),
            }
        }
    };

    let trash = matches.get_flag("TRASH");
    let force = matches.get_flag("FORCE");
    let unattended = matches.get_flag("UNATTENDED");

    let mut system_ids: HashSet<i64> = HashSet::new();

    for romfile_path in romfile_paths {
        let romfile_path = get_canonicalized_path(&romfile_path).await?;
        if romfile_path.is_dir() {
            cfg_if! {
                if #[cfg(feature = "ird")] {
                    if romfile_path.join(PS3_DISC_SFB).is_file() {
                        progress_bar.println(format!(
                            "Processing \"{}\"",
                            &romfile_path.file_name().unwrap().to_str().unwrap()
                        ));
                        match system.as_ref() {
                            Some(system) => import_jbfolder(connection, progress_bar, system, &romfile_path, trash, unattended).await?,
                            None => {
                                let system = prompt_for_system_like(
                                    connection,
                                    None,
                                    "%PlayStation 3%",
                                )
                                .await?;
                                import_jbfolder(connection, progress_bar, &system, &romfile_path, trash, unattended).await?;
                            }
                        }
                    } else {
                        let walker = WalkDir::new(&romfile_path).into_iter();
                        for entry in walker.filter_map(|e| e.ok()) {
                            if entry.path().is_file() {
                                system_ids.extend(
                                    import_rom(
                                        connection,
                                        progress_bar,
                                        system.as_ref(),
                                        &header,
                                        &entry.path(),
                                        &hash_algorithm,
                                        trash,
                                        force,
                                        unattended,
                                    )
                                    .await?
                                );
                            }
                        }
                    }
                } else {
                    let walker = WalkDir::new(&romfile_path).into_iter();
                    for entry in walker.filter_map(|e| e.ok()) {
                        if entry.path().is_file() {
                            system_ids.extend(
                                import_rom(
                                    connection,
                                    progress_bar,
                                    system.as_ref(),
                                    &header,
                                    &entry.path(),
                                    &hash_algorithm,
                                    trash,
                                    force,
                                )
                                .await?
                            );
                        }
                    }
                }
            }
        } else {
            system_ids.extend(
                import_rom(
                    connection,
                    progress_bar,
                    system.as_ref(),
                    &header,
                    &romfile_path,
                    &hash_algorithm,
                    trash,
                    force,
                    unattended,
                )
                .await?,
            );
        }
        progress_bar.println("");
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
    system: Option<&System>,
    header: &Option<Header>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    force: bool,
    unattended: bool,
) -> SimpleResult<HashSet<i64>> {
    progress_bar.println(format!(
        "Processing \"{}\"",
        romfile_path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut transaction = begin_transaction(connection).await;
    let mut system_ids: HashSet<i64> = HashSet::new();

    // abort if the romfile is already in the database
    if !force
        && find_romfile_by_path(
            &mut transaction,
            romfile_path.as_ref().as_os_str().to_str().unwrap(),
        )
        .await
        .is_some()
    {
        progress_bar.println("Already in database");
        return Ok(system_ids);
    }

    let romfile_extension = romfile_path
        .as_ref()
        .extension()
        .unwrap_or(&OsString::new())
        .to_str()
        .unwrap()
        .to_lowercase();

    if ARCHIVE_EXTENSIONS.contains(&romfile_extension.as_str()) {
        if sevenzip::get_version().await.is_err() {
            progress_bar.println("Please install sevenzip");
            return Ok(system_ids);
        }
        system_ids.extend(
            import_archive(
                &mut transaction,
                progress_bar,
                system,
                header,
                &romfile_path,
                &romfile_extension,
                hash_algorithm,
                trash,
                unattended,
            )
            .await?,
        );
    } else if CHD_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "chd")] {
                if chdman::get_version().await.is_err() {
                    progress_bar.println("Please install chdman");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_chd(
                    &mut transaction,
                    progress_bar,
                    system,
                    header,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the CHD feature enabled");
            }
        }
    } else if CIA_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "cia")] {
                if ctrtool::get_version().await.is_err() {
                    progress_bar.println("Please install ctrtool");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_cia(
                    &mut transaction,
                    progress_bar,
                    system,
                    header,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the CIA feature enabled");
            }
        }
    } else if CSO_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "cso")] {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_cso(
                    &mut transaction,
                    progress_bar,
                    system,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the CSO feature enabled");
            }
        }
    } else if NSZ_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "nsz")] {
                if nsz::get_version().await.is_err() {
                    progress_bar.println("Please install nsz");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_nsz(
                    &mut transaction,
                    progress_bar,
                    system,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the NSZ feature enabled");
            }
        }
    } else if RVZ_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "rvz")] {
                if dolphin::get_version().await.is_err() {
                    progress_bar.println("Please install dolphin");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_rvz(
                    &mut transaction,
                    progress_bar,
                    system,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the RVZ feature enabled");
            }
        }
    } else if ZSO_EXTENSION == romfile_extension {
        cfg_if! {
            if #[cfg(feature = "zso")] {
                if maxcso::get_version().await.is_err() {
                    progress_bar.println("Please install maxcso");
                    return Ok(system_ids);
                }
                if let Some(system_id) = import_zso(
                    &mut transaction,
                    progress_bar,
                    system,
                    &romfile_path,
                    hash_algorithm,
                    trash,
                    unattended,
                )
                .await?
                {
                    system_ids.insert(system_id);
                };
            } else {
                progress_bar.println("Please rebuild with the ZSO feature enabled");
            }
        }
    } else if let Some(system_id) = import_other(
        &mut transaction,
        progress_bar,
        system,
        header,
        &romfile_path,
        hash_algorithm,
        trash,
        unattended,
    )
    .await?
    {
        system_ids.insert(system_id);
    };

    commit_transaction(transaction).await;

    Ok(system_ids)
}

#[cfg(feature = "ird")]
async fn import_jbfolder<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    folder_path: &P,
    trash: bool,
    unattended: bool,
) -> SimpleResult<()> {
    let sfb_romfile_path = folder_path.as_ref().join(PS3_DISC_SFB);

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
    let original_romfile = CommonRomfile {
        path: sfb_romfile_path,
    };
    let (md5, size) = original_romfile
        .get_hash_and_size(
            &mut transaction,
            progress_bar,
            &None,
            1,
            1,
            &HashAlgorithm::Md5,
        )
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

        let walker = WalkDir::new(folder_path.as_ref()).into_iter();
        for entry in walker.filter_map(|e| e.ok()) {
            if entry.path().is_file() {
                progress_bar.println(format!(
                    "Processing \"{}\"",
                    &entry.path().as_os_str().to_str().unwrap()
                ));
                // force MD5 as IRD files only provide those
                let original_romfile = CommonRomfile {
                    path: entry.path().to_path_buf(),
                };
                let (md5, size) = original_romfile
                    .get_hash_and_size(
                        &mut transaction,
                        progress_bar,
                        &None,
                        1,
                        1,
                        &HashAlgorithm::Md5,
                    )
                    .await?;

                let rom: Option<Rom>;
                let mut roms = find_roms_without_romfile_by_size_and_md5_and_parent_id(
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
                    rom = Some(roms.remove(0));
                    progress_bar.println(format!("Matches \"{}\"", rom.as_ref().unwrap().name));
                } else {
                    // select the first rom that matches the file name if there multiple matches
                    if let Some(rom_index) = roms.iter().position(|rom| {
                        entry
                            .path()
                            .as_os_str()
                            .to_str()
                            .unwrap()
                            .ends_with(&rom.name)
                    }) {
                        rom = Some(roms.remove(rom_index));
                    } else {
                        // skip if unattended
                        if unattended {
                            progress_bar.println("Multiple matches, skipping");
                        }
                        // let the user select the rom if all else fails
                        rom = prompt_for_rom(&mut roms, None)?;
                    }
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
                    create_or_update_romfile(&mut transaction, &new_path, &[rom]).await;

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
        if trash {
            move_to_trash(&mut transaction, progress_bar, &folder_path).await?;
        }
        return Ok(());
    }

    commit_transaction(transaction).await;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn import_archive<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    header: &Option<Header>,
    romfile_path: &P,
    romfile_extension: &str,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<HashSet<i64>> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let archive_romfiles = sevenzip::parse(progress_bar, romfile_path).await?;
    let romfiles_count = archive_romfiles.len();

    let mut roms_games_systems_archive_romfiles: Vec<(
        Rom,
        Game,
        System,
        sevenzip::ArchiveRomfile,
    )> = Vec::new();
    let mut game_ids: HashSet<i64> = HashSet::new();
    let mut system_ids: HashSet<i64> = HashSet::new();

    for archive_romfile in archive_romfiles {
        progress_bar.println(format!(
            "Processing \"{} ({})\"",
            &archive_romfile.file_path,
            romfile_path.as_ref().file_name().unwrap().to_str().unwrap()
        ));

        let (hash, size) = archive_romfile
            .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;

        let path = Path::new(&archive_romfile.path);
        let mut game_names: Vec<&str> = Vec::new();
        game_names.push(romfile_path.as_ref().file_stem().unwrap().to_str().unwrap());
        if let Some(path) = path.parent() {
            let game_name = path.as_os_str().to_str().unwrap();
            if !game_name.is_empty() {
                game_names.push(game_name);
            }
        }
        let rom_name = path.file_name().unwrap().to_str();

        match find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            &system,
            game_names,
            rom_name,
            hash_algorithm,
            unattended,
        )
        .await?
        {
            Some((rom, game, system)) => {
                game_ids.insert(game.id);
                roms_games_systems_archive_romfiles.push((rom, game, system, archive_romfile));
            }
            None => {
                if trash && romfiles_count == 1 {
                    move_to_trash(connection, progress_bar, romfile_path).await?;
                }
            }
        }
    }

    // archive contains a single full game with no invalid file
    if roms_games_systems_archive_romfiles.len() == romfiles_count && game_ids.len() == 1 {
        let game_id = game_ids.drain().last().unwrap();
        let rom_ids: HashSet<i64> = find_roms_by_game_id_no_parents(connection, game_id)
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
            system_ids.insert(system.id);
            let system_directory = get_system_directory(connection, system).await?;

            for (rom, _game, _system, archive_romfile) in &roms_games_systems_archive_romfiles {
                if archive_romfile.file_path != rom.name {
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
            rename_file(progress_bar, romfile_path, &new_path, false).await?;

            // persist in database
            create_or_update_romfile(
                connection,
                &new_path,
                &roms_games_systems_archive_romfiles
                    .into_iter()
                    .map(|(rom, _, _, _)| rom)
                    .collect::<Vec<Rom>>(),
            )
            .await;

            return Ok(system_ids);
        }
    }

    // all other cases
    for (rom, game, system, archive_romfile) in roms_games_systems_archive_romfiles {
        let original_romfile = archive_romfile
            .to_common(progress_bar, &tmp_directory.path())
            .await?;

        system_ids.insert(system.id);
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
        create_or_update_romfile(connection, &new_path, &[rom]).await;
    }

    Ok(system_ids)
}

#[cfg(feature = "chd")]
async fn import_chd<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    header: &Option<Header>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let tmp_directory = create_tmp_directory(connection).await?;

    let cue_path = romfile_path
        .as_ref()
        .to_path_buf()
        .with_extension(CUE_EXTENSION);

    if cue_path.is_file() {
        progress_bar.println("CUE file found");
        let cue_romfile = CommonRomfile {
            path: cue_path.clone(),
        };
        let (hash, size) = cue_romfile
            .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;
        if let Some((cue_rom, _game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            &system,
            Vec::new(),
            None,
            hash_algorithm,
            unattended,
        )
        .await?
        {
            let chd_romfile = ChdRomfile {
                path: romfile_path.as_ref().to_path_buf(),
                cue_path: Some(cue_path.clone()),
            };
            let roms: Vec<Rom> = find_roms_by_game_id_no_parents(connection, cue_rom.game_id)
                .await
                .into_iter()
                .filter(|rom| rom.id != cue_rom.id)
                .collect();
            let cue_bin_romfile = chd_romfile
                .to_cue_bin(
                    progress_bar,
                    &tmp_directory.path(),
                    &cue_romfile,
                    &roms.iter().collect::<Vec<&Rom>>(),
                    true,
                )
                .await?;

            let total = cue_bin_romfile.bin_romfiles.len();
            let mut hashes: Vec<String> = Vec::new();
            for (i, bin_romfile) in cue_bin_romfile.bin_romfiles.into_iter().enumerate() {
                let (hash, _) = bin_romfile
                    .get_hash_and_size(connection, progress_bar, header, i, total, hash_algorithm)
                    .await?;
                println!("{}", &hash);
                hashes.push(hash);
                bin_romfile.delete(progress_bar, true).await?;
            }

            if roms.iter().enumerate().any(|(i, rom)| {
                &hashes[i]
                    != match hash_algorithm {
                        HashAlgorithm::Crc => rom.crc.as_ref().unwrap(),
                        HashAlgorithm::Md5 => rom.md5.as_ref().unwrap(),
                        HashAlgorithm::Sha1 => rom.sha1.as_ref().unwrap(),
                    }
            }) {
                progress_bar.println("CRC mismatch");
                if trash {
                    move_to_trash(connection, progress_bar, romfile_path).await?;
                }
                return Ok(None);
            }

            let system_directory = get_system_directory(connection, &system).await?;

            let new_cue_path = system_directory.join(&cue_rom.name);
            let new_chd_path = new_cue_path.with_extension(CHD_EXTENSION);

            // move cue and chd if needed
            cue_romfile
                .rename(progress_bar, &new_cue_path, false)
                .await?;
            chd_romfile
                .as_common()?
                .rename(progress_bar, &new_chd_path, false)
                .await?;

            // persist in database
            create_or_update_romfile(connection, &new_cue_path, &[cue_rom]).await;
            create_or_update_romfile(connection, &new_chd_path, &roms).await;

            Ok(Some(system.id))
        } else {
            if trash {
                move_to_trash(connection, progress_bar, &cue_romfile.path).await?;
            }
            Ok(None)
        }
    } else {
        progress_bar.println("CUE file not found");
        let chd_romfile = ChdRomfile {
            path: romfile_path.as_ref().to_path_buf(),
            cue_path: None,
        };
        let iso_romfile = chd_romfile
            .to_iso(progress_bar, &tmp_directory.path())
            .await?;
        let (hash, size) = iso_romfile
            .as_common()?
            .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;
        iso_romfile.as_common()?.delete(progress_bar, true).await?;
        if let Some((rom, _game, system)) = find_rom_by_size_and_hash(
            connection,
            progress_bar,
            size,
            &hash,
            &system,
            Vec::new(),
            None,
            hash_algorithm,
            unattended,
        )
        .await?
        {
            let system_directory = get_system_directory(connection, &system).await?;

            let new_chd_path = system_directory
                .join(&rom.name)
                .with_extension(CHD_EXTENSION);

            // move CHD if needed
            rename_file(progress_bar, romfile_path, &new_chd_path, false).await?;

            // persist in database
            create_or_update_romfile(connection, &new_chd_path, &[rom]).await;

            Ok(Some(system.id))
        } else {
            if trash {
                move_to_trash(connection, progress_bar, romfile_path).await?;
            }
            Ok(None)
        }
    }
}

#[cfg(feature = "cia")]
async fn import_cia<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    header: &Option<Header>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let tmp_directory = create_tmp_directory(connection).await?;
    let cia_infos = ctrtool::parse_cia(progress_bar, romfile_path).await?;

    let mut roms_games_systems_cia_infos: Vec<(Rom, Game, System, &ctrtool::ArchiveInfo)> =
        Vec::new();
    let mut game_ids: HashSet<i64> = HashSet::new();

    let extracted_files =
        ctrtool::extract_files_from_cia(progress_bar, romfile_path, &tmp_directory.path()).await?;

    for (cia_info, extracted_path) in cia_infos.iter().zip(extracted_files) {
        progress_bar.println(format!(
            "Processing \"{} ({})\"",
            &cia_info.path,
            romfile_path.as_ref().file_name().unwrap().to_str().unwrap()
        ));

        let extracted_romfile = CommonRomfile {
            path: extracted_path,
        };
        let (hash, size) = extracted_romfile
            .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;
        extracted_romfile.delete(progress_bar, true).await?;

        let path = Path::new(&cia_info.path);
        let mut game_names: Vec<&str> = Vec::new();
        game_names.push(romfile_path.as_ref().file_stem().unwrap().to_str().unwrap());
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
            &system,
            game_names,
            rom_name,
            hash_algorithm,
            unattended,
        )
        .await?
        {
            game_ids.insert(game.id);
            roms_games_systems_cia_infos.push((rom, game, system, cia_info));
        }
    }

    // archive contains a single full game with no invalid file
    if roms_games_systems_cia_infos.len() == cia_infos.len() && game_ids.len() == 1 {
        let game_id = game_ids.drain().last().unwrap();
        let rom_ids: HashSet<i64> = find_roms_by_game_id_no_parents(connection, game_id)
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
            let system_id = system.id;
            let system_directory = get_system_directory(connection, system).await?;

            let new_path = system_directory.join(format!("{}.cia", &game.name));

            // move file
            rename_file(progress_bar, romfile_path, &new_path, false).await?;

            // persist in database
            create_or_update_romfile(
                connection,
                &new_path,
                &roms_games_systems_cia_infos
                    .into_iter()
                    .map(|(rom, _, _, _)| rom)
                    .collect::<Vec<Rom>>(),
            )
            .await;

            return Ok(Some(system_id));
        }
    }

    if trash {
        move_to_trash(connection, progress_bar, romfile_path).await?;
    }

    Ok(None)
}

#[cfg(feature = "cso")]
async fn import_cso<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let cso_romfile = maxcso::XsoRomfile::from_path(romfile_path)?;
    let (hash, size) = cso_romfile
        .get_hash_and_size(connection, progress_bar, &None, 1, 1, hash_algorithm)
        .await?;
    if let Some((rom, _game, system)) = find_rom_by_size_and_hash(
        connection,
        progress_bar,
        size,
        &hash,
        &system,
        Vec::new(),
        None,
        hash_algorithm,
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
            .as_common()?
            .rename(progress_bar, &new_path, false)
            .await?;
        // persist in database
        create_or_update_romfile(connection, &new_path, &[rom]).await;
        Ok(Some(system.id))
    } else {
        if trash {
            move_to_trash(connection, progress_bar, romfile_path).await?;
        }
        Ok(None)
    }
}

#[cfg(feature = "nsz")]
async fn import_nsz<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let nsz_romfile = nsz::NszRomfile::from_path(romfile_path)?;
    let (hash, size) = nsz_romfile
        .get_hash_and_size(connection, progress_bar, &None, 1, 1, hash_algorithm)
        .await?;
    if let Some((rom, _game, system)) = find_rom_by_size_and_hash(
        connection,
        progress_bar,
        size,
        &hash,
        &system,
        Vec::new(),
        None,
        hash_algorithm,
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
            .as_common()?
            .rename(progress_bar, &new_nsz_path, false)
            .await?;
        // persist in database
        create_or_update_romfile(connection, &new_nsz_path, &[rom]).await;
        Ok(Some(system.id))
    } else {
        if trash {
            move_to_trash(connection, progress_bar, romfile_path).await?;
        }
        Ok(None)
    }
}

#[cfg(feature = "rvz")]
async fn import_rvz<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let rvz_romfile = dolphin::RvzRomfile::from_path(romfile_path)?;
    let (hash, size) = rvz_romfile
        .get_hash_and_size(connection, progress_bar, &None, 1, 1, hash_algorithm)
        .await?;
    if let Some((rom, _game, system)) = find_rom_by_size_and_hash(
        connection,
        progress_bar,
        size,
        &hash,
        &system,
        Vec::new(),
        None,
        hash_algorithm,
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
            .as_common()?
            .rename(progress_bar, &new_rvz_path, false)
            .await?;
        // persist in database
        create_or_update_romfile(connection, &new_rvz_path, &[rom]).await;
        Ok(Some(system.id))
    } else {
        if trash {
            move_to_trash(connection, progress_bar, romfile_path).await?;
        }
        Ok(None)
    }
}

#[cfg(feature = "zso")]
async fn import_zso<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let zso_romfile = maxcso::XsoRomfile::from_path(romfile_path)?;
    let (hash, size) = zso_romfile
        .get_hash_and_size(connection, progress_bar, &None, 1, 1, hash_algorithm)
        .await?;
    if let Some((rom, _game, system)) = find_rom_by_size_and_hash(
        connection,
        progress_bar,
        size,
        &hash,
        &system,
        Vec::new(),
        None,
        hash_algorithm,
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
            .as_common()?
            .rename(progress_bar, &new_zso_path, false)
            .await?;
        // persist in database
        create_or_update_romfile(connection, &new_zso_path, &[rom]).await;
        Ok(Some(system.id))
    } else {
        if trash {
            move_to_trash(connection, progress_bar, romfile_path).await?;
        }
        Ok(None)
    }
}

async fn import_other<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: Option<&System>,
    header: &Option<Header>,
    romfile_path: &P,
    hash_algorithm: &HashAlgorithm,
    trash: bool,
    unattended: bool,
) -> SimpleResult<Option<i64>> {
    let original_romfile = CommonRomfile {
        path: romfile_path.as_ref().to_path_buf(),
    };
    let (hash, size) = original_romfile
        .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
        .await?;
    if let Some((rom, game, system)) = find_rom_by_size_and_hash(
        connection,
        progress_bar,
        size,
        &hash,
        &system,
        Vec::new(),
        None,
        hash_algorithm,
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
        original_romfile
            .rename(progress_bar, &new_path, false)
            .await?;
        // persist in database
        create_or_update_romfile(connection, &new_path, &[rom]).await;
        Ok(Some(system.id))
    } else {
        if trash {
            move_to_trash(connection, progress_bar, romfile_path).await?;
        }
        Ok(None)
    }
}

#[allow(clippy::too_many_arguments)]
async fn find_rom_by_size_and_hash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    size: u64,
    hash: &str,
    system: &Option<&System>,
    game_names: Vec<&str>,
    rom_name: Option<&str>,
    hash_algorithm: &HashAlgorithm,
    unattended: bool,
) -> SimpleResult<Option<(Rom, Game, System)>> {
    let mut rom_game_system: Option<(Rom, Game, System)> = None;
    let mut roms: Vec<Rom> = Vec::new();

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
                        &game_names,
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
                        &game_names,
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
                        &game_names,
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
                        &game_names,
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
                    &game_names,
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
                        &game_names,
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
                        connection,
                        size,
                        hash,
                        &game_names,
                        system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_crc_and_game_names(
                        connection,
                        size,
                        hash,
                        &game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Md5 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_md5_and_game_names_and_system_id(
                        connection,
                        size,
                        hash,
                        &game_names,
                        system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_md5_and_game_names(
                        connection,
                        size,
                        hash,
                        &game_names,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                }
            }
            HashAlgorithm::Sha1 => {
                if let Some(system) = system {
                    find_roms_without_romfile_by_size_and_sha1_and_game_names_and_system_id(
                        connection,
                        size,
                        hash,
                        &game_names,
                        system.id,
                    )
                    .await
                    .into_iter()
                    .for_each(|rom| roms.push(rom))
                } else {
                    find_roms_without_romfile_by_size_and_sha1_and_game_names(
                        connection,
                        size,
                        hash,
                        &game_names,
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

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        let rom = roms.remove(0);
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

#[cfg(feature = "ird")]
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
    roms: &[Rom],
) {
    let romfile = find_romfile_by_path(
        connection,
        romfile_path.as_ref().as_os_str().to_str().unwrap(),
    )
    .await;
    let romfile_id = match romfile {
        Some(romfile) => {
            update_romfile(
                connection,
                romfile.id,
                &romfile.path,
                romfile_path.as_ref().metadata().unwrap().len(),
            )
            .await;
            romfile.id
        }
        None => {
            create_romfile(
                connection,
                romfile_path.as_ref().as_os_str().to_str().unwrap(),
                romfile_path.as_ref().metadata().unwrap().len(),
            )
            .await
        }
    };
    for rom in roms {
        update_rom_romfile(connection, rom.id, Some(romfile_id)).await;
    }
}

async fn move_to_trash<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile_path: &P,
) -> SimpleResult<()> {
    let new_path = get_rom_directory(connection)
        .await
        .join("Trash")
        .join(romfile_path.as_ref().file_name().unwrap());
    rename_file(progress_bar, romfile_path, &new_path, false).await?;
    match find_romfile_by_path(connection, new_path.as_os_str().to_str().unwrap()).await {
        Some(romfile) => {
            update_romfile(
                connection,
                romfile.id,
                new_path.as_os_str().to_str().unwrap(),
                new_path.metadata().unwrap().len(),
            )
            .await;
        }
        None => {
            create_romfile(
                connection,
                new_path.as_os_str().to_str().unwrap(),
                new_path.metadata().unwrap().len(),
            )
            .await;
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "chd"))]
mod test_chd_multiple_tracks;
#[cfg(all(test, feature = "chd"))]
mod test_chd_multiple_tracks_without_cue_should_fail;
#[cfg(all(test, feature = "chd"))]
mod test_chd_single_track;
#[cfg(all(test, feature = "cia"))]
mod test_cia;
#[cfg(all(test, feature = "cso"))]
mod test_cso;
#[cfg(test)]
mod test_original;
#[cfg(test)]
mod test_original_headered;
#[cfg(all(test, feature = "rvz"))]
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
mod test_zip_single_file;
#[cfg(all(test, feature = "zso"))]
mod test_zso;
