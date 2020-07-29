use super::config::*;
use super::SimpleResult;
use diesel::SqliteConnection;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn get_canonicalized_path(path: &str) -> SimpleResult<PathBuf> {
    let canonicalized_path = try_with!(
        Path::new(path).canonicalize(),
        "Failed to get canonicalized path for {}",
        path
    );
    Ok(canonicalized_path)
}

pub fn open_file(path: &PathBuf) -> SimpleResult<fs::File> {
    let file = try_with!(fs::File::open(&path), "Failed to open {:?}", path);
    Ok(file)
}

pub fn create_file(path: &PathBuf) -> SimpleResult<fs::File> {
    let file = try_with!(fs::File::create(&path), "Failed to create {:?}", path);
    Ok(file)
}

pub fn rename_file(old_path: &PathBuf, new_path: &PathBuf) -> SimpleResult<()> {
    try_with!(
        fs::rename(&old_path, &new_path),
        "Failed to rename {:?} to {:?}"
    );
    Ok(())
}

pub fn remove_file(path: &PathBuf) -> SimpleResult<()> {
    try_with!(fs::remove_file(path), "Failed to delete {:?}", path);
    Ok(())
}

pub fn create_directory(path: &PathBuf) -> SimpleResult<()> {
    if !path.is_dir() {
        try_with!(
            fs::create_dir_all(path),
            format!("Failed to create {:?}", path)
        );
    }
    Ok(())
}

pub fn create_tmp_directory(connection: &SqliteConnection) -> SimpleResult<TempDir> {
    let tmp_directory = try_with!(
        TempDir::new_in(get_tmp_directory(connection)),
        "Failed to create temp directory"
    );
    Ok(tmp_directory)
}
