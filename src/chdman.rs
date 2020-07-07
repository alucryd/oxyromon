use std::error::Error;
use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn create_chd(cue_path: &PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let mut chd_path = cue_path.clone();
    chd_path.set_extension("chd");

    println!("Compressing {:?}", cue_path);
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
    Ok(chd_path)
}

pub fn extract_chd(
    chd_path: &PathBuf,
    directory: &Path,
    tmp_directory: &Path,
    cue_name: &str,
    bin_names_sizes: &Vec<(&str, u64)>,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut bin_paths: Vec<PathBuf> = Vec::new();
    println!("Extracting {:?}", chd_path.file_name().unwrap());
    let cue_path = tmp_directory.join(cue_name);
    let mut tmp_bin_path = cue_path.clone();
    tmp_bin_path.set_extension("bin");
    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path)
        .arg("-o")
        .arg(&cue_path)
        .arg("-ob")
        .arg(&tmp_bin_path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        println!("{}", stderr);
        bail!(stderr)
    }
    fs::remove_file(cue_path)?;
    if bin_names_sizes.len() == 1 {
        let (bin_name, _) = bin_names_sizes.get(0).unwrap();
        let bin_path = directory.join(bin_name);
        if bin_path != tmp_bin_path {
            fs::rename(&tmp_bin_path, &bin_path)?;
        }
        bin_paths.push(bin_path);
    } else {
        let bin_file = fs::File::open(&tmp_bin_path)?;
        for (bin_name, size) in bin_names_sizes {
            let split_bin_path = directory.join(bin_name);
            let mut split_bin_file = fs::File::create(&split_bin_path)?;
            let mut handle = (&bin_file).take(*size);
            io::copy(&mut handle, &mut split_bin_file)?;
            bin_paths.push(split_bin_path);
        }
        fs::remove_file(&tmp_bin_path)?;
    }
    Ok(bin_paths)
}
