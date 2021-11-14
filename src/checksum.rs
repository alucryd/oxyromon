use super::database::*;
use super::model::Header;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use crc32fast::Hasher;
use digest::generic_array::typenum::{U4, U64};
use digest::generic_array::GenericArray;
use digest::Digest;
use digest::{BlockInput, FixedOutputDirty, Reset, Update};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
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

impl FixedOutputDirty for Crc32 {
    type OutputSize = U4;

    fn finalize_into_dirty(&mut self, out: &mut GenericArray<u8, U4>) {
        out.copy_from_slice(&self.hasher.to_owned().finalize().to_be_bytes());
    }
}

impl BlockInput for Crc32 {
    type BlockSize = U64;
}

impl Update for Crc32 {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        self.hasher.update(input.as_ref());
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

pub async fn get_file_size_and_crc<P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    file_path: &P,
    header: &Option<Header>,
    position: usize,
    total: usize,
) -> SimpleResult<(u64, String)> {
    let mut f = open_file_sync(file_path)?;
    let mut size = f.metadata().unwrap().len();

    // extract a potential header, revert if none is found
    if header.is_some() {
        let header = header.as_ref().unwrap();
        let operation = header.operation.as_ref();
        let rules = find_rules_by_header_id(connection, header.id).await;

        for rule in rules {
            let mut buffer: Vec<u8> = Vec::with_capacity(rule.size as usize);
            try_with!(
                (&mut f).take(rule.size as u64).read_to_end(&mut buffer),
                "Failed to read into buffer"
            );
            let start_byte = rule.start_byte as usize;
            let hex_values: Vec<String> = buffer[start_byte..]
                .iter()
                .map(|b| format!("{:x}", b))
                .collect();
            let hex_value = hex_values.join("").to_lowercase();
            if hex_value.starts_with(&rule.hex_value.to_lowercase())
                && (operation.is_none() || operation.unwrap() != "none")
            {
                size -= rule.size as u64;
            } else {
                try_with!(f.seek(std::io::SeekFrom::Start(0)), "Failed to seek file");
            }
        }
    }

    progress_bar.reset();
    progress_bar.set_message(format!("Computing CRC ({}/{})", position, total));
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_length(size);

    // compute the checksum
    let mut digest = Crc32::new();
    try_with!(
        io::copy(&mut progress_bar.wrap_read(f), &mut digest),
        "Failed to copy data"
    );
    let crc = format!("{:08x}", digest.finalize()).to_lowercase();

    progress_bar.set_message("");
    progress_bar.set_style(get_none_progress_style());

    Ok((size, crc))
}
