use super::config::HashAlgorithm;
use super::database::*;
use super::model::Header;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use crc32fast::Hasher;
use digest::generic_array::typenum::U4;
use digest::generic_array::GenericArray;
use digest::Digest;
use digest::OutputSizeUser;
use digest::{FixedOutput, HashMarker, Reset, Update};
use indicatif::ProgressBar;
use md5::Md5;
use sha1::Sha1;
use sqlx::sqlite::SqliteConnection;
use std::fs;
use std::io;
use std::io::prelude::*;

#[derive(Clone, Default)]
struct Crc32 {
    hasher: Hasher,
}

impl Crc32 {
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }
}

impl HashMarker for Crc32 {}

impl OutputSizeUser for Crc32 {
    type OutputSize = U4;
}

impl FixedOutput for Crc32 {
    fn finalize_into(self, out: &mut GenericArray<u8, U4>) {
        out.copy_from_slice(&self.hasher.finalize().to_be_bytes());
    }
}

impl Update for Crc32 {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }
}

impl Reset for Crc32 {
    fn reset(&mut self) {
        self.hasher.reset();
    }
}

impl io::Write for Crc32 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Update::update(self, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub async fn get_size_and_hash<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    file_path: &P,
    header: &Option<Header>,
    position: usize,
    total: usize,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<(u64, String)> {
    Ok(match hash_algorithm {
        HashAlgorithm::Crc => {
            get_size_and_crc(connection, progress_bar, file_path, header, position, total).await?
        }
        HashAlgorithm::Md5 => {
            get_size_and_md5(connection, progress_bar, file_path, header, position, total).await?
        }
        HashAlgorithm::Sha1 => {
            get_size_and_sha1(connection, progress_bar, file_path, header, position, total).await?
        }
    })
}

async fn get_size_and_crc<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    file_path: &P,
    header: &Option<Header>,
    position: usize,
    total: usize,
) -> SimpleResult<(u64, String)> {
    let (mut file, size) = get_file_and_size(connection, file_path, header).await?;

    progress_bar.reset();
    progress_bar.set_message(format!("Computing CRC ({}/{})", position, total));
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_length(size);

    // compute the checksum
    let mut digest = Crc32::new();
    try_with!(
        io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
        "Failed to copy data"
    );
    let crc = format!("{:08x}", digest.finalize()).to_lowercase();

    progress_bar.set_message("");
    progress_bar.set_style(get_none_progress_style());

    Ok((size, crc))
}

async fn get_size_and_md5<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    file_path: &P,
    header: &Option<Header>,
    position: usize,
    total: usize,
) -> SimpleResult<(u64, String)> {
    let (mut file, size) = get_file_and_size(connection, file_path, header).await?;

    progress_bar.reset();
    progress_bar.set_message(format!("Computing MD5 ({}/{})", position, total));
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_length(size);

    let mut digest = Md5::new();
    try_with!(
        io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
        "Failed to copy data"
    );
    let md5 = format!("{:032x}", digest.finalize()).to_lowercase();

    progress_bar.set_message("");
    progress_bar.set_style(get_none_progress_style());

    Ok((size, md5))
}

async fn get_size_and_sha1<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    file_path: &P,
    header: &Option<Header>,
    position: usize,
    total: usize,
) -> SimpleResult<(u64, String)> {
    let (mut file, size) = get_file_and_size(connection, file_path, header).await?;

    progress_bar.reset();
    progress_bar.set_message(format!("Computing SHA1 ({}/{})", position, total));
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_length(size);

    let mut digest = Sha1::new();
    try_with!(
        io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
        "Failed to copy data"
    );
    let md5 = format!("{:040x}", digest.finalize()).to_lowercase();

    progress_bar.set_message("");
    progress_bar.set_style(get_none_progress_style());

    Ok((size, md5))
}

async fn get_file_and_size<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    file_path: &P,
    header: &Option<Header>,
) -> SimpleResult<(fs::File, u64)> {
    let mut file = open_file_sync(file_path)?;
    let mut size = file.metadata().unwrap().len();

    // extract a potential header, revert if none is found
    if header.is_some() {
        let header = header.as_ref().unwrap();
        let rules = find_rules_by_header_id(connection, header.id).await;
        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        try_with!(
            (&mut file)
                .take(header.size as u64)
                .read_to_end(&mut buffer),
            "Failed to read into buffer"
        );

        let mut matches: Vec<bool> = Vec::new();
        for rule in rules {
            let start_byte = rule.start_byte as usize;
            let hex_values: Vec<String> = buffer[start_byte..]
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            let hex_value = hex_values.join("").to_lowercase();
            matches.push(hex_value.starts_with(&rule.hex_value.to_lowercase()));
        }

        if matches.iter().all(|&m| m) {
            size -= header.size as u64;
        } else {
            try_with!(file.rewind(), "Failed to rewind file");
        }
    }

    Ok((file, size))
}
