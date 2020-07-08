use super::util::*;
use super::SimpleResult;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn create_chd(cue_path: &PathBuf) -> SimpleResult<PathBuf> {
    let mut chd_path = cue_path.clone();
    chd_path.set_extension("chd");

    println!("Compressing {:?}", cue_path);
    let output = try_with!(
        Command::new("chdman")
            .arg("createcd")
            .arg("-i")
            .arg(cue_path)
            .arg("-o")
            .arg(&chd_path)
            .output(),
        "Failed to create CHD"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    Ok(chd_path)
}

pub fn extract_chd(
    chd_path: &PathBuf,
    directory: &Path,
    tmp_directory: &Path,
    cue_name: &str,
    bin_names_sizes: &Vec<(&str, u64)>,
) -> SimpleResult<Vec<PathBuf>> {
    let mut bin_paths: Vec<PathBuf> = Vec::new();
    println!("Extracting {:?}", chd_path.file_name().unwrap());
    let cue_path = tmp_directory.join(cue_name);
    let mut tmp_bin_path = cue_path.clone();
    tmp_bin_path.set_extension("bin");
    let output = try_with!(
        Command::new("chdman")
            .arg("extractcd")
            .arg("-i")
            .arg(chd_path)
            .arg("-o")
            .arg(&cue_path)
            .arg("-ob")
            .arg(&tmp_bin_path)
            .output(),
        "Failed to extract CHD"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    remove_file(&cue_path)?;
    if bin_names_sizes.len() == 1 {
        let (bin_name, _) = bin_names_sizes.get(0).unwrap();
        let bin_path = directory.join(bin_name);
        if bin_path != tmp_bin_path {
            rename_file(&tmp_bin_path, &bin_path)?;
        }
        bin_paths.push(bin_path);
    } else {
        let bin_file = open_file(&tmp_bin_path)?;
        for (bin_name, size) in bin_names_sizes {
            let split_bin_path = directory.join(bin_name);
            let mut split_bin_file = create_file(&split_bin_path)?;
            let mut handle = (&bin_file).take(*size);
            try_with!(
                io::copy(&mut handle, &mut split_bin_file),
                "Failed to copy data"
            );
            bin_paths.push(split_bin_path);
        }
        remove_file(&tmp_bin_path)?;
    }
    Ok(bin_paths)
}
