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

pub async fn open_file<P: AsRef<Path>>(path: &P) -> SimpleResult<fs::File> {
    let file = try_with!(
        fs::File::open(path.as_ref()).await,
        "Failed to open {:?}",
        path.as_ref()
    );
    Ok(file)
}

pub fn open_file_sync<P: AsRef<Path>>(path: &P) -> SimpleResult<std::fs::File> {
    let file = try_with!(
        std::fs::File::open(path.as_ref()),
        "Failed to open {:?}",
        path.as_ref()
    );
    Ok(file)
}

pub fn get_reader_sync<P: AsRef<Path>>(
    path: &P,
) -> SimpleResult<std::io::BufReader<std::fs::File>> {
    let f = open_file_sync(path)?;
    Ok(std::io::BufReader::new(f))
}

pub async fn create_file<P: AsRef<Path>>(path: &P) -> SimpleResult<fs::File> {
    let file = try_with!(
        fs::File::create(path).await,
        "Failed to create {:?}",
        path.as_ref()
    );
    Ok(file)
}

pub async fn rename_file<P: AsRef<Path>, Q: AsRef<Path>>(old_path: &P, new_path: &Q) -> SimpleResult<()> {
    try_with!(
        fs::rename(old_path, new_path).await,
        "Failed to rename {:?} to {:?}",
        old_path.as_ref(),
        new_path.as_ref()
    );
    Ok(())
}

pub async fn remove_file<P: AsRef<Path>>(path: &P) -> SimpleResult<()> {
    try_with!(
        fs::remove_file(path).await,
        "Failed to delete {:?}",
        path.as_ref()
    );
    Ok(())
}

pub async fn create_directory<P: AsRef<Path>>(path: &P) -> SimpleResult<()> {
    if !path.as_ref().is_dir().await {
        try_with!(
            fs::create_dir_all(path).await,
            format!("Failed to create {:?}", path.as_ref())
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
