use super::common::*;
use super::database::*;
use super::mimetype::*;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use strum::{Display, EnumString};

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum PatchType {
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

pub async fn parse_patch<P: AsRef<Path>>(path: &P) -> SimpleResult<Option<PatchType>> {
    let mimetype = get_mimetype(path).await?;
    Ok(match mimetype {
        Some(mimetype) => PatchType::from_str(mimetype.extension()).ok(),
        None => None,
    })
}

pub async fn import_patch<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    patch_path: &P,
    patch_format: &PatchType,
    name: bool,
    force: bool,
) -> SimpleResult<()> {
    let system = prompt_for_system(connection, None).await?;
    let system_directory = get_system_directory(connection, &system).await?;
    let games = find_full_games_by_system_id(connection, system.id).await;
    let game = match prompt_for_game(&games, None)? {
        Some(game) => game,
        None => {
            progress_bar.println("Skipping patch");
            return Ok(());
        }
    };
    let roms = find_roms_by_game_id_no_parents(connection, game.id).await;
    let rom = match prompt_for_rom(&roms, None)? {
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
        PatchType::Bps => BPS_EXTENSION,
        PatchType::Ips => IPS_EXTENSION,
        PatchType::Xdelta => XDELTA_EXTENSION,
    }
    .to_string();

    let existing_patches = find_patches_by_rom_id(connection, rom.id).await;
    if !existing_patches.is_empty() {
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
                .await?
                .update(connection, progress_bar, patch.romfile_id)
                .await?;
        } else {
            progress_bar.println("Name already exists, skipping patch");
        }
    } else {
        let mut transaction = begin_transaction(connection).await;
        let romfile_id = CommonRomfile::from_path(patch_path)?
            .rename(progress_bar, &romfile_path, false)
            .await?
            .create(&mut transaction, progress_bar, RomfileType::Romfile)
            .await?;
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
