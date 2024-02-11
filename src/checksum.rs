use super::common::*;
use super::config::HashAlgorithm;
use super::model::Header;
use super::progress::*;
use super::SimpleResult;
use async_trait::async_trait;
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
use std::fs::File;
use std::io;

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

#[async_trait]
trait Checksum {
    async fn get_hash(
        self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<String>;

    async fn check(
        self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()>;
}

#[async_trait]
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

#[async_trait]
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

#[async_trait]
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

#[async_trait]
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

#[async_trait]
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

#[async_trait]
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

#[async_trait]
impl Checksum for OriginalRomfile {
    async fn get_hash(
        self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<String> {
        let (mut file, size) = self.get_file_and_header_size(connection, header).await?;
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
        self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        let size = self.get_size(connection, header).await?;
        if size != self.rom.size as u64 {
            bail!("Size mismatch");
        };

        let hash = self
            .get_hash(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if &hash != self.rom.crc.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Md5 => {
                if &hash != self.rom.md5.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Sha1 => {
                if &hash != self.rom.sha1.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
        }

        Ok(())
    }
}

impl CrcChecksum for ArchiveFile {}
