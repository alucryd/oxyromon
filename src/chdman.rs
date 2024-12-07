use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;
use strum::{Display, EnumString, VariantNames};
use tokio::io;
use tokio::io::AsyncReadExt;
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
pub enum MediaType {
    Cd,
    Dvd,
    Hd,
    Ld,
}

pub struct ChdRomfile {
    pub romfile: CommonRomfile,
    pub cue_romfile: Option<CommonRomfile>,
    pub parent_romfile: Option<CommonRomfile>,
    pub track_count: usize,
    pub media_type: MediaType,
}

impl Check for ChdRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        if self.cue_romfile.is_some() {
            let cue_bin_romfile = self
                .to_cue_bin(progress_bar, &tmp_directory.path(), roms, true)
                .await?;
            for (rom, bin_romfile) in roms.iter().zip(cue_bin_romfile.bin_romfiles) {
                bin_romfile
                    .check(connection, progress_bar, header, &[rom], hash_algorithm)
                    .await?;
            }
        } else {
            let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
            iso_romfile
                .romfile
                .check(connection, progress_bar, header, roms, hash_algorithm)
                .await?;
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
        let media_type = MediaType::Cd;
        let path = create_chd(
            progress_bar,
            &self.cue_romfile.path,
            destination_directory,
            &media_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            cue_romfile: Some(self.cue_romfile.clone()),
            parent_romfile,
            track_count: self.bin_romfiles.len(),
            media_type,
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
        let media_type = MediaType::Dvd;
        let path = create_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            &media_type,
            hunk_size,
            compression_algorithms,
            &parent_romfile,
        )
        .await?;
        Ok(ChdRomfile {
            romfile: CommonRomfile::from_path(&path)?,
            cue_romfile: None,
            parent_romfile,
            track_count: 1,
            media_type,
        })
    }
}

impl ToCueBin for ChdRomfile {
    async fn to_cue_bin<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        bin_roms: &[&Rom],
        quiet: bool,
    ) -> SimpleResult<CueBinRomfile> {
        let split = bin_roms.len() > 1
            && (get_version().await?.as_str().cmp(MIN_SPLITBIN_VERSION) == Ordering::Equal
                || get_version().await?.as_str().cmp(MIN_SPLITBIN_VERSION) == Ordering::Greater);
        let path = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            BIN_EXTENSION,
            &self.parent_romfile,
            split,
        )
        .await?;

        let cue_romfile = CommonRomfile::from_path(
            &destination_directory
                .as_ref()
                .join(self.cue_romfile.as_ref().unwrap().path.file_name().unwrap()),
        )?;
        if cue_romfile.path != self.cue_romfile.as_ref().unwrap().path {
            copy_file(
                progress_bar,
                &self.cue_romfile.as_ref().unwrap().path,
                &cue_romfile.path,
                quiet,
            )
            .await?;
        }

        if bin_roms.len() == 1 {
            let bin_romfile = CommonRomfile::from_path(&path)?
                .rename(
                    progress_bar,
                    &destination_directory
                        .as_ref()
                        .join(&bin_roms.first().unwrap().name),
                    quiet,
                )
                .await?;
            return cue_romfile.as_cue_bin(vec![bin_romfile]);
        }

        let mut bin_romfiles: Vec<CommonRomfile> = Vec::new();

        if split {
            for (i, bin_rom) in bin_roms.iter().enumerate() {
                let new_bin_path = destination_directory.as_ref().join(&bin_rom.name);
                let bin_romfile = CommonRomfile::from_path(
                    &destination_directory.as_ref().join(
                        path.file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_owned()
                            .replace("%t", &(i + 1).to_string()),
                    ),
                )?
                .rename(progress_bar, &new_bin_path, quiet)
                .await?;
                bin_romfiles.push(bin_romfile);
            }
        } else {
            let mut bin_file = open_file(&path).await?;

            for bin_rom in bin_roms {
                progress_bar.set_length(bin_rom.size as u64);

                let split_bin_path = destination_directory.as_ref().join(&bin_rom.name);
                let mut split_bin_file = create_file(progress_bar, &split_bin_path, quiet).await?;

                let mut handle = (&mut bin_file).take(bin_rom.size as u64);

                io::copy(&mut handle, &mut split_bin_file)
                    .await
                    .expect("Failed to copy data");

                bin_romfiles.push(CommonRomfile::from_path(&split_bin_path)?);
            }

            remove_file(progress_bar, &path, quiet).await?;
        }

        cue_romfile.as_cue_bin(bin_romfiles)
    }
}

impl ToIso for ChdRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<IsoRomfile> {
        let path = extract_chd(
            progress_bar,
            &self.romfile.path,
            destination_directory,
            ISO_EXTENSION,
            &self.parent_romfile,
            false,
        )
        .await?;
        CommonRomfile::from_path(&path)?.as_iso()
    }
}

pub trait AsChd {
    fn as_chd(self) -> SimpleResult<ChdRomfile>;
    fn as_chd_with_cue(self, cue_romfile: CommonRomfile) -> SimpleResult<ChdRomfile>;
    fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> SimpleResult<ChdRomfile>;
    fn as_chd_with_cue_and_parent(
        self,
        cue_romfile: CommonRomfile,
        parent_romfile: ChdRomfile,
    ) -> SimpleResult<ChdRomfile>;
}

impl AsChd for CommonRomfile {
    fn as_chd(self) -> SimpleResult<ChdRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CHD_EXTENSION
        {
            bail!("Not a valid chd");
        }
        Ok(ChdRomfile {
            romfile: self,
            cue_romfile: None,
            parent_romfile: None,
            //changeme
            track_count: 0,
            media_type: MediaType::Cd,
        })
    }
    fn as_chd_with_cue(self, cue_romfile: CommonRomfile) -> SimpleResult<ChdRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CHD_EXTENSION
        {
            bail!("Not a valid chd");
        }
        if cue_romfile
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CUE_EXTENSION
        {
            bail!("Not a valid cue");
        }
        Ok(ChdRomfile {
            romfile: self,
            cue_romfile: Some(cue_romfile),
            parent_romfile: None,
            //changeme
            track_count: 0,
            media_type: MediaType::Cd,
        })
    }
    fn as_chd_with_parent(self, parent_romfile: ChdRomfile) -> SimpleResult<ChdRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CHD_EXTENSION
        {
            bail!("Not a valid chd");
        }
        Ok(ChdRomfile {
            romfile: self,
            cue_romfile: None,
            parent_romfile: Some(parent_romfile.romfile),
            //changeme
            track_count: 0,
            media_type: MediaType::Cd,
        })
    }
    fn as_chd_with_cue_and_parent(
        self,
        cue_romfile: CommonRomfile,
        parent_romfile: ChdRomfile,
    ) -> SimpleResult<ChdRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CHD_EXTENSION
        {
            bail!("Not a valid chd");
        }
        if cue_romfile
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CUE_EXTENSION
        {
            bail!("Not a valid cue");
        }
        Ok(ChdRomfile {
            romfile: self,
            cue_romfile: Some(cue_romfile),
            parent_romfile: Some(parent_romfile.romfile),
            //changeme
            track_count: 0,
            media_type: MediaType::Cd,
        })
    }
}

async fn create_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    romfile_path: &P,
    destination_directory: &Q,
    media_type: &MediaType,
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
        .arg(match media_type {
            MediaType::Cd => "createcd",
            MediaType::Dvd => "createdvd",
            MediaType::Hd => "createhd",
            MediaType::Ld => "createld",
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
    parent_romfile: &Option<CommonRomfile>,
    split: bool,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting chd");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let media_type = parse(progress_bar, path).await?;

    let cue_path = destination_directory
        .as_ref()
        .join(format!(
            ".{}",
            path.as_ref().file_name().unwrap().to_str().unwrap()
        ))
        .with_extension(CUE_EXTENSION);
    let bin_path = destination_directory
        .as_ref()
        .join(path.as_ref().file_name().unwrap())
        .with_extension(if split {
            format!("%t.{}", extension)
        } else {
            extension.to_owned()
        });

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
        .arg(match media_type {
            MediaType::Cd => "extractcd",
            MediaType::Dvd => "extractdvd",
            MediaType::Hd => "extracthd",
            MediaType::Ld => "extractld",
        })
        .arg("-i")
        .arg(path.as_ref());
    match media_type {
        MediaType::Cd => command.arg("-o").arg(&cue_path).arg("-ob").arg(&bin_path),
        MediaType::Dvd | MediaType::Hd | MediaType::Ld => command.arg("-o").arg(&bin_path),
    };
    if let Some(parent_romfile) = parent_romfile {
        command.arg("-ip").arg(&parent_romfile.path);
    }
    if split {
        command.arg("-sb");
    }

    log::debug!("{:?}", command);

    let output = command.output().await.expect("Failed to extract chd");

    if media_type == MediaType::Cd {
        remove_file(progress_bar, &cue_path, true).await?;
    }

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(bin_path)
}

async fn parse<P: AsRef<Path>>(progress_bar: &ProgressBar, path: &P) -> SimpleResult<MediaType> {
    progress_bar.set_message("Parsing chd");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new(CHDMAN)
        .arg("info")
        .arg("-i")
        .arg(path.as_ref())
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

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    if metadata.contains("CHCD")
        || metadata.contains("CHGD")
        || metadata.contains("CHGT")
        || metadata.contains("CHT2")
        || metadata.contains("CHTR")
    {
        return Ok(MediaType::Cd);
    }
    if metadata.contains("DVD") {
        return Ok(MediaType::Dvd);
    }
    if metadata.contains("GDDD") || metadata.contains("GDDI") {
        return Ok(MediaType::Hd);
    }
    if metadata.contains("AVAV") || metadata.contains("AVLD") {
        return Ok(MediaType::Ld);
    }
    bail!("Unknown CHD type");
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
