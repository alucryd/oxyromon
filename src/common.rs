use super::config::*;
use super::database::*;
use super::model::Header;
use super::model::*;
use super::progress::*;
use super::util::*;
use crc32fast::Hasher;
use digest::generic_array::typenum::U4;
use digest::generic_array::GenericArray;
use digest::Digest;
use digest::OutputSizeUser;
use digest::{FixedOutput, HashMarker, Reset, Update};
use indicatif::ProgressBar;
use md5::Md5;
use sha1::Sha1;
use simple_error::SimpleResult;
use sqlx::SqliteConnection;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

pub struct OriginalRomfile {
    pub path: PathBuf,
}

pub trait CommonFile {
    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<OriginalRomfile>;
    async fn delete(self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()>;
}

pub trait OriginalFile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
    ) -> SimpleResult<(File, u64)>;
}

pub trait Size {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
    ) -> SimpleResult<u64>;
}

pub trait AsOriginal {
    fn as_original(&self) -> OriginalRomfile;
}

impl AsOriginal for Romfile {
    fn as_original(&self) -> OriginalRomfile {
        OriginalRomfile {
            path: PathBuf::from(&self.path),
        }
    }
}

pub trait ToOriginal {
    async fn to_original<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        directory: &P,
    ) -> SimpleResult<OriginalRomfile>;
}

impl CommonFile for OriginalRomfile {
    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<OriginalRomfile> {
        rename_file(progress_bar, &self.path, new_path, quiet).await?;
        Ok(OriginalRomfile {
            path: new_path.as_ref().to_path_buf(),
        })
    }

    async fn delete(self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()> {
        remove_file(progress_bar, &self.path, quiet).await?;
        Ok(())
    }
}

impl OriginalFile for OriginalRomfile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
        header: &Option<Header>,
    ) -> SimpleResult<(File, u64)> {
        let mut file = open_file_sync(&self.path)?;
        let mut header_size: u64 = 0;

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
                header_size = header.size as u64;
            } else {
                try_with!(file.rewind(), "Failed to rewind file");
            }
        }

        Ok((file, header_size))
    }
}

impl Size for OriginalRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
    ) -> SimpleResult<u64> {
        let (file, header_size) = self
            .get_file_and_header_size(connection, progress_bar, header)
            .await?;
        Ok(file.metadata().unwrap().len() - header_size)
    }
}

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

pub trait Checksum {
    async fn get_hash(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<String>;

    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        rom: &Rom,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()>;
}

trait CrcChecksum {
    async fn get_crc(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String>;
}

trait Md5Checksum {
    async fn get_md5(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String>;
}

trait Sha1Checksum {
    async fn get_sha1(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String>;
}

impl CrcChecksum for OriginalRomfile {
    async fn get_crc(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String> {
        progress_bar.reset();
        progress_bar.set_message(format!("Computing CRC ({}/{})", position, total));
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(size);

        // compute the checksum
        let mut digest = Crc32::new();
        try_with!(
            io::copy(file, &mut progress_bar.wrap_write(&mut digest)),
            "Failed to copy data"
        );
        let crc = format!("{:08x}", digest.finalize()).to_lowercase();

        progress_bar.set_message("");
        progress_bar.set_style(get_none_progress_style());

        Ok(crc)
    }
}

impl Md5Checksum for OriginalRomfile {
    async fn get_md5(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String> {
        progress_bar.reset();
        progress_bar.set_message(format!("Computing MD5 ({}/{})", position, total));
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(size);

        let mut digest = Md5::new();
        try_with!(
            io::copy(file, &mut progress_bar.wrap_write(&mut digest)),
            "Failed to copy data"
        );
        let md5 = format!("{:032x}", digest.finalize()).to_lowercase();

        progress_bar.set_message("");
        progress_bar.set_style(get_none_progress_style());

        Ok(md5)
    }
}

impl Sha1Checksum for OriginalRomfile {
    async fn get_sha1(
        &self,
        progress_bar: &ProgressBar,
        file: &mut File,
        size: u64,
        position: usize,
        total: usize,
    ) -> SimpleResult<String> {
        progress_bar.reset();
        progress_bar.set_message(format!("Computing SHA1 ({}/{})", position, total));
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(size);

        let mut digest = Sha1::new();
        try_with!(
            io::copy(file, &mut progress_bar.wrap_write(&mut digest)),
            "Failed to copy data"
        );
        let sha1 = format!("{:040x}", digest.finalize()).to_lowercase();

        progress_bar.set_message("");
        progress_bar.set_style(get_none_progress_style());

        Ok(sha1)
    }
}

impl Checksum for OriginalRomfile {
    async fn get_hash(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<String> {
        let (mut file, size) = self
            .get_file_and_header_size(connection, progress_bar, header)
            .await?;
        Ok(match hash_algorithm {
            HashAlgorithm::Crc => {
                self.get_crc(progress_bar, &mut file, size, position, total)
                    .await?
            }
            HashAlgorithm::Md5 => {
                self.get_md5(progress_bar, &mut file, size, position, total)
                    .await?
            }
            HashAlgorithm::Sha1 => {
                self.get_sha1(progress_bar, &mut file, size, position, total)
                    .await?
            }
        })
    }

    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        rom: &Rom,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        let size = self.get_size(connection, progress_bar, header).await?;
        if size != rom.size as u64 {
            bail!("Size mismatch");
        };

        let hash = self
            .get_hash(
                connection,
                progress_bar,
                header,
                position,
                total,
                hash_algorithm,
            )
            .await?;
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if &hash != rom.crc.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Md5 => {
                if &hash != rom.md5.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Sha1 => {
                if &hash != rom.sha1.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
        }

        Ok(())
    }
}
