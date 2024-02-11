use super::database::*;
use super::model::Header;
use super::model::*;
use super::util::*;
use async_trait::async_trait;
use simple_error::SimpleResult;
use sqlx::SqliteConnection;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

pub struct OriginalRomfile {
    pub path: PathBuf,
    pub rom: Rom,
}

#[async_trait]
pub trait HeaderedFile {
    async fn get_file_and_header_size(
        self,
        connection: &mut SqliteConnection,
        header: &Option<Header>,
    ) -> SimpleResult<(File, u64)>;
}

#[async_trait]
pub trait Size {
    async fn get_size(
        self,
        connection: &mut SqliteConnection,
        header: &Option<Header>,
    ) -> SimpleResult<u64>;
}

#[async_trait]
pub trait ToOriginal {
    async fn to_original<P: AsRef<Path>>() -> OriginalRomfile;
}

#[async_trait]
impl HeaderedFile for OriginalRomfile {
    async fn get_file_and_header_size(
        self,
        connection: &mut SqliteConnection,
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

#[async_trait]
impl Size for OriginalRomfile {
    async fn get_size(
        self,
        connection: &mut SqliteConnection,
        header: &Option<Header>,
    ) -> SimpleResult<u64> {
        let (mut file, header_size) = self.get_file_and_header_size(connection, header).await?;
        Ok(file.metadata().unwrap().len() - header_size)
    }
}
