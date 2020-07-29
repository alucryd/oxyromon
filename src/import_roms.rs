use super::chdman::*;
use super::checksum::*;
use super::config::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use diesel::SqliteConnection;
use indicatif::ProgressBar;
use std::ffi::OsString;
use std::path::PathBuf;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("import-roms")
        .about("Validates and imports ROM files into oxyromon")
        .arg(
            Arg::with_name("ROMS")
                .help("Sets the ROM files to import")
                .required(true)
                .multiple(true)
                .index(1),
        )
}

pub fn main<'a>(connection: &SqliteConnection, matches: &ArgMatches<'a>) -> SimpleResult<()> {
    let roms: Vec<String> = matches.values_of_lossy("ROMS").unwrap();
    let system = prompt_for_system(connection);
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    let header = find_header_by_system_id(connection, system.id);

    let system_directory = (get_rom_directory(connection)).join(&system.name);
    create_directory(&system_directory)?;

    for rom in roms {
        let rom_path = get_canonicalized_path(&rom)?;
        import_rom(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )?;
    }

    Ok(())
}

fn import_rom(
    connection: &SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let rom_extension = rom_path
        .extension()
        .unwrap()
        .to_str()
        .unwrap()
        .to_lowercase();

    progress_bar.println(&format!("Processing {:?}", rom_path.file_name().unwrap()));

    if ARCHIVE_EXTENSIONS.contains(&rom_extension.as_str()) {
        import_archive(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &rom_extension,
            &progress_bar,
        )?;
    } else if CHD_EXTENSION == rom_extension {
        import_chd(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )?;
    } else if CSO_EXTENSION == rom_extension {
        import_cso(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )?;
    } else {
        import_other(
            connection,
            &system_directory,
            &system,
            &header,
            &rom_path,
            &progress_bar,
        )?;
    }

    Ok(())
}

fn import_archive(
    connection: &SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    rom_extension: &str,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection)?;
    let sevenzip_infos = parse_archive(rom_path, &progress_bar)?;

    // archive contains a single file
    if sevenzip_infos.len() == 1 {
        let size: u64;
        let crc: String;
        let sevenzip_info = sevenzip_infos.get(0).unwrap();

        // system has a header or crc is absent
        if header.is_some() || sevenzip_info.crc == "" {
            let extracted_path = extract_files_from_archive(
                rom_path,
                &vec![&sevenzip_info.path],
                &tmp_directory.path().to_path_buf(),
                &progress_bar,
            )?
            .remove(0);
            let size_crc = get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1)?;
            remove_file(&extracted_path)?;
            size = size_crc.0;
            crc = size_crc.1;
        } else {
            size = sevenzip_info.size;
            crc = sevenzip_info.crc.clone();
        }

        let rom = match find_rom(connection, size, &crc, &system, &progress_bar) {
            Some(rom) => rom,
            None => return Ok(()),
        };

        let mut new_name = OsString::from(&rom.name);
        new_name.push(".");
        new_name.push(&rom_extension);
        let new_path = system_directory.join(&new_name);

        // move file inside archive if needed
        if sevenzip_info.path != rom.name {
            rename_file_in_archive(rom_path, &sevenzip_info.path, &rom.name, &progress_bar)?;
        }

        // move archive if needed
        move_file(rom_path, &new_path, &progress_bar)?;

        // persist in database
        create_or_update_romfile(connection, &new_path, &rom);

    // archive contains multiple files
    } else {
        for sevenzip_info in sevenzip_infos {
            let size: u64;
            let crc: String;

            let extracted_path = extract_files_from_archive(
                rom_path,
                &vec![&sevenzip_info.path],
                &tmp_directory.path().to_path_buf(),
                &progress_bar,
            )?
            .remove(0);

            // system has a header or crc is absent
            if header.is_some() || sevenzip_info.crc == "" {
                let size_crc =
                    get_file_size_and_crc(&extracted_path, &header, &progress_bar, 1, 1)?;
                size = size_crc.0;
                crc = size_crc.1;
            } else {
                size = sevenzip_info.size;
                crc = sevenzip_info.crc.clone();
            }

            let rom = match find_rom(connection, size, &crc, &system, &progress_bar) {
                Some(rom) => rom,
                None => {
                    remove_file(&extracted_path)?;
                    return Ok(());
                }
            };

            let mut new_path = system_directory.join(&rom.name);
            new_path.push(".");
            new_path.push(&rom_extension);

            // move file
            move_file(&extracted_path, &new_path, &progress_bar)?;

            // persist in database
            create_or_update_romfile(connection, &new_path, &rom);
        }

        // delete archive
        remove_file(rom_path)?;
    }

    Ok(())
}

fn import_chd(
    connection: &SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection)?;
    let mut cue_path = rom_path.clone();
    cue_path.set_extension(CUE_EXTENSION);

    if !cue_path.is_file() {
        progress_bar.println(&format!("Missing {:?}", cue_path.file_name().unwrap()));
        return Ok(());
    }

    let (size, crc) = get_file_size_and_crc(&cue_path, &header, &progress_bar, 1, 1)?;
    let cue_rom = match find_rom(connection, size, &crc, &system, &progress_bar) {
        Some(rom) => rom,
        None => return Ok(()),
    };

    let roms: Vec<Rom> = find_roms_by_game_id(connection, cue_rom.game_id)
        .into_iter()
        .filter(|rom| rom.id != cue_rom.id)
        .collect();

    let names_sizes: Vec<(&str, u64)> = roms
        .iter()
        .map(|rom| (rom.name.as_str(), rom.size as u64))
        .collect();
    let bin_paths = extract_chd(
        rom_path,
        &tmp_directory.path().to_path_buf(),
        &names_sizes,
        &progress_bar,
    )?;
    let mut crcs: Vec<String> = Vec::new();
    for (i, bin_path) in bin_paths.iter().enumerate() {
        let (_, crc) =
            get_file_size_and_crc(&bin_path, &header, &progress_bar, i, bin_paths.len())?;
        crcs.push(crc);
        remove_file(&bin_path)?;
    }

    if roms.iter().enumerate().any(|(i, rom)| crcs[i] != rom.crc) {
        progress_bar.println("CRC mismatch");
        return Ok(());
    }

    let new_meta_path = system_directory.join(&cue_rom.name);
    let mut new_file_path = new_meta_path.clone();
    new_file_path.set_extension(CHD_EXTENSION);

    // move cue and chd if needed
    move_file(&cue_path, &new_meta_path, &progress_bar)?;
    move_file(rom_path, &new_file_path, &progress_bar)?;

    // persist in database
    create_or_update_romfile(connection, &new_meta_path, &cue_rom);
    for rom in roms {
        create_or_update_romfile(connection, &new_file_path, &rom);
    }

    Ok(())
}

fn import_cso(
    connection: &SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let tmp_directory = create_tmp_directory(connection)?;
    let iso_path = extract_cso(rom_path, &tmp_directory.path().to_path_buf(), &progress_bar)?;
    let (size, crc) = get_file_size_and_crc(&iso_path, &header, &progress_bar, 1, 1)?;
    remove_file(&iso_path)?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar) {
        Some(rom) => rom,
        None => return Ok(()),
    };

    let mut new_file_path = system_directory.join(&rom.name);
    new_file_path.set_extension(CSO_EXTENSION);

    // move CSO if needed
    move_file(rom_path, &new_file_path, &progress_bar)?;

    // persist in database
    create_or_update_romfile(connection, &new_file_path, &rom);

    Ok(())
}

fn import_other(
    connection: &SqliteConnection,
    system_directory: &PathBuf,
    system: &System,
    header: &Option<Header>,
    rom_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let (size, crc) = get_file_size_and_crc(rom_path, &header, &progress_bar, 1, 1)?;
    let rom = match find_rom(connection, size, &crc, &system, &progress_bar) {
        Some(rom) => rom,
        None => return Ok(()),
    };

    let new_path = system_directory.join(&rom.name);

    // move file if needed
    move_file(rom_path, &new_path, &progress_bar)?;

    // persist in database
    create_or_update_romfile(connection, &new_path, &rom);

    Ok(())
}

fn find_rom(
    connection: &SqliteConnection,
    size: u64,
    crc: &str,
    system: &System,
    progress_bar: &ProgressBar,
) -> Option<Rom> {
    let rom: Rom;
    let mut roms = find_roms_by_size_and_crc_and_system(connection, size, crc, system.id);

    // abort if no match
    if roms.is_empty() {
        progress_bar.println("No match");
        return None;
    }

    // let user choose the rom if there are multiple matches
    if roms.len() == 1 {
        rom = roms.remove(0);
        progress_bar.println(&format!("Matches \"{}\"", rom.name));
    } else {
        rom = prompt_for_rom(&mut roms);
    }

    // abort if rom already has a file
    if rom.romfile_id.is_some() {
        let romfile = find_romfile_by_id(connection, rom.romfile_id.unwrap());
        if romfile.is_some() {
            let romfile = romfile.unwrap();
            progress_bar.println(&format!("Duplicate of \"{}\"", romfile.path));
            return None;
        }
    }

    Some(rom)
}

pub fn create_or_update_romfile(connection: &SqliteConnection, path: &PathBuf, rom: &Rom) {
    let romfile_input = RomfileInput {
        path: &String::from(path.as_os_str().to_str().unwrap()),
    };
    let file = find_romfile_by_path(connection, &romfile_input.path);
    let file_id = match file {
        Some(file) => {
            update_romfile(connection, &file, &romfile_input);
            file.id
        }
        None => create_romfile(connection, &romfile_input),
    };
    update_rom_romfile(connection, rom, file_id);
}

fn move_file(
    old_path: &PathBuf,
    new_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if old_path != new_path {
        progress_bar.println(&format!("Moving to {:?}", new_path));
        rename_file(old_path, new_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::set_directory;
    use super::super::database::*;
    use super::super::import_dats::import_dat;
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    embed_migrations!("migrations");

    #[test]
    fn test_import_other() {
        // given
        let connection = establish_connection(":memory:").unwrap();
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let dat_path = test_directory.join("test.dat");
        import_dat(&connection, &dat_path, false, &progress_bar).unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let system_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_path = tmp_directory.path().join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .unwrap();
        set_directory(
            &connection,
            "ROM_DIRECTORY",
            &system_directory.path().to_path_buf(),
        );

        let system = find_systems(&connection).remove(0);

        // when
        import_other(
            &connection,
            &system_directory.path().to_path_buf(),
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .unwrap();

        // then
        let mut games_roms_romfiles =
            find_games_roms_romfiles_with_romfile_by_system(&connection, &system);
        assert_eq!(games_roms_romfiles.len(), 1);

        let (game, mut roms_romfiles) = games_roms_romfiles.remove(0);
        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(roms_romfiles.len(), 1);

        let (rom, romfile) = roms_romfiles.remove(0);
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(
            romfile.path,
            system_directory
                .path()
                .join("Test Game (USA, Europe).rom")
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(Path::new(&romfile.path).is_file());
    }
}
