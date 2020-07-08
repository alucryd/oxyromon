use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn create_cso(iso_path: &PathBuf, directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    println!("Compressing {:?}", iso_path.file_name().unwrap());
    let mut cso_path = directory.join(iso_path.file_name().unwrap());
    cso_path.set_extension("cso");
    let output = Command::new("maxcso")
        .arg(iso_path)
        .arg("-o")
        .arg(&cso_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    Ok(cso_path)
}

pub fn extract_cso(cso_path: &PathBuf, directory: &Path) -> Result<PathBuf, Box<dyn Error>> {
    println!("Extracting {:?}", cso_path.file_name().unwrap());
    let mut iso_path = directory.join(cso_path.file_name().unwrap());
    iso_path.set_extension("iso");
    let output = Command::new("maxcso")
        .arg("--decompress")
        .arg(cso_path)
        .arg("-o")
        .arg(&iso_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    Ok(iso_path)
}
