use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn create_chd(cue_path: &PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let mut chd_path = cue_path.clone();
    chd_path.set_extension("chd");

    println!("Creating {:?} from {:?}", chd_path, cue_path);
    let output = Command::new("chdman")
        .arg("createcd")
        .arg("-i")
        .arg(cue_path)
        .arg("-o")
        .arg(&chd_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    fs::remove_file(cue_path)?;
    Ok(chd_path)
}

pub fn extract_chd(
    chd_path: &PathBuf,
    directory: &Path,
    cue_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    println!(
        "Extracting {:?} to {:?}",
        chd_path.file_name().unwrap(),
        directory
    );
    let cue_path = directory.join(cue_name);
    let mut bin_name = chd_path.file_stem().unwrap().to_os_string();
    bin_name.push(".bin");
    let bin_path = directory.join(bin_name);
    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path)
        .arg("-o")
        .arg(&cue_path)
        .arg("-ob")
        .arg(&bin_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    fs::remove_file(cue_path)?;
    Ok(bin_path)
}
