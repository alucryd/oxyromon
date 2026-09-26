//! CUE/BIN to ISO: a port of the part of [bchunk] oxyromon used, which cuts the
//! first track of a CUE/BIN down to the 2048-byte user data of each sector.
//! That is the ISO Open PS2 Loader reads for PlayStation 2 CD games.
//!
//! bchunk writes every track; this writes only the first, which is all the ISO
//! ever held, and says why when that track is not data it can convert. A CUE
//! with a BIN per track works too: the first track is in the BIN the CUE names
//! first, and ends with it.
//!
//! [bchunk]: https://github.com/extramaster/bchunk
//!
//! The ISO 9660 reader `import-irds` walks is vendored in [`iso9660`].

pub mod iso9660;

use super::common::*;
use super::mimetype::*;
use super::progress::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// A raw CD sector, as a BIN stores it.
const SECTOR_SIZE: u64 = 2352;
/// The user data of a data sector, which is what an ISO keeps.
const USER_DATA_SIZE: usize = 2048;
/// CD sectors per second, the unit of a CUE's `mm:ss:ff` timestamps.
const SECTORS_PER_SECOND: u64 = 75;

/// Where a CUE's first track lies in its BIN, in sectors, and where the user
/// data sits in each of them.
#[derive(Debug, PartialEq)]
struct DataTrack {
    /// The BIN the CUE names for this track, when it names one.
    bin: Option<String>,
    start: u64,
    /// Exclusive; `None` when the track runs to the end of the BIN.
    end: Option<u64>,
    offset: usize,
}

/// Find a CUE's first track the way bchunk does: it starts at its last INDEX
/// (INDEX 01, past any pregap) and ends one sector before the next track's
/// first INDEX, or with its BIN.
fn first_track(cue: &str) -> Result<DataTrack> {
    let mut bin = None;
    let mut mode = None;
    let mut start = None;
    let mut end = None;
    let mut tracks = 0;
    for line in cue.lines() {
        match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            // The name is quoted, so take it from the line rather than a word.
            ["FILE", ..] if tracks == 0 => bin = line.split('"').nth(1).map(str::to_string),
            // The next track lives in another BIN: this one runs to its end.
            ["FILE", ..] => break,
            ["TRACK", _, track_mode] => {
                tracks += 1;
                if tracks == 1 {
                    mode = Some(track_mode.to_uppercase());
                }
            }
            ["INDEX", _, time] if tracks == 1 => start = Some(sectors(time)?),
            ["INDEX", _, time] if tracks == 2 => {
                end = Some(sectors(time)?);
                break;
            }
            _ => {}
        }
    }
    let offset = match mode.as_deref() {
        Some("MODE1/2352") => 16,
        Some("MODE2/2352") => 24,
        Some(mode) => bail!(
            "The first track is {mode}, but only MODE1/2352 and MODE2/2352 data tracks convert to ISO"
        ),
        None => bail!("No track in the CUE"),
    };
    let start = start.context("The first track has no INDEX")?;
    Ok(DataTrack {
        bin,
        start,
        end,
        offset,
    })
}

/// A CUE `mm:ss:ff` timestamp, in sectors.
fn sectors(time: &str) -> Result<u64> {
    let parts = time
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|parts| parts.len() == 3)
        .with_context(|| format!("Invalid CUE timestamp \"{time}\""))?;
    Ok((parts[0] * 60 + parts[1]) * SECTORS_PER_SECOND + parts[2])
}

/// Write the user data of `track`'s sectors from `bin` to `iso`.
fn extract(bin: &Path, iso: &Path, track: &DataTrack, progress_bar: &ProgressBar) -> Result<()> {
    let mut input =
        File::open(bin).with_context(|| format!("Failed to open \"{}\"", bin.display()))?;
    // Only whole sectors count, as in bchunk.
    let sectors = input.metadata()?.len() / SECTOR_SIZE;
    let end = track.end.unwrap_or(sectors);
    if end > sectors || end <= track.start {
        bail!(
            "\"{}\" is too short for the first track the CUE describes",
            bin.display()
        );
    }

    progress_bar.reset();
    progress_bar.set_style(get_bytes_progress_style());
    progress_bar.set_length((end - track.start) * SECTOR_SIZE);

    input.seek(SeekFrom::Start(track.start * SECTOR_SIZE))?;
    let mut reader = BufReader::with_capacity(1 << 20, input);
    let mut writer = BufWriter::with_capacity(
        1 << 20,
        File::create(iso).with_context(|| format!("Failed to create \"{}\"", iso.display()))?,
    );
    let mut sector = [0u8; SECTOR_SIZE as usize];
    for _ in track.start..end {
        reader.read_exact(&mut sector)?;
        writer.write_all(&sector[track.offset..track.offset + USER_DATA_SIZE])?;
        progress_bar.inc(SECTOR_SIZE);
    }
    writer.flush()?;
    Ok(())
}

impl ToIso for CueBinRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        start_action(progress_bar, Some("Creating iso"));

        let path = destination_directory
            .as_ref()
            .join(self.cue_romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let track = first_track(&tokio::fs::read_to_string(&self.cue_romfile.path).await?)?;
        // With a BIN per track, only the one holding the first track is read.
        // A lone BIN is that one whatever the CUE calls it; among several, the
        // CUE has to name it.
        let bin: PathBuf = match (track.bin.as_deref(), self.bin_romfiles.as_slice()) {
            (_, [bin]) => bin,
            (Some(name), bins) => bins
                .iter()
                .find(|romfile| romfile.path.file_name().and_then(|f| f.to_str()) == Some(name))
                .with_context(|| {
                    format!("The CUE names \"{name}\", which is not one of its BINs")
                })?,
            (None, _) => bail!("The CUE names no BIN for its first track"),
        }
        .path
        .clone();
        let iso = path.clone();
        let bar = progress_bar.clone();
        spawn_blocking(move || extract(&bin, &iso, &track, &bar))
            .await
            .context("CUE/BIN conversion task failed")??;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_track_starts_past_its_pregap() {
        let cue = "FILE \"game.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 00 00:00:00\n    INDEX 01 00:03:06\n";
        assert_eq!(
            first_track(cue).unwrap(),
            DataTrack {
                bin: Some("game.bin".to_string()),
                start: 231,
                end: None,
                offset: 16
            }
        );
    }

    #[test]
    fn the_next_track_ends_the_first_at_its_first_index() {
        let cue = "FILE \"game.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 00:30:00\n    INDEX 01 00:32:00\n";
        assert_eq!(
            first_track(cue).unwrap(),
            DataTrack {
                bin: Some("game.bin".to_string()),
                start: 0,
                end: Some(2250),
                offset: 24
            }
        );
    }

    #[test]
    fn a_next_track_in_another_bin_leaves_the_first_whole() {
        let cue = "FILE \"1.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\nFILE \"2.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n";
        let track = first_track(cue).unwrap();
        assert_eq!(track.end, None);
        // The first track is in the first BIN, not whichever arrives first.
        assert_eq!(track.bin.as_deref(), Some("1.bin"));
    }

    #[test]
    fn a_bin_too_short_for_its_track_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let (bin, iso) = (
            directory.path().join("game.bin"),
            directory.path().join("game.iso"),
        );
        std::fs::write(&bin, vec![0; 10 * SECTOR_SIZE as usize]).unwrap();
        let progress_bar = ProgressBar::hidden();
        for (start, end) in [(231, None), (0, Some(11)), (5, Some(5))] {
            let track = DataTrack {
                bin: None,
                start,
                end,
                offset: 16,
            };
            assert!(
                extract(&bin, &iso, &track, &progress_bar).is_err(),
                "{start}..{end:?}"
            );
        }
    }

    #[test]
    fn only_2352_byte_data_tracks_convert() {
        for mode in ["AUDIO", "MODE1/2048", "MODE2/2336"] {
            let cue =
                format!("FILE \"game.bin\" BINARY\n  TRACK 01 {mode}\n    INDEX 01 00:00:00\n");
            assert!(first_track(&cue).is_err(), "{mode}");
        }
    }
}
