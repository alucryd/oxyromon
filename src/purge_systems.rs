use super::database::*;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::Command;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::time::Duration;

pub fn subcommand() -> Command {
    Command::new("purge-systems").about("Purge systems")
}

pub async fn main(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, false).await?;
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    for system in systems {
        purge_system(connection, progress_bar, &system).await?;
        progress_bar.println("");
    }
    Ok(())
}

async fn purge_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) -> SimpleResult<()> {
    progress_bar.println(&format!("Processing \"{}\"", system.name));

    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let trash_directory = get_trash_directory(connection, progress_bar, system).await?;

    for romfile in romfiles {
        let new_path = trash_directory.join(Path::new(&romfile.path).file_name().unwrap());
        rename_file(progress_bar, &romfile.path, &new_path, false).await?;
        update_romfile(
            connection,
            romfile.id,
            new_path.as_os_str().to_str().unwrap(),
            romfile.size as u64,
        )
        .await;
    }

    progress_bar.println("Deleting system");
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    delete_system_by_id(connection, system.id).await;

    progress_bar.disable_steady_tick();

    Ok(())
}
