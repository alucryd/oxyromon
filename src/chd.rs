use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use chd_rs::Chd;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};
use strum::{Display, EnumString, VariantNames};

pub const CHD_HUNK_SIZE_RANGE: [usize; 2] = [16, 1048576];

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
    ) -> Result<RiffRomfile>;
}

pub trait AsRiff {
    async fn as_riff(self) -> Result<RiffRomfile>;
}

impl AsRiff for CommonRomfile {
    async fn as_riff(self) -> Result<RiffRomfile> {
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
    ) -> Result<RdskRomfile>;
}

pub trait AsRdsk {
    async fn as_rdsk(self) -> Result<RdskRomfile>;
}

impl AsRdsk for CommonRomfile {
    async fn as_rdsk(self) -> Result<RdskRomfile> {
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

impl GetRomfile for ChdRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl Size for ChdRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> Result<u64> {
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
    ) -> Result<(String, u64)> {
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
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
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
    ) -> Result<ChdRomfile>;
}

impl ToChd for CueBinRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithms: &[String],
        hunk_size: &Option<usize>,
        parent_romfile: Option<CommonRomfile>,
    ) -> Result<ChdRomfile> {
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
    ) -> Result<ChdRomfile> {
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
    ) -> Result<ChdRomfile> {
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
    ) -> Result<ChdRomfile> {
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
    ) -> Result<CueBinRomfile> {
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
    ) -> Result<IsoRomfile> {
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
    ) -> Result<RiffRomfile> {
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
    ) -> Result<RdskRomfile> {
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
    async fn parse_chd(&self) -> Result<(ChdType, u64, String, String, Option<String>, usize)>;
    async fn as_chd(self) -> Result<ChdRomfile>;
    async fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> Result<ChdRomfile>;
}

impl AsChd for CommonRomfile {
    async fn parse_chd(&self) -> Result<(ChdType, u64, String, String, Option<String>, usize)> {
        let path = self.path.clone();
        let info = tokio::task::spawn_blocking(move || Chd::open(&path).map(|chd| chd.info()))
            .await
            .context("Failed to read CHD")?
            .with_context(|| format!("Failed to open \"{}\"", self.path.display()))?;
        let hex = |sha1: Option<[u8; 20]>| sha1.map(|sha1| chd_rs::sha1_hex(&sha1));
        let sha1 = hex(info.sha1).unwrap_or_default();
        let parent_sha1 = hex(info.parent_sha1);
        Ok(match info.chd_type {
            // a CD has no single size or hash, only its tracks do
            chd_rs::ChdType::Cd => (
                ChdType::Cd,
                0,
                String::new(),
                sha1,
                parent_sha1,
                info.track_count,
            ),
            chd_type => (
                match chd_type {
                    chd_rs::ChdType::Dvd => ChdType::Dvd,
                    chd_rs::ChdType::Ld => ChdType::Ld,
                    _ => ChdType::Hd,
                },
                info.logical_size,
                hex(info.data_sha1).unwrap_or_default(),
                sha1,
                parent_sha1,
                1,
            ),
        })
    }
    async fn as_chd(self) -> Result<ChdRomfile> {
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
                        // Skip directories
                        if entry_path.is_dir() {
                            continue;
                        }
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
                            system_id: None,
                        };
                        if let Ok((
                            _chd_type,
                            _size,
                            _sha1,
                            candidate_sha1,
                            _parent_sha1,
                            _track_count,
                        )) = candidate_romfile.parse_chd().await
                            && candidate_sha1 == *parent_sha1_value
                        {
                            parent_romfile = Some(candidate_romfile);
                            break;
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
    async fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> Result<ChdRomfile> {
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
) -> Result<PathBuf> {
    start_action(progress_bar, Some("Creating chd"));

    let chd_path = destination_directory
        .as_ref()
        .join(romfile_path.as_ref().file_name().unwrap())
        .with_extension(CHD_EXTENSION);

    print_action(
        progress_bar,
        &format!(
            "Creating \"{}\"",
            chd_path.file_name().unwrap().to_str().unwrap()
        ),
    );
    if let Some(parent_romfile) = parent_romfile {
        print_info(
            progress_bar,
            &format!(
                "Using parent \"{}\"",
                parent_romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );
    }

    {
        // chdman's defaults: its CD codecs, its LaserDisc one, or those it
        // uses for hard disks
        let names: Vec<&str> = if compression_algorithms.is_empty() {
            match chd_type {
                ChdType::Cd => vec!["cdlz", "cdzl", "cdfl"],
                ChdType::Ld => vec!["avhu"],
                _ => vec!["lzma", "zlib", "huff", "flac"],
            }
        } else {
            compression_algorithms.iter().map(String::as_str).collect()
        };
        let compression = codecs(&names)?;
        let input = romfile_path.as_ref().to_path_buf();
        let output = chd_path.clone();
        let parent_path = parent_romfile.as_ref().map(|parent| parent.path.clone());
        let chd_type = *chd_type;
        let hunk_size = hunk_size.map(|hunk_size| hunk_size as u32);
        let length = match chd_type {
            ChdType::Cd => chd_rs::cd_input_size(&input)?,
            _ => input.metadata()?.len(),
        };
        run_blocking(progress_bar, length, move |progress| {
            let mut parent = parent_path.map(Chd::open).transpose()?;
            if chd_type == ChdType::Ld {
                // one field a hunk unless told otherwise
                return chd_rs::create_ld(
                    &input,
                    &output,
                    hunk_size,
                    compression,
                    parent.as_mut(),
                    progress,
                );
            }
            // without a hunk size, chdman takes the parent's or its default
            let hunk_size = hunk_size
                .or(parent.as_ref().map(|parent| parent.info().hunk_size))
                .unwrap_or(match chd_type {
                    ChdType::Cd => 8 * chd_rs::CD_FRAME_SIZE,
                    _ => 4096,
                });
            match chd_type {
                ChdType::Cd => chd_rs::create_cd(
                    &input,
                    &output,
                    hunk_size,
                    compression,
                    parent.as_mut(),
                    progress,
                ),
                ChdType::Dvd => chd_rs::create_dvd(
                    &input,
                    &output,
                    hunk_size,
                    compression,
                    parent.as_mut(),
                    progress,
                ),
                _ => chd_rs::create_hd(
                    &input,
                    &output,
                    512,
                    hunk_size,
                    compression,
                    parent.as_mut(),
                    None,
                    progress,
                ),
            }
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", chd_path.display()))?;
    }

    stop_action(progress_bar);

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
) -> Result<(PathBuf, Option<PathBuf>)> {
    start_action(progress_bar, Some("Extracting chd"));

    let bin_path = destination_directory
        .as_ref()
        .join(path.as_ref().file_name().unwrap())
        .with_extension(if split {
            format!("%t.{}", extension)
        } else {
            extension.to_owned()
        });

    print_action(
        progress_bar,
        &format!(
            "Extracting \"{}\"",
            path.as_ref().file_name().unwrap().to_str().unwrap()
        ),
    );
    if let Some(parent_romfile) = parent_romfile {
        print_info(
            progress_bar,
            &format!(
                "Using parent \"{}\"",
                parent_romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );
    }

    // the CUE is hidden: a caller keeping the original sheet deletes it
    let cue_path = (*chd_type == ChdType::Cd).then(|| {
        destination_directory
            .as_ref()
            .join(format!(
                ".{}",
                path.as_ref().file_name().unwrap().to_str().unwrap()
            ))
            .with_extension(CUE_EXTENSION)
    });

    {
        let input = path.as_ref().to_path_buf();
        let parent_path = parent_romfile.as_ref().map(|parent| parent.path.clone());
        let chd = tokio::task::spawn_blocking(move || match parent_path {
            Some(parent) => Chd::open_with_parent(&input, &parent),
            None => Chd::open(&input),
        })
        .await
        .context("Failed to read CHD")?
        .with_context(|| format!("Failed to open \"{}\"", path.as_ref().display()))?;
        let length = chd.info().logical_size;
        let (bin, cue) = (bin_path.clone(), cue_path.clone());
        let chd_type = *chd_type;
        run_blocking(progress_bar, length, move |progress| {
            let mut chd = chd;
            match cue {
                Some(cue) => {
                    chd_rs::extract_cd(&mut chd, &cue, Some(&bin), split, progress).map(|_| ())
                }
                None if chd_type == ChdType::Ld => chd_rs::extract_ld(&mut chd, &bin, progress),
                None => chd.extract(&bin, progress),
            }
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", path.as_ref().display()))?;
    }

    stop_action(progress_bar);

    Ok((bin_path, cue_path))
}

/// The codec names, laid out in the four compression slots of a header:
/// `none` alone leaves them all empty, like chdman.
fn codecs(names: &[&str]) -> Result<[u32; 4]> {
    let mut slots = [0u32; 4];
    if names == ["none"] {
        return Ok(slots);
    }
    if names.len() > 4 {
        bail!("At most four CHD compression algorithms can be used");
    }
    for (slot, name) in slots.iter_mut().zip(names) {
        let tag: [u8; 4] = name
            .as_bytes()
            .try_into()
            .with_context(|| format!("Invalid CHD compression algorithm \"{name}\""))?;
        *slot = u32::from_be_bytes(tag);
    }
    Ok(slots)
}

pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

#[cfg(test)]
mod test_ld;
