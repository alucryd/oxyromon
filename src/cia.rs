use super::progress::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

const CA_CERT_SIZE: usize = 0x400;
const CA_CERT_OFFSET: u64 = 0;
const TICKET_CERT_SIZE: usize = 0x300;
const TICKET_CERT_OFFSET: u64 = CA_CERT_OFFSET + CA_CERT_SIZE as u64;
const TMD_CERT_SIZE: usize = 0x300;
const TMD_CERT_OFFSET: u64 = TICKET_CERT_OFFSET + TICKET_CERT_SIZE as u64;

#[derive(Debug)]
pub struct ArchiveInfo {
    pub path: String,
    // parsed from the listing but not consumed yet
    #[allow(dead_code)]
    pub size: u64,
}

pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

pub async fn parse_cia<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    cia_path: &P,
) -> Result<Vec<ArchiveInfo>> {
    start_action(progress_bar, Some("Parsing cia"));

    let path = cia_path.as_ref().to_path_buf();
    let length = path.metadata()?.len();
    let cia = run_blocking(progress_bar, length, move |progress| {
        cia_rs::Cia::parse(&path, progress)
    })
    .await
    .with_context(|| format!("Failed to parse CIA: {}", cia_path.as_ref().display()))?;

    if cia.section(cia_rs::Section::TitleMetaData).is_none() {
        // a SimpleCIA, the homebrew container: one content with no ticket
        // and no title metadata, nothing a DAT could match
        bail!("Not a retail CIA: {}", cia_path.as_ref().display());
    }

    // contents first, in the order unpack will write them (packed in the
    // file by offset, whatever order the TMD's records were in), then the
    // TMD with its appended certificates — the order extract_files_from_cia
    // returns, so import_cia can zip them
    let mut contents = cia.contents().to_vec();
    contents.sort_by_key(|content| content.offset);
    let mut cia_infos: Vec<ArchiveInfo> = contents
        .iter()
        .map(|content| ArchiveInfo {
            path: format!("{:08x}", content.id),
            size: content.size,
        })
        .collect();
    cia_infos.push(ArchiveInfo {
        path: format!("tmd.{}", cia.title_version()),
        size: u64::from(cia.tmd_meta_size()) + TMD_CERT_SIZE as u64 + CA_CERT_SIZE as u64,
    });

    stop_action(progress_bar);

    Ok(cia_infos)
}

pub async fn extract_files_from_cia<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    directory: &Q,
) -> Result<Vec<PathBuf>> {
    start_action(progress_bar, Some("Extracting files"));

    let input = archive_path.as_ref().to_path_buf();
    let output = directory.as_ref().to_path_buf();
    // the TMD as the DAT records it: the metadata plus the CA and TMD
    // certificates, appended in that order
    let certs_path = output.join("certs");
    let tmd_path = output.join("tmd");
    let length = input.metadata()?.len();
    let written = run_blocking(progress_bar, length, move |progress| {
        cia_rs::unpack(
            &input,
            &output,
            &cia_rs::UnpackOptions {
                ticket: false,
                footer: false,
            },
            progress,
        )
    })
    .await
    .with_context(|| format!("Failed to extract CIA: {}", archive_path.as_ref().display()))?;

    let mut certs = File::open(&certs_path).await.with_context(|| {
        format!(
            "CIA has no certificate chain: {}",
            archive_path.as_ref().display()
        )
    })?;
    let certs_length = certs.metadata().await?.len();
    if certs_length < TMD_CERT_OFFSET + TMD_CERT_SIZE as u64 {
        bail!(
            "Certificate chain too small in CIA: {}",
            archive_path.as_ref().display()
        );
    }

    let mut ca_cert = [0; CA_CERT_SIZE];
    certs.read_exact(&mut ca_cert).await?;

    let mut tmd_cert = [0; TMD_CERT_SIZE];
    certs.seek(SeekFrom::Start(TMD_CERT_OFFSET)).await?;
    certs.read_exact(&mut tmd_cert).await?;

    let mut tmd = File::options().append(true).open(&tmd_path).await?;
    tmd.write_all(&tmd_cert).await?;
    tmd.write_all(&ca_cert).await?;

    let mut extracted_paths: Vec<PathBuf> = written
        .iter()
        .filter(|file| matches!(file.part, cia_rs::Part::Content(_)))
        .map(|file| file.path.clone())
        .collect();
    extracted_paths.push(tmd_path);

    stop_action(progress_bar);

    Ok(extracted_paths)
}
