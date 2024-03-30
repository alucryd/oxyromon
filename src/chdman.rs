use std::path::{Path, PathBuf};
use std::time::Duration;

use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;

const CHDMAN: &str = "chdman";

pub const MIN_DREAMCAST_VERSION: &str = "0.262";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+").unwrap();
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Cd,
    Dvd,
}

pub struct ChdRomfile {
    pub path: PathBuf,
    pub cue_path: Option<PathBuf>,
}

impl AsCommon for ChdRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
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
        progress_bar.println(format!("Checking \"{}\"", self.as_common()?.to_string()));
        let tmp_directory = create_tmp_directory(connection).await?;
        if self.cue_path.is_some() {
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
                .as_common()?
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
        media_type: &MediaType,
    ) -> SimpleResult<ChdRomfile>;
}

impl ToChd for CueBinRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        media_type: &MediaType,
    ) -> SimpleResult<ChdRomfile> {
        let path = create_chd(
            progress_bar,
            &self.cue_romfile.path,
            destination_directory,
            media_type,
        )
        .await?;
        Ok(ChdRomfile {
            path,
            cue_path: Some(self.cue_romfile.path.clone()),
        })
    }
}

impl ToChd for IsoRomfile {
    async fn to_chd<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        media_type: &MediaType,
    ) -> SimpleResult<ChdRomfile> {
        let path = create_chd(progress_bar, &self.path, destination_directory, media_type).await?;
        Ok(ChdRomfile {
            path,
            cue_path: None,
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
        let path = extract_chd(
            progress_bar,
            &self.path,
            destination_directory,
            BIN_EXTENSION,
        )
        .await?;

        let mut cue_path: Option<PathBuf> = None;
        if destination_directory.as_ref() != self.cue_path.as_ref().unwrap().parent().unwrap() {
            let new_cue_path = destination_directory
                .as_ref()
                .join(&self.cue_path.as_ref().unwrap().file_name().unwrap());
            copy_file(
                progress_bar,
                &self.cue_path.as_ref().unwrap(),
                &new_cue_path,
                quiet,
            )
            .await?;
            cue_path = Some(new_cue_path);
        }

        if bin_roms.len() == 1 {
            let mut bin_romfile = CommonRomfile { path };
            bin_romfile = bin_romfile
                .rename(
                    progress_bar,
                    &destination_directory
                        .as_ref()
                        .join(&bin_roms.first().unwrap().name),
                    quiet,
                )
                .await?;
            return Ok(CueBinRomfile {
                cue_romfile: CommonRomfile {
                    path: cue_path.unwrap_or(self.cue_path.as_ref().unwrap().clone()),
                },
                bin_romfiles: vec![bin_romfile],
            });
        }

        let mut bin_paths: Vec<PathBuf> = Vec::new();
        let mut bin_file = open_file(&path).await?;

        for bin_rom in bin_roms {
            progress_bar.set_length(bin_rom.size as u64);

            let split_bin_path = destination_directory.as_ref().join(&bin_rom.name);
            let mut split_bin_file = create_file(progress_bar, &split_bin_path, quiet).await?;

            let mut handle = (&mut bin_file).take(bin_rom.size as u64);

            io::copy(&mut handle, &mut split_bin_file)
                .await
                .expect("Failed to copy data");

            bin_paths.push(split_bin_path);
        }

        remove_file(progress_bar, &path, quiet).await?;

        Ok(CueBinRomfile {
            cue_romfile: CommonRomfile {
                path: cue_path.unwrap_or(self.cue_path.as_ref().unwrap().clone()),
            },
            bin_romfiles: bin_paths
                .iter()
                .map(|bin_path| CommonRomfile {
                    path: bin_path.clone(),
                })
                .collect(),
        })
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
            &self.path,
            destination_directory,
            ISO_EXTENSION,
        )
        .await?;
        Ok(IsoRomfile { path })
    }
}

impl FromPath<ChdRomfile> for ChdRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<ChdRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != CHD_EXTENSION {
            bail!("Not a valid chd");
        }
        Ok(ChdRomfile {
            path,
            cue_path: None,
        })
    }
}

pub trait FromCuePath<T> {
    fn from_cue_path<P: AsRef<Path>, Q: AsRef<Path>>(path: &P, cue_path: &Q) -> SimpleResult<T>;
}

impl FromCuePath<ChdRomfile> for ChdRomfile {
    fn from_cue_path<P: AsRef<Path>, Q: AsRef<Path>>(
        path: &P,
        cue_path: &Q,
    ) -> SimpleResult<ChdRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != CHD_EXTENSION {
            bail!("Not a valid chd");
        }
        Ok(ChdRomfile {
            path,
            cue_path: Some(cue_path.as_ref().to_path_buf()),
        })
    }
}

pub trait AsChd {
    fn as_chd(&self) -> SimpleResult<ChdRomfile>;
    fn as_chd_with_cue<P: AsRef<Path>>(&self, cue_path: &P) -> SimpleResult<ChdRomfile>;
}

impl AsChd for Romfile {
    fn as_chd(&self) -> SimpleResult<ChdRomfile> {
        ChdRomfile::from_path(&self.path)
    }
    fn as_chd_with_cue<P: AsRef<Path>>(&self, cue_path: &P) -> SimpleResult<ChdRomfile> {
        ChdRomfile::from_cue_path(&self.path, cue_path)
    }
}

impl AsChd for CommonRomfile {
    fn as_chd(&self) -> SimpleResult<ChdRomfile> {
        ChdRomfile::from_path(&self.path)
    }
    fn as_chd_with_cue<P: AsRef<Path>>(&self, cue_path: &P) -> SimpleResult<ChdRomfile> {
        ChdRomfile::from_cue_path(&self.path, cue_path)
    }
}

async fn create_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    romfile_path: &P,
    destination_directory: &Q,
    media_type: &MediaType,
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

    let output = Command::new(CHDMAN)
        .arg(match media_type {
            MediaType::Cd => "createcd",
            MediaType::Dvd => "createdvd",
        })
        .arg("-i")
        .arg(romfile_path.as_ref())
        .arg("-o")
        .arg(&chd_path)
        .output()
        .await
        .expect("Failed to create chd");

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
    let iso_bin_path = destination_directory
        .as_ref()
        .join(path.as_ref().file_name().unwrap())
        .with_extension(extension);

    progress_bar.println(format!(
        "Extracting \"{}\"",
        path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut command = Command::new(CHDMAN);
    command
        .arg(match media_type {
            MediaType::Cd => "extractcd",
            MediaType::Dvd => "extractdvd",
        })
        .arg("-i")
        .arg(path.as_ref());
    match media_type {
        MediaType::Cd => command
            .arg("-o")
            .arg(&cue_path)
            .arg("-ob")
            .arg(&iso_bin_path),
        MediaType::Dvd => command.arg("-o").arg(&iso_bin_path),
    };
    let output = command.output().await.expect("Failed to extract chd");

    if media_type == MediaType::Cd {
        remove_file(progress_bar, &cue_path, true).await?;
    }

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(iso_bin_path)
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

    Ok(if metadata.contains("DVD") {
        MediaType::Dvd
    } else {
        MediaType::Cd
    })
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
