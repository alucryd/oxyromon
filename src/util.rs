use super::config::*;
use super::SimpleResult;
use async_std::fs;
use async_std::path::{Path, PathBuf};
use sqlx::SqliteConnection;
use tempfile::TempDir;

pub async fn get_canonicalized_path(path: &str) -> SimpleResult<PathBuf> {
    let canonicalized_path = try_with!(
        Path::new(path).canonicalize().await,
        "Failed to get canonicalized path for {}",
        path
    );
    Ok(canonicalized_path)
}

pub async fn open_file(path: &PathBuf) -> SimpleResult<fs::File> {
    let file = try_with!(fs::File::open(&path).await, "Failed to open {:?}", path);
    Ok(file)
}

pub fn open_file_sync(path: &Path) -> SimpleResult<std::fs::File> {
    let file = try_with!(std::fs::File::open(&path), "Failed to open {:?}", path);
    Ok(file)
}

pub fn get_reader_sync(path: &Path) -> SimpleResult<std::io::BufReader<std::fs::File>> {
    let f = open_file_sync(path)?;
    Ok(std::io::BufReader::new(f))
}

pub async fn create_file(path: &PathBuf) -> SimpleResult<fs::File> {
    let file = try_with!(fs::File::create(&path).await, "Failed to create {:?}", path);
    Ok(file)
}

pub async fn rename_file(old_path: &PathBuf, new_path: &PathBuf) -> SimpleResult<()> {
    try_with!(
        fs::rename(&old_path, &new_path).await,
        "Failed to rename {:?} to {:?}"
    );
    Ok(())
}

pub async fn remove_file(path: &PathBuf) -> SimpleResult<()> {
    try_with!(fs::remove_file(path).await, "Failed to delete {:?}", path);
    Ok(())
}

pub async fn create_directory(path: &PathBuf) -> SimpleResult<()> {
    if !path.is_dir().await {
        try_with!(
            fs::create_dir_all(path).await,
            format!("Failed to create {:?}", path)
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
