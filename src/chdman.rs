use super::SimpleResult;
use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};
use std::time::Duration;
use strum::{Display, EnumString, VariantNames};
use tokio::process::Command;

const CHDMAN: &str = "chdman";

pub const CHD_HUNK_SIZE_RANGE: [usize; 2] = [16, 1048576];
pub const MIN_DREAMCAST_VERSION: &str = "0.264";
pub const MIN_SPLITBIN_VERSION: &str = "0.265";

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ChdCdCompressionAlgorithm {
    None,
    Cdfl,
    Cdlz,
    Cdzl,
    Cdzs,
}

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ChdDvdCompressionAlgorithm {
    None,
    Flac,
    Huff,
    Lzma,
    Zlib,
    Zstd,
}

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ChdHdCompressionAlgorithm {
    None,
    Flac,
    Huff,
    Lzma,
    Zlib,
    Zstd,
}

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ChdLdCompressionAlgorithm {
    None,
    Avhu,
}

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+").unwrap();
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChdType {
    Cd,
    Dvd,
    Hd,
    Ld,
}

pub struct RiffRomfile {
    pub romfile: CommonRomfile,
}

pub trait ToRiff {
    async fn to_riff<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<RiffRomfile>;
}

pub trait AsRiff {
    async fn as_riff(self) -> SimpleResult<RiffRomfile>;
}

impl AsRiff for CommonRomfile {
    async fn as_riff(self) -> SimpleResult<RiffRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() || mimetype.unwrap().extension() != RIFF_EXTENSION {
            bail!("Not a valid riff");
        }
        Ok(RiffRomfile { romfile: self })
    }
}

pub struct RdskRomfile {
    pub romfile: CommonRomfile,
}

pub trait ToRdsk {
    async fn to_rdsk<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<RdskRomfile>;
}

pub trait AsRdsk {
    async fn as_rdsk(self) -> SimpleResult<RdskRomfile>;
}

impl AsRdsk for CommonRomfile {
    async fn as_rdsk(self) -> SimpleResult<RdskRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() || mimetype.unwrap().extension() != RDSK_EXTENSION {
            bail!("Not a valid rdsk");
        }
        Ok(RdskRomfile { romfile: self })
    }
}

pub struct ChdRomfile {
    pub romfile: CommonRomfile,
    pub parent_romfile: Option<CommonRomfile>,
    pub chd_type: ChdType,
    pub size: u64,
    pub sha1: String,
    pub chd_sha1: String,
    pub track_count: usize,
}

impl Size for ChdRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> SimpleResult<u64> {
        if self.size > 0 {
            Ok(self.size)
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            match self.chd_type {
                ChdType::Cd => {
                    bail!("Not possible")
                }
                ChdType::Dvd => {
                    let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
                    Ok(iso_romfile
                        .romfile
                        .get_size(connection, progress_bar)
                        .await?)
                }
                ChdType::Hd => {
                    let rdsk_romfile = self.to_rdsk(progress_bar, &tmp_directory.path()).await?;
                    Ok(rdsk_romfile
                        .romfile
                        .get_size(connection, progress_bar)
                        .await?)
                }
                ChdType::Ld => {
                    let riff_romfile = self.to_riff(progress_bar, &tmp_directory.path()).await?;
                    Ok(riff_romfile
                        .romfile
                        .get_size(connection, progress_bar)
                        .await?)
                }
            }
        }
    }
}

impl HashAndSize for ChdRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)> {
        if hash_algorithm == &HashAlgorithm::Sha1 && !self.sha1.is_empty() && self.size > 0 {
            Ok((self.sha1.clone(), self.size))
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            match self.chd_type {
                ChdType::Cd => {
                    bail!("Not possible")
                }
                ChdType::Dvd => {
                    let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
                    Ok(iso_romfile
                        .romfile
                        .get_hash_and_size(
                            connection,
                            progress_bar,
                            position,
                            total,
                            hash_algorithm,
                        )
                        .await?)
                }
                ChdType::Hd => {
                    let rdsk_romfile = self.to_rdsk(progress_bar, &tmp_directory.path()).await?;
                    Ok(rdsk_romfile
                        .romfile
                        .get_hash_and_size(
                            connection,
                            progress_bar,
                            position,
                            total,
                            hash_algorithm,
                        )
                        .await?)
                }
                ChdType::Ld => {
                    let riff_romfile = self.to_riff(progress_bar, &tmp_directory.path()).await?;
                    Ok(riff_romfile
                        .romfile
                        .get_hash_and_size(
                            connection,
                            progress_bar,
                            position,
                            total,
                            hash_algorithm,
                        )
                        .await?)
                }
            }
        }
    }
}

impl Check for ChdRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        match self.chd_type {
            ChdType::Cd => {
                let cue_bin_romfile = self
                    .to_cue_bin(progress_bar, &tmp_directory.path(), None, roms, true)
                    .await?;
                for (rom, bin_romfile) in roms.iter().zip(cue_bin_romfile.bin_romfiles) {
                    bin_romfile
                        .check(connection, progress_bar, header, &[rom])
                        .await?;
                }
            }
            ChdType::Dvd => {
                let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
                iso_romfile
                    .romfile
                    .check(connection, progress_bar, header, roms)
                    .await?;
            }
            ChdType::Hd => {
                let rdsk_romfile = self.to_rdsk(progress_bar, &tmp_directory.path()).await?;
                rdsk_romfile
                    .romfile
                    .check(connection, progress_bar, header, roms)
                    .await?;
            }
            ChdType::Ld => {
                let riff_romfile = self.to_riff(progress_bar, &tmp_directory.path()).await?;
                riff_romfile
                    .romfile
                    .check(connection, progress_bar, header, roms)
                    .await?;
            }
        }
        Ok(())
    }
}

pub trait ToChd {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> SimpleResult<ChdRomfile>;
}

impl ToChd for CueBinRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> SimpleResult<ChdRomfile> {
        let chd_type = ChdType::Cd;
        let path = create_chd(
            progress_bar,
            &self.cue_romfile.path,
            destination_directory,
            &chd_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            parent_romfile,
            chd_type,
            size: 0,
            sha1: String::new(),
            chd_sha1: String::new(),
            track_count: self.bin_romfiles.len(),
        })
    }
}

impl ToChd for IsoRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> SimpleResult<ChdRomfile> {
        let chd_type = ChdType::Dvd;
        let path = create_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            &chd_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            parent_romfile,
            chd_type,
            size: 0,
            sha1: String::new(),
            chd_sha1: String::new(),
            track_count: 1,
        })
    }
}

impl ToChd for RiffRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> SimpleResult<ChdRomfile> {
        let chd_type = ChdType::Ld;
        let path = create_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            &chd_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            parent_romfile,
            chd_type,
            size: 0,
            sha1: String::new(),
            chd_sha1: String::new(),
            track_count: 1,
        })
    }
}

impl ToChd for RdskRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> SimpleResult<ChdRomfile> {
        let chd_type = ChdType::Hd;
        let path = create_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            &chd_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            parent_romfile,
            chd_type,
            size: 0,
            sha1: String::new(),
            chd_sha1: String::new(),
            track_count: 1,
        })
    }
}

impl ToCueBin for ChdRomfile {
    async fn to_cue_bin<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        cue_romfile: Option<CommonRomfile>,
        bin_roms: &[&Rom],
        quiet: bool,
    ) -> SimpleResult<CueBinRomfile> {
        let split = self.track_count > 1;
        let (bin_path, cue_path) = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            BIN_EXTENSION,
            &self.chd_type,
            &self.parent_romfile,
            split,
        )
        .await?;

        let mut bin_romfiles: Vec<CommonRomfile> = vec![];

        if split {
            for i in 0..self.track_count {
                let mut bin_romfile = CommonRomfile::from_path(
                    &destination_directory.as_ref().join(
                        bin_path
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned()
                            .replace("%t", &(i + 1).to_string()),
                    ),
                )?;
                if let Some(bin_rom) = bin_roms.get(i) {
                    bin_romfile = bin_romfile
                        .rename(
                            progress_bar,
                            &destination_directory.as_ref().join(&bin_rom.name),
                            quiet,
                        )
                        .await?;
                }
                bin_romfiles.push(bin_romfile);
            }
        } else {
            bin_romfiles.push(CommonRomfile::from_path(&bin_path)?);
        }

        match cue_romfile {
            Some(cue_romfile) => {
                CommonRomfile::from_path(&cue_path.unwrap())?
                    .delete(progress_bar, true)
                    .await?;
                cue_romfile.as_cue_bin(bin_romfiles)
            }
            None => CommonRomfile::from_path(&cue_path.unwrap())?.as_cue_bin(bin_romfiles),
        }
    }
}

impl ToIso for ChdRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<IsoRomfile> {
        let (path, _) = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            ISO_EXTENSION,
            &self.chd_type,
            &self.parent_romfile,
            false,
        )
        .await?;
        CommonRomfile::from_path(&path)?.as_iso()
    }
}

impl ToRiff for ChdRomfile {
    async fn to_riff<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<RiffRomfile> {
        let (path, _) = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            RIFF_EXTENSION,
            &self.chd_type,
            &self.parent_romfile,
            false,
        )
        .await?;
        CommonRomfile::from_path(&path)?.as_riff().await
    }
}

impl ToRdsk for ChdRomfile {
    async fn to_rdsk<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<RdskRomfile> {
        let (path, _) = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            RDSK_EXTENSION,
            &self.chd_type,
            &self.parent_romfile,
            false,
        )
        .await?;
        CommonRomfile::from_path(&path)?.as_rdsk().await
    }
}

pub trait AsChd {
    async fn parse_chd(
        &self,
    ) -> SimpleResult<(ChdType, u64, String, String, Option<String>, usize)>;
    async fn as_chd(self) -> SimpleResult<ChdRomfile>;
    async fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> SimpleResult<ChdRomfile>;
}

impl AsChd for CommonRomfile {
    async fn parse_chd(
        &self,
    ) -> SimpleResult<(ChdType, u64, String, String, Option<String>, usize)> {
        let output = Command::new(CHDMAN)
            .arg("info")
            .arg("-i")
            .arg(&self.path)
            .output()
            .await
            .expect("Failed to parse chd");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str());
        }

        let stdout = String::from_utf8(output.stdout).unwrap();

        let metadata: &str = stdout
            .lines()
            .find(|&line| line.starts_with("Metadata:"))
            .unwrap();

        let sha1 = stdout
            .lines()
            .find(|&line| line.starts_with("SHA1:"))
            .unwrap()
            .split(":")
            .last()
            .unwrap()
            .trim()
            .to_string();

        let parent_sha1 = stdout
            .lines()
            .find(|&line| line.starts_with("Parent SHA1:"))
            .map(|line| line.split(":").last().unwrap().trim().to_string());

        if metadata.contains("CHCD")
            || metadata.contains("CHGD")
            || metadata.contains("CHGT")
            || metadata.contains("CHT2")
            || metadata.contains("CHTR")
        {
            let track_count = stdout
                .lines()
                .filter(|&line| line.trim().starts_with("TRACK:"))
                .count();
            return Ok((
                ChdType::Cd,
                0,
                String::new(),
                sha1,
                parent_sha1,
                track_count,
            ));
        }

        let size: u64 = try_with!(
            stdout
                .lines()
                .find(|&line| line.starts_with("Logical size:"))
                .unwrap()
                .split(":")
                .last()
                .unwrap()
                .trim()
                .split(" ")
                .next()
                .unwrap()
                .replace(",", "")
                .parse(),
            "Failed to parse size"
        );
        let data_sha1 = stdout
            .lines()
            .find(|&line| line.starts_with("Data SHA1:"))
            .unwrap()
            .split(":")
            .last()
            .unwrap()
            .trim()
            .to_string();

        if metadata.contains("DVD") {
            return Ok((ChdType::Dvd, size, data_sha1, sha1, parent_sha1, 1));
        }
        if metadata.contains("GDDD") || metadata.contains("GDDI") {
            return Ok((ChdType::Hd, size, data_sha1, sha1, parent_sha1, 1));
        }
        if metadata.contains("AVAV") || metadata.contains("AVLD") {
            return Ok((ChdType::Ld, size, data_sha1, sha1, parent_sha1, 1));
        }
        bail!("Unknown CHD type");
    }
    async fn as_chd(self) -> SimpleResult<ChdRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() || mimetype.unwrap().extension() != CHD_EXTENSION {
            bail!("Not a valid chd");
        }
        let (chd_type, size, sha1, chd_sha1, parent_sha1, track_count) = self.parse_chd().await?;

        // Look for parent CHD if parent_sha1 is not null
        let parent_romfile = if let Some(ref parent_sha1_value) = parent_sha1 {
            if let Some(parent_dir) = self.path.parent() {
                if let Ok(entries) = std::fs::read_dir(parent_dir) {
                    let mut parent_romfile = None;
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        // Skip if it's the same file
                        if entry_path == self.path {
                            continue;
                        }
                        // Check if it's a CHD file
                        let mimetype = get_mimetype(&entry_path).await?;
                        if mimetype.is_none() || mimetype.unwrap().extension() != CHD_EXTENSION {
                            continue;
                        }
                        // Create a CommonRomfile and check its SHA1
                        let candidate_romfile = CommonRomfile {
                            path: entry_path.clone(),
                        };
                        if let Ok((
                            _chd_type,
                            _size,
                            _sha1,
                            candidate_sha1,
                            _parent_sha1,
                            _track_count,
                        )) = candidate_romfile.parse_chd().await
                        {
                            if candidate_sha1 == *parent_sha1_value {
                                parent_romfile = Some(candidate_romfile);
                                break;
                            }
                        }
                    }
                    parent_romfile
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(ChdRomfile {
            romfile: self,
            parent_romfile,
            chd_type,
            size,
            sha1,
            chd_sha1,
            track_count,
        })
    }
    async fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> SimpleResult<ChdRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() || mimetype.unwrap().extension() != CHD_EXTENSION {
            bail!("Not a valid chd");
        }
        let (chd_type, size, sha1, chd_sha1, parent_sha1, track_count) = self.parse_chd().await?;

        // Verify that the provided parent's SHA1 matches the expected parent_sha1
        if let Some(parent_sha1) = parent_sha1 {
            let (_chd_type, _size, _sha1, chd_sha1, _parent_sha1, _track_count) =
                parent_romfile.romfile.parse_chd().await?;
            if chd_sha1 != parent_sha1 {
                bail!(
                    "Parent CHD SHA1 mismatch: expected {}, got {}",
                    parent_sha1,
                    chd_sha1
                );
            }
        }

        Ok(ChdRomfile {
            romfile: self,
            parent_romfile: Some(parent_romfile.romfile),
            chd_type,
            size,
            sha1,
            chd_sha1,
            track_count,
        })
    }
}

async fn create_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    romfile_path: &P,
    destination_directory: &Q,
    chd_type: &ChdType,
    hunk_size: &Option<usize>,
    compression_algorithms: &[String],
    parent_romfile: &Option<CommonRomfile>,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating chd");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let chd_path = destination_directory
        .as_ref()
        .join(romfile_path.as_ref().file_name().unwrap())
        .with_extension(CHD_EXTENSION);

    progress_bar.println(format!(
        "Creating \"{}\"",
        chd_path.file_name().unwrap().to_str().unwrap()
    ));
    if let Some(parent_romfile) = parent_romfile {
        progress_bar.println(format!(
            "Using parent \"{}\"",
            parent_romfile.path.file_name().unwrap().to_str().unwrap()
        ));
    }

    let mut command = Command::new(CHDMAN);
    command
        .arg(match chd_type {
            ChdType::Cd => "createcd",
            ChdType::Dvd => "createdvd",
            ChdType::Hd => "createhd",
            ChdType::Ld => "createld",
        })
        .arg("-i")
        .arg(romfile_path.as_ref())
        .arg("-o")
        .arg(&chd_path);
    if let Some(hunk_size) = hunk_size {
        command.arg("--hunksize").arg(hunk_size.to_string());
    }
    if !compression_algorithms.is_empty() {
        command
            .arg("--compression")
            .arg(compression_algorithms.join(","));
    }
    if let Some(parent_romfile) = parent_romfile {
        command.arg("-op").arg(&parent_romfile.path);
    }

    log::debug!("{:?}", command);

    let output = command.output().await.expect("Failed to create chd");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(chd_path)
}

async fn extract_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
    destination_directory: &Q,
    extension: &str,
    chd_type: &ChdType,
    parent_romfile: &Option<CommonRomfile>,
    split: bool,
) -> SimpleResult<(PathBuf, Option<PathBuf>)> {
    progress_bar.set_message("Extracting chd");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let bin_path = destination_directory
        .as_ref()
        .join(path.as_ref().file_name().unwrap())
        .with_extension(if split {
            format!("%t.{}", extension)
        } else {
            extension.to_owned()
        });
    let cue_path: Option<PathBuf>;

    progress_bar.println(format!(
        "Extracting \"{}\"",
        path.as_ref().file_name().unwrap().to_str().unwrap()
    ));
    if let Some(parent_romfile) = parent_romfile {
        progress_bar.println(format!(
            "Using parent \"{}\"",
            parent_romfile.path.file_name().unwrap().to_str().unwrap()
        ));
    }

    let mut command = Command::new(CHDMAN);
    command
        .arg(match chd_type {
            ChdType::Cd => "extractcd",
            ChdType::Dvd => "extractdvd",
            ChdType::Hd => "extracthd",
            ChdType::Ld => "extractld",
        })
        .arg("-i")
        .arg(path.as_ref());
    match chd_type {
        ChdType::Cd => {
            cue_path = Some(
                destination_directory
                    .as_ref()
                    .join(format!(
                        ".{}",
                        path.as_ref().file_name().unwrap().to_str().unwrap()
                    ))
                    .with_extension(CUE_EXTENSION),
            );
            command
                .arg("-o")
                .arg(cue_path.as_ref().unwrap())
                .arg("-ob")
                .arg(&bin_path);
        }
        ChdType::Dvd | ChdType::Hd | ChdType::Ld => {
            cue_path = None;
            command.arg("-o").arg(&bin_path);
        }
    };
    if let Some(parent_romfile) = parent_romfile {
        command.arg("-ip").arg(&parent_romfile.path);
    }
    if split {
        command.arg("-sb");
    }

    log::debug!("{:?}", command);

    let output = command.output().await.expect("Failed to extract chd");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok((bin_path, cue_path))
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(CHDMAN).output().await,
        "Failed to spawn chdman"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = stdout
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}
