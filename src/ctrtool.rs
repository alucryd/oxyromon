use super::progress::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use indicatif::ProgressBar;
use std::fs::File;
use std::io::{prelude::*, SeekFrom};
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;

const CA_CERT_SIZE: usize = 0x400;
const CA_CERT_OFFSET: u64 = 0;
const TICKET_CERT_SIZE: usize = 0x300;
const TICKET_CERT_OFFSET: u64 = CA_CERT_OFFSET + CA_CERT_SIZE as u64;
const TMD_CERT_SIZE: usize = 0x300;
const TMD_CERT_OFFSET: u64 = TICKET_CERT_OFFSET + TICKET_CERT_SIZE as u64;

#[derive(Debug)]
pub struct ArchiveInfo {
    pub path: String,
    pub size: u64,
}

pub fn parse_cia<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    cia_path: &P,
) -> SimpleResult<Vec<ArchiveInfo>> {
    progress_bar.set_message("Parsing ctrtool");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new("ctrtool")
        .arg("-p")
        .arg("-v")
        .arg(cia_path.as_ref())
        .output()
        .expect("Failed to parse CIA");

    if !output.status.success()
        // error expected when using -p with non-homebrew titles
        && !output
            .stdout
            .ends_with(b"[ctrtool::NcchProcess ERROR] NcchHeader is corrupted (Bad struct magic).\n")
    {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut cia_infos = Vec::new();

    let mut version = None;
    let mut tmd_size = 0;
    let mut content_id = None;

    for line in stdout.lines() {
        if let Some(version_str) = line.strip_prefix("|- TitleVersion:  ") {
            let version_number_str = version_str.split_once('(').unwrap().1.trim_end_matches(')');
            version = Some(u16::from_str(version_number_str).unwrap());
        } else if let Some(tmd_size_str) = line.strip_prefix("|- TitleMetaSize:") {
            tmd_size =
                u64::from_str_radix(tmd_size_str.trim().trim_start_matches("0x"), 16).unwrap();
        } else if let Some(content_id_str) = line
            .trim_start_matches(&[' ', '|', '\\', '-'])
            .strip_prefix("ContentId:   0x")
        {
            content_id = Some(content_id_str.trim().to_string());
        } else if let Some(content_size_str) = line
            .trim_start_matches(&[' ', '|', '\\', '-'])
            .strip_prefix("Size:")
        {
            if let Some(content_id) = content_id.take() {
                cia_infos.push(ArchiveInfo {
                    path: content_id,
                    size: u64::from_str_radix(content_size_str.trim().trim_start_matches("0x"), 16)
                        .unwrap(),
                });
            }
        }
    }

    cia_infos.push(ArchiveInfo {
        path: format!("tmd.{}", version.unwrap()),
        size: tmd_size + TMD_CERT_SIZE as u64 + CA_CERT_SIZE as u64,
    });

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(cia_infos)
}

pub fn extract_files_from_cia<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    directory: &Q,
) -> SimpleResult<Vec<PathBuf>> {
    progress_bar.set_message("Extracting files");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let directory = directory.as_ref();
    let mut extracted_paths = Vec::new();

    let output = Command::new("ctrtool")
        .arg("-p")
        .arg("-v")
        .arg("--certs=certs")
        .arg("--tmd=tmd")
        .arg("--contents=content")
        .arg(archive_path.as_ref())
        .current_dir(directory)
        .output()
        .expect("Failed to extract archive");

    let stderr = String::from_utf8(output.stderr).unwrap();

    if !output.status.success()
        // error expected when using -p with non-homebrew titles
        && !output
            .stdout
            .ends_with(b"[ctrtool::NcchProcess ERROR] NcchHeader is corrupted (Bad struct magic).\n")
    {
        bail!(stderr.as_str())
    }

    for line in stderr.lines() {
        if let Some(content_str) = line.strip_prefix("[ctrtool::CiaProcess LOG] Saving content ") {
            let filename = &content_str[8..content_str.len() - 3];
            extracted_paths.push(directory.join(filename));
        }
    }

    let tmd_path = directory.join("tmd");

    let mut certs = File::open(directory.join("certs")).unwrap();

    let mut ca_cert = [0; CA_CERT_SIZE];
    certs.read_exact(&mut ca_cert).unwrap();

    let mut tmd_cert = [0; TMD_CERT_SIZE];
    certs.seek(SeekFrom::Start(TMD_CERT_OFFSET)).unwrap();
    certs.read_exact(&mut tmd_cert).unwrap();

    let mut tmd = File::options().append(true).open(&tmd_path).unwrap();
    tmd.write_all(&tmd_cert).unwrap();
    tmd.write_all(&ca_cert).unwrap();

    extracted_paths.push(tmd_path);

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(extracted_paths)
}
