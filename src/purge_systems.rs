use super::common::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

pub fn subcommand() -> Command {
    Command::new("purge-systems")
        .about("Purge systems")
        .arg(
            Arg::new("EMPTY")
                .short('e')
                .long("empty")
                .help("Only list empty systems for selection")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select systems by name")
                .required(false)
                .action(ArgAction::Append),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let systems = match matches.get_many::<String>("SYSTEM") {
        Some(system_names) => resolve_systems_by_name_like(connection, system_names).await,
        None => {
            prompt_for_systems(connection, None, false, matches.get_flag("EMPTY"), false).await?
        }
    };
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    for system in systems {
        print_header(progress_bar, &format!("Purging \"{}\"", system.name));
        purge_system(connection, progress_bar, &system).await?;
        print_separator(progress_bar);
    }
    Ok(())
}

pub async fn purge_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) -> Result<()> {
    print_header(progress_bar, &format!("Processing \"{}\"", system.name));

    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let mut emptied_directories: HashSet<PathBuf> = HashSet::new();

    for romfile in romfiles {
        let common_romfile = romfile.as_common(connection).await?;
        if let Some(directory) = common_romfile.path.parent() {
            emptied_directories.insert(directory.to_path_buf());
        }
        let new_path = get_trash_path(connection, Some(system), &common_romfile.path).await?;
        common_romfile
            .rename(progress_bar, &new_path, false)
            .await?
            .update(connection, progress_bar, romfile.id)
            .await?;
    }

    print_action(progress_bar, "Deleting system");
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    delete_system_by_id(connection, system.id).await;

    progress_bar.disable_steady_tick();

    // the system is gone, so its game directories are no longer kept
    let kept_directories = get_kept_directories(connection).await?;
    for directory in &emptied_directories {
        remove_empty_directories(directory, &kept_directories).await?;
    }

    Ok(())
}

#[cfg(test)]
mod test_purge_systems;
#[cfg(test)]
mod test_purge_systems_arcade;
#[cfg(test)]
mod test_purge_systems_by_name;
