use super::common::*;
use super::config::*;
use super::database::*;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

const BPS_MAGIC: &[u8] = &[66, 80, 83, 49];
const IPS_MAGIC: &[u8] = &[80, 65, 84, 67, 72];
const XDELTA_MAGIC: &[u8] = &[214, 195, 196];

pub enum PatchFormat {
    Bps,
    Ips,
    Xdelta,
}

pub fn subcommand() -> Command {
    Command::new("import-patches")
        .about("Import patch files into oxyromon")
        .arg(
            Arg::new("PATCHES")
                .help("Set the patch files to import")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("NAME")
                .short('n')
                .long("name")
                .help("Customize patch names")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of already imported patch files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let patch_paths: Vec<&PathBuf> = matches.get_many::<PathBuf>("IRDS").unwrap().collect();
    let name = matches.get_flag("NAME");
    let force = matches.get_flag("FORCE");
    for patch_path in patch_paths {
        if let Some(patch_format) = parse_patch(patch_path).await? {
            import_patch(
                connection,
                progress_bar,
                patch_path,
                &patch_format,
                name,
                force,
            )
            .await?;
        } else {
            progress_bar.println("Unsupported patch format");
        }
        progress_bar.println("");
    }
    Ok(())
}

pub async fn parse_patch<P: AsRef<Path>>(patch_path: &P) -> SimpleResult<Option<PatchFormat>> {
    let mut reader = get_reader_sync(&patch_path)?;
    let mut magic = [0u8; 5];
    reader.read_exact(&mut magic).unwrap();
    if &magic[0..3] == BPS_MAGIC {
        return Ok(Some(PatchFormat::Bps));
    }
    if &magic[0..4] == IPS_MAGIC {
        return Ok(Some(PatchFormat::Ips));
    }
    if &magic[0..2] == XDELTA_MAGIC {
        return Ok(Some(PatchFormat::Xdelta));
    }
    Ok(None)
}

pub async fn import_patch<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    patch_path: &P,
    patch_format: &PatchFormat,
    name: bool,
    force: bool,
) -> SimpleResult<()> {
    let system = prompt_for_system(connection, None).await?;
    let system_directory = get_system_directory(connection, &system).await?;
    let mut games = find_games_with_romfiles_by_system_id(connection, system.id).await;
    let game = match prompt_for_game(&mut games, None)? {
        Some(game) => game,
        None => {
            progress_bar.println("Skipping patch");
            return Ok(());
        }
    };
    let mut roms = find_roms_by_game_id_no_parents(connection, game.id).await;
    let rom = match prompt_for_rom(&mut roms, None)? {
        Some(rom) => rom,
        None => {
            progress_bar.println("Skipping patch");
            return Ok(());
        }
    };

    let patch_name = match name {
        true => match prompt_for_name("Please enter a name for the patch")? {
            Some(name) => name,
            None => {
                progress_bar.println("Skipping patch");
                return Ok(());
            }
        },
        false => patch_path
            .as_ref()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
    };

    let mut extension = match patch_format {
        PatchFormat::Bps => BPS_EXTENSION,
        PatchFormat::Ips => IPS_EXTENSION,
        PatchFormat::Xdelta => XDELTA_EXTENSION,
    }
    .to_string();

    let existing_patches = find_patches_by_rom_id(connection, rom.id).await;
    if existing_patches.len() > 0 {
        extension = format!("{}{}", extension, existing_patches.len());
    }

    let mut romfile_path = system_directory;
    if game.sorting == Sorting::OneRegion as i64 {
        romfile_path = romfile_path.join("1G1R");
    }
    romfile_path = romfile_path.join(&rom.name).with_extension(extension);

    if let Some(patch) = existing_patches
        .iter()
        .find(|patch| patch.name == patch_name)
    {
        if force {
            CommonRomfile::from_path(patch_path)?
                .rename(progress_bar, &romfile_path, false)
                .await?;
            update_romfile(
                connection,
                patch.romfile_id,
                romfile_path.as_os_str().to_str().unwrap(),
                romfile_path.metadata().unwrap().len(),
            )
            .await;
        } else {
            progress_bar.println("Name already exists, skipping patch");
        }
    } else {
        let mut transaction = begin_transaction(connection).await;
        CommonRomfile::from_path(patch_path)?
            .rename(progress_bar, &romfile_path, false)
            .await?;
        let romfile_id = create_romfile(
            &mut transaction,
            romfile_path.as_os_str().to_str().unwrap(),
            romfile_path.metadata().unwrap().len(),
            RomfileType::Romfile,
        )
        .await;
        create_patch(
            &mut transaction,
            &patch_name,
            existing_patches.len() as i64,
            rom.id,
            romfile_id,
        )
        .await;
        commit_transaction(transaction).await;
    }

    Ok(())
}

#[cfg(test)]
mod test_bps;
#[cfg(test)]
mod test_bps_ips;
#[cfg(test)]
mod test_ips;
#[cfg(test)]
mod test_xdelta;
