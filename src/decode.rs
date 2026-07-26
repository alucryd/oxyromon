use super::chdman::{AsChd, ChdType};
use super::common::*;
use super::database::find_romfile_by_id;
use super::maxcso::AsXso;
use super::model::{Rom, Romfile};
use super::sevenzip::AsArchive;
use anyhow::Result;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use tempfile::TempDir;

/// A romfile decoded to ISO, along with the original file it came from.
pub struct DecodedIso {
    pub source: CommonRomfile,
    pub iso: IsoRomfile,
}

/// Extracts the single ISO contained in an archive into the tmp directory.
pub async fn archive_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<DecodedIso> {
    let archive_romfile = romfile
        .as_common(connection)
        .await?
        .as_archive(progress_bar, Some(rom))
        .await?
        .pop()
        .unwrap();
    let iso = archive_romfile
        .to_common(progress_bar, &tmp_directory.path())
        .await?
        .as_iso()?;
    Ok(DecodedIso {
        source: archive_romfile.romfile,
        iso,
    })
}

/// Views an ISO romfile as ISO, no decoding needed.
pub async fn iso_as_iso(
    connection: &mut SqliteConnection,
    romfile: &Romfile,
) -> Result<DecodedIso> {
    let common_romfile = romfile.as_common(connection).await?;
    Ok(DecodedIso {
        source: common_romfile.clone(),
        iso: common_romfile.as_iso()?,
    })
}

/// Decodes a DVD CHD to ISO in the tmp directory, resolving its parent if any.
/// Returns None when the CHD is not a DVD.
pub async fn chd_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Option<DecodedIso>> {
    let chd_romfile = match romfile.parent_id {
        Some(parent_id) => {
            let parent_chd_romfile = find_romfile_by_id(connection, parent_id)
                .await
                .as_common(connection)
                .await?
                .as_chd()
                .await?;
            romfile
                .as_common(connection)
                .await?
                .as_chd_with_parent(parent_chd_romfile)
                .await?
        }
        None => romfile.as_common(connection).await?.as_chd().await?,
    };
    if chd_romfile.chd_type != ChdType::Dvd {
        return Ok(None);
    }
    let iso = chd_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Some(DecodedIso {
        source: chd_romfile.romfile,
        iso,
    }))
}

/// Decodes a CSO/ZSO to ISO in the tmp directory.
pub async fn xso_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<DecodedIso> {
    let xso_romfile = romfile.as_common(connection).await?.as_xso().await?;
    let iso = xso_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(DecodedIso {
        source: xso_romfile.romfile,
        iso,
    })
}
