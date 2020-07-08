use super::SimpleResult;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn create_cso(iso_path: &PathBuf, directory: &Path) -> SimpleResult<PathBuf> {
    println!("Compressing {:?}", iso_path.file_name().unwrap());
    let mut cso_path = directory.join(iso_path.file_name().unwrap());
    cso_path.set_extension("cso");
    let output = try_with!(Command::new("maxcso")
        .arg(iso_path)
        .arg("-o")
        .arg(&cso_path)
        .output(), "Failed to create CSO");
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    Ok(cso_path)
}

pub fn extract_cso(cso_path: &PathBuf, directory: &Path) -> SimpleResult<PathBuf> {
    println!("Extracting {:?}", cso_path.file_name().unwrap());
    let mut iso_path = directory.join(cso_path.file_name().unwrap());
    iso_path.set_extension("iso");
    let output = try_with!(Command::new("maxcso")
        .arg("--decompress")
        .arg(cso_path)
        .arg("-o")
        .arg(&iso_path)
        .output(), "Failed to extract CSO");
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    Ok(iso_path)
}
