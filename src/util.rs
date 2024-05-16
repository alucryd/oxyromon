use super::config::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use num_traits::FromPrimitive;
use rayon::prelude::*;
use regex::Regex;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;
use tempfile::TempDir;
use tokio::fs;
use tokio::fs::File;

lazy_static! {
    static ref SYSTEM_NAME_REGEX: Regex =
        Regex::new(r"^(Non-Redump - |Unofficial - )?([^()]+)( \(.*\))?$").unwrap();
}

pub async fn get_canonicalized_path<P: AsRef<Path>>(path: &P) -> SimpleResult<PathBuf> {
    let canonicalized_path = try_with!(
        path.as_ref().canonicalize(),
        "Failed to get canonicalized path for \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(canonicalized_path)
}

pub async fn open_file<P: AsRef<Path>>(path: &P) -> SimpleResult<File> {
    let file = try_with!(
        File::open(path.as_ref()).await,
        "Failed to open \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(file)
}

pub fn open_file_sync<P: AsRef<Path>>(path: &P) -> SimpleResult<std::fs::File> {
    let file = try_with!(
        std::fs::File::open(path.as_ref()),
        "Failed to open \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(file)
}

pub fn get_reader_sync<P: AsRef<Path>>(
    path: &P,
) -> SimpleResult<std::io::BufReader<std::fs::File>> {
    let f = open_file_sync(path)?;
    Ok(std::io::BufReader::new(f))
}

pub async fn create_file<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
    quiet: bool,
) -> SimpleResult<File> {
    if !quiet {
        progress_bar.println(format!(
            "Creating \"{}\"",
            path.as_ref().as_os_str().to_str().unwrap()
        ));
    }
    let file = try_with!(
        File::create(path).await,
        "Failed to create \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(file)
}

pub async fn create_tmp_file(connection: &mut SqliteConnection) -> SimpleResult<NamedTempFile> {
    let tmp_file = try_with!(
        NamedTempFile::new_in(get_tmp_directory(connection).await),
        "Failed to create temp file"
    );
    Ok(tmp_file)
}

pub async fn copy_file<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    old_path: &P,
    new_path: &Q,
    quiet: bool,
) -> SimpleResult<()> {
    if old_path.as_ref() != new_path.as_ref() {
        let new_directory = new_path.as_ref().parent().unwrap();
        if !new_directory.is_dir() {
            create_directory(progress_bar, &new_directory, quiet).await?;
        }
        if !quiet {
            progress_bar.println(format!(
                "Copying to \"{}\"",
                new_path.as_ref().as_os_str().to_str().unwrap()
            ));
        }
        try_with!(
            fs::copy(old_path, new_path).await,
            "Failed to copy \"{}\" to \"{}\"",
            old_path.as_ref().as_os_str().to_str().unwrap(),
            new_path.as_ref().as_os_str().to_str().unwrap()
        );
    }
    Ok(())
}

pub async fn rename_file<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    old_path: &P,
    new_path: &Q,
    quiet: bool,
) -> SimpleResult<()> {
    if old_path.as_ref() != new_path.as_ref() {
        let new_directory = new_path.as_ref().parent().unwrap();
        if !new_directory.is_dir() {
            create_directory(progress_bar, &new_directory, quiet).await?;
        }
        if !quiet {
            progress_bar.println(format!(
                "Moving to \"{}\"",
                new_path.as_ref().as_os_str().to_str().unwrap()
            ));
        }
        let result = fs::rename(old_path, new_path).await;
        // rename doesn't work across filesystems, use copy/remove as fallback
        if result.is_err() {
            copy_file(progress_bar, old_path, new_path, quiet).await?;
            remove_file(progress_bar, old_path, quiet).await?;
        }
    }
    Ok(())
}

pub async fn remove_file<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
    quiet: bool,
) -> SimpleResult<()> {
    if !quiet {
        progress_bar.println(format!(
            "Deleting \"{}\"",
            path.as_ref().as_os_str().to_str().unwrap()
        ));
    }
    try_with!(
        fs::remove_file(path).await,
        "Failed to delete \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(())
}

pub async fn create_directory<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
    quiet: bool,
) -> SimpleResult<()> {
    if !quiet {
        progress_bar.println(format!(
            "Creating \"{}\"",
            path.as_ref().as_os_str().to_str().unwrap()
        ));
    }
    if !path.as_ref().is_dir() {
        try_with!(
            fs::create_dir_all(path).await,
            "Failed to create \"{}\"",
            path.as_ref().as_os_str().to_str().unwrap()
        );
    }
    Ok(())
}

pub async fn create_tmp_directory(connection: &mut SqliteConnection) -> SimpleResult<TempDir> {
    let tmp_directory = try_with!(
        TempDir::new_in(get_tmp_directory(connection).await),
        "Failed to create temp directory"
    );
    Ok(tmp_directory)
}

pub async fn remove_directory<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
    quiet: bool,
) -> SimpleResult<()> {
    if !quiet {
        progress_bar.println(format!(
            "Deleting \"{}\"",
            path.as_ref().as_os_str().to_str().unwrap()
        ));
    }
    try_with!(
        fs::remove_dir_all(path).await,
        "Failed to delete \"{}\"",
        path.as_ref().as_os_str().to_str().unwrap()
    );
    Ok(())
}

pub async fn get_system_directory_name(
    connection: &mut SqliteConnection,
    system: &System,
) -> SimpleResult<String> {
    let system_directory_name = match &system.custom_name {
        Some(custom_name) => custom_name.to_owned(),
        None => {
            if get_bool(connection, "GROUP_SUBSYSTEMS").await {
                SYSTEM_NAME_REGEX
                    .captures(&system.name)
                    .unwrap()
                    .get(2)
                    .unwrap()
                    .as_str()
                    .to_owned()
            } else {
                system.name.trim().to_owned()
            }
        }
    };
    Ok(system_directory_name)
}

pub async fn get_system_directory(
    connection: &mut SqliteConnection,
    system: &System,
) -> SimpleResult<PathBuf> {
    let system_directory_name = get_system_directory_name(connection, system).await?;
    let system_directory = get_rom_directory(connection)
        .await
        .join(system_directory_name);
    Ok(system_directory)
}

pub async fn get_one_region_directory(
    connection: &mut SqliteConnection,
    system: &System,
) -> SimpleResult<PathBuf> {
    let trash_directory = get_system_directory(connection, system).await?.join("1G1R");
    Ok(trash_directory)
}

pub async fn get_trash_directory(
    connection: &mut SqliteConnection,
    system: Option<&System>,
) -> SimpleResult<PathBuf> {
    let trash_directory = match system {
        Some(system) => get_system_directory(connection, system)
            .await?
            .join("Trash"),
        None => get_rom_directory(connection).await.join("Trash"),
    };
    Ok(trash_directory)
}

pub fn is_update(progress_bar: &ProgressBar, old_version: &str, new_version: &str) -> bool {
    match new_version.cmp(old_version) {
        Ordering::Less => {
            progress_bar.println(format!(
                "Version \"{}\" is older than \"{}\"",
                new_version, old_version
            ));
            false
        }
        Ordering::Equal => {
            progress_bar.println(format!("Already at version \"{}\"", new_version));
            false
        }
        Ordering::Greater => {
            progress_bar.println(format!(
                "Version \"{}\" is newer than \"{}\"",
                new_version, old_version
            ));
            true
        }
    }
}

pub async fn compute_system_completion(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.set_message("Computing system completion");
    update_games_by_system_id_mark_complete(connection, system.id).await;
    update_jbfolder_games_by_system_id_mark_complete(connection, system.id).await;
    update_system_mark_complete(connection, system.id).await;
    progress_bar.set_message("");
    progress_bar.disable_steady_tick();
}

pub async fn compute_system_incompletion(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.set_message("Computing system completion");
    update_games_by_system_id_mark_incomplete(connection, system.id).await;
    update_jbfolder_games_by_system_id_mark_incomplete(connection, system.id).await;
    update_system_mark_incomplete(connection, system.id).await;
    progress_bar.set_message("");
    progress_bar.disable_steady_tick();
}

pub async fn compute_arcade_system_completion(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.set_message("Computing system completion");
    let merging = Merging::from_i64(system.merging).unwrap();
    match merging {
        Merging::Split => {
            update_split_games_by_system_id_mark_complete(connection, system.id).await;
        }
        Merging::NonMerged | Merging::Merged => {
            update_non_merged_and_merged_games_by_system_id_mark_complete(connection, system.id)
                .await;
        }
        Merging::FullNonMerged | Merging::FullMerged => {
            update_games_by_system_id_mark_complete(connection, system.id).await;
        }
    }
    update_system_mark_complete(connection, system.id).await;
    progress_bar.set_message("");
    progress_bar.disable_steady_tick();
}

pub async fn compute_arcade_system_incompletion(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.set_message("Computing system completion");
    let merging = Merging::from_i64(system.merging).unwrap();
    match merging {
        Merging::Split => {
            update_split_games_by_system_id_mark_incomplete(connection, system.id).await;
        }
        Merging::NonMerged | Merging::Merged => {
            update_non_merged_and_merged_games_by_system_id_mark_incomplete(connection, system.id)
                .await;
        }
        Merging::FullNonMerged | Merging::FullMerged => {
            update_games_by_system_id_mark_incomplete(connection, system.id).await;
        }
    }
    update_system_mark_incomplete(connection, system.id).await;
    progress_bar.set_message("");
    progress_bar.disable_steady_tick();
}

pub async fn find_parent_chd_romfile_by_game(
    connection: &mut SqliteConnection,
    game: &Game,
) -> Option<Romfile> {
    let parent_game = match game.playlist_id {
        Some(playlist_id) => {
            let parent_game = find_first_game_by_playlist_id(connection, playlist_id).await;
            if parent_game.id != game.id {
                Some(parent_game)
            } else {
                None
            }
        }
        None => None,
    };
    match parent_game {
        Some(parent_game) => {
            let roms = find_roms_with_romfile_by_game_ids(connection, &[parent_game.id]).await;
            let mut romfile_ids = roms
                .into_par_iter()
                .map(|rom| rom.romfile_id.unwrap())
                .collect::<Vec<i64>>();
            romfile_ids.dedup();
            find_romfiles_by_ids(connection, &romfile_ids)
                .await
                .into_par_iter()
                .find_first(|romfile| romfile.path.ends_with(CHD_EXTENSION))
        }
        None => None,
    }
}

#[cfg(test)]
mod test_system_directory_no_group_subsystems;

#[cfg(test)]
mod test_system_directory_group_subsystems;

#[cfg(test)]
mod test_system_directory_custom_name;
#[cfg(test)]
mod test_system_directory_group_non_redump;
