use super::chdman::{AsChd, ChdType};
use super::common::*;
use super::database::find_romfile_by_id;
use super::dolphin::AsRvz;
use super::maxcso::AsXso;
use super::model::{Rom, Romfile};
use super::nsz::{AsNsz, NspRomfile, ToNsp};
use super::sevenzip::AsArchive;
use anyhow::Result;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use tempfile::TempDir;

/// A romfile decoded to a neutral format, along with the original file it came from.
pub struct Decoded<T> {
    pub source: CommonRomfile,
    pub inner: T,
}

/// Extracts the single file contained in an archive into the tmp directory.
pub async fn archive_to_common(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    rom: &Rom,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<CommonRomfile>> {
    let archive_romfile = romfile
        .as_common(connection)
        .await?
        .as_archive(progress_bar, Some(rom))
        .await?
        .pop()
        .unwrap();
    let inner = archive_romfile
        .to_common(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: archive_romfile.romfile,
        inner,
    })
}

/// Views a plain romfile as its own source, no decoding needed.
pub async fn common_as_source(
    connection: &mut SqliteConnection,
    romfile: &Romfile,
) -> Result<Decoded<CommonRomfile>> {
    let common_romfile = romfile.as_common(connection).await?;
    Ok(Decoded {
        source: common_romfile.clone(),
        inner: common_romfile,
    })
}

/// Decodes a DVD CHD to ISO in the tmp directory, resolving its parent if any.
/// Returns None when the CHD is not a DVD.
pub async fn chd_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Option<Decoded<IsoRomfile>>> {
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
    let inner = chd_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Some(Decoded {
        source: chd_romfile.romfile,
        inner,
    }))
}

/// Decodes a CSO/ZSO to ISO in the tmp directory.
pub async fn xso_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<IsoRomfile>> {
    let xso_romfile = romfile.as_common(connection).await?.as_xso().await?;
    let inner = xso_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: xso_romfile.romfile,
        inner,
    })
}

/// Decodes an NSZ to NSP in the tmp directory.
pub async fn nsz_to_nsp(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<NspRomfile>> {
    let nsz_romfile = romfile.as_common(connection).await?.as_nsz()?;
    let inner = nsz_romfile
        .to_nsp(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: nsz_romfile.romfile,
        inner,
    })
}

/// Decodes an RVZ to ISO in the tmp directory.
pub async fn rvz_to_iso(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    tmp_directory: &TempDir,
) -> Result<Decoded<IsoRomfile>> {
    let rvz_romfile = romfile.as_common(connection).await?.as_rvz()?;
    let inner = rvz_romfile
        .to_iso(progress_bar, &tmp_directory.path())
        .await?;
    Ok(Decoded {
        source: rvz_romfile.romfile,
        inner,
    })
}
