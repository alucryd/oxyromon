use super::config::*;
use super::crc32::*;
use super::database::*;
use super::model::Header;
use super::model::*;
use super::progress::*;
use super::util::*;
use digest::Digest;
use indicatif::ProgressBar;
use md5::Md5;
use sha1::Sha1;
use simple_error::SimpleResult;
use sqlx::SqliteConnection;
use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct CommonRomfile {
    pub path: PathBuf,
}

pub trait CommonFile {
    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<CommonRomfile>;
    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()>;
}

impl CommonFile for CommonRomfile {
    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<CommonRomfile> {
        if self.path != new_path.as_ref() {
            rename_file(progress_bar, &self.path, new_path, quiet).await?;
        }
        Ok(CommonRomfile {
            path: new_path.as_ref().to_path_buf(),
        })
    }

    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()> {
        remove_file(progress_bar, &self.path, quiet).await?;
        Ok(())
    }
}

impl ToString for CommonRomfile {
    fn to_string(&self) -> String {
        self.path.as_os_str().to_str().unwrap().to_string()
    }
}

pub trait FromPath<T> {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<T>;
}

impl FromPath<CommonRomfile> for CommonRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<CommonRomfile> {
        Ok(CommonRomfile {
            path: path.as_ref().to_path_buf(),
        })
    }
}

pub trait AsCommon {
    fn as_common(&self) -> SimpleResult<CommonRomfile>;
}

impl AsCommon for Romfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

pub trait ToCommon {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<CommonRomfile>;
}

pub trait HeaderedFile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<(File, u64)>;
}

impl HeaderedFile for CommonRomfile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<(File, u64)> {
        let mut file = open_file_sync(&self.path)?;
        let mut header_size: u64 = 0;

        // extract a potential header, revert if none is found
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

        Ok((file, header_size))
    }
}

pub trait Size {
    async fn get_size(&self) -> SimpleResult<u64>;

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<u64>;
}

impl Size for CommonRomfile {
    async fn get_size(&self) -> SimpleResult<u64> {
        Ok(self.path.metadata().unwrap().len())
    }

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<u64> {
        let (file, header_size) = self
            .get_file_and_header_size(connection, progress_bar, header)
            .await?;
        Ok(file.metadata().unwrap().len() - header_size)
    }
}

pub trait Hash {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)>;
}

impl Hash for CommonRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)> {
        let size = match header {
            Some(header) => {
                self.get_headered_size(connection, progress_bar, header)
                    .await?
            }
            None => self.get_size().await?,
        };

        progress_bar.reset();
        progress_bar.set_message(format!(
            "Computing {} ({}/{})",
            hash_algorithm, position, total
        ));
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(size);

        let mut file = match header {
            Some(header) => {
                self.get_file_and_header_size(connection, progress_bar, header)
                    .await?
                    .0
            }
            None => open_file_sync(&self.path)?,
        };
        let hash = match hash_algorithm {
            HashAlgorithm::Crc => {
                let mut digest = Crc32::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:08x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Md5 => {
                let mut digest = Md5::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:032x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Sha1 => {
                let mut digest = Sha1::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:040x}", digest.finalize()).to_lowercase()
            }
        };

        progress_bar.set_message("");
        progress_bar.set_style(get_none_progress_style());

        Ok((hash, size))
    }
}

pub trait Check {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()>;
}

impl Check for CommonRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.to_string()));
        let (hash, size) = self
            .get_hash_and_size(connection, progress_bar, header, 1, 1, hash_algorithm)
            .await?;
        if size != roms[0].size as u64 {
            bail!("Size mismatch");
        };
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if &hash != roms[0].crc.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Md5 => {
                if &hash != roms[0].md5.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Sha1 => {
                if &hash != roms[0].sha1.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
        }
        Ok(())
    }
}

pub struct IsoRomfile {
    pub path: PathBuf,
}

impl AsCommon for IsoRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

pub trait ToIso {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<IsoRomfile>;
}

impl FromPath<IsoRomfile> for IsoRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<IsoRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != ISO_EXTENSION {
            bail!("Not a valid iso");
        }
        Ok(IsoRomfile { path })
    }
}

pub trait AsIso {
    fn as_iso(&self) -> SimpleResult<IsoRomfile>;
}

impl AsIso for Romfile {
    fn as_iso(&self) -> SimpleResult<IsoRomfile> {
        IsoRomfile::from_path(&self.path)
    }
}

impl AsIso for CommonRomfile {
    fn as_iso(&self) -> SimpleResult<IsoRomfile> {
        IsoRomfile::from_path(&self.path)
    }
}

pub struct CueBinRomfile {
    pub cue_romfile: CommonRomfile,
    pub bin_romfiles: Vec<CommonRomfile>,
}

pub trait ToCueBin {
    async fn to_cue_bin<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        bin_roms: &[&Rom],
        quiet: bool,
    ) -> SimpleResult<CueBinRomfile>;
}

pub trait FromBinPaths<T> {
    fn from_bin_paths<P: AsRef<Path>, Q: AsRef<Path>>(path: &P, bin_paths: &[Q])
        -> SimpleResult<T>;
}

impl FromBinPaths<CueBinRomfile> for CueBinRomfile {
    fn from_bin_paths<P: AsRef<Path>, Q: AsRef<Path>>(
        path: &P,
        bin_paths: &[Q],
    ) -> SimpleResult<CueBinRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != CUE_EXTENSION {
            bail!("Not a valid cue");
        }
        for bin_path in bin_paths {
            let bin_path = bin_path.as_ref().to_path_buf();
            let extension = bin_path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase();
            if extension != BIN_EXTENSION {
                bail!("Not a valid bin");
            }
        }
        Ok(CueBinRomfile {
            cue_romfile: CommonRomfile { path },
            bin_romfiles: bin_paths
                .iter()
                .map(|bin_path| CommonRomfile {
                    path: bin_path.as_ref().to_path_buf(),
                })
                .collect(),
        })
    }
}

pub trait AsCueBin {
    fn as_cue_bin<P: AsRef<Path>>(&self, bin_paths: &[P]) -> SimpleResult<CueBinRomfile>;
}

impl AsCueBin for Romfile {
    fn as_cue_bin<P: AsRef<Path>>(&self, bin_paths: &[P]) -> SimpleResult<CueBinRomfile> {
        CueBinRomfile::from_bin_paths(&self.path, bin_paths)
    }
}

impl AsCueBin for CommonRomfile {
    fn as_cue_bin<P: AsRef<Path>>(&self, bin_paths: &[P]) -> SimpleResult<CueBinRomfile> {
        CueBinRomfile::from_bin_paths(&self.path, bin_paths)
    }
}
