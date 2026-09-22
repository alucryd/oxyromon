//! Dreamcast GD-ROM CUE/BIN to GDI conversion: a port of [gdidrop].
//!
//! Redump dumps GD-ROMs as a CUE with one BIN per track; optical drive
//! emulators such as GDEMU load GDI instead. The track data is the same, save
//! for pregaps: a GDI track starts at its INDEX 01, so the pregap before it is
//! dropped, and the GDI descriptor lists where each track sits on the disc
//! instead, with the high-density area starting at sector 45000. A CUE whose
//! BIN holds several tracks converts too, as if Redump had split it.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let gdi = gdi_rs::convert(Path::new("game.cue"), Path::new("out"), &mut |_| {}).unwrap();
//! println!("{} and {} tracks", gdi.gdi.display(), gdi.tracks.len());
//! ```
//!
//! [gdidrop]: https://github.com/ElektroStudios/gdidrop-Dreamcast-Redump-Tool

mod cue;
mod error;

pub use error::{Error, Result};

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// A raw CD sector, as both formats store it.
const SECTOR_SIZE: u64 = 2352;
/// Where a GD-ROM's high-density area starts.
const HIGH_DENSITY_START: u64 = 45000;

/// The files a conversion wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gdi {
    /// The GDI descriptor, named after the CUE.
    pub gdi: PathBuf,
    /// One file per track, in track order: `.bin` for data, `.raw` for audio,
    /// named after the track's BIN, and its number when the BIN holds several.
    pub tracks: Vec<PathBuf>,
}

/// One track's copy: its share of `input` is `start..end`, of which
/// `begin..end` is copied to `output` and the pregap before `begin` dropped.
struct Copy {
    input: PathBuf,
    output: PathBuf,
    start: u64,
    begin: u64,
    end: u64,
}

/// Convert a CUE/BIN set into a GDI set in `output_dir`.
///
/// The BINs are read from the CUE's directory, which `output_dir` may only be
/// when no track would overwrite one of them. `progress` is called with each
/// newly consumed chunk of the BINs, in bytes; the calls add up to
/// [`input_size`]. On error the partial output is removed.
pub fn convert(cue: &Path, output_dir: &Path, progress: &mut dyn FnMut(u64)) -> Result<Gdi> {
    let (descriptor, copies) = plan(cue, output_dir)?;
    let gdi = output_dir
        .join(cue.file_stem().unwrap_or_default())
        .with_extension("gdi");

    // Everything is validated: from here on a failure removes what was written.
    let mut written = Vec::with_capacity(copies.len() + 1);
    let result = (|| {
        for copy in &copies {
            written.push(copy.output.clone());
            copy_track(copy, progress)?;
        }
        written.push(gdi.clone());
        std::fs::write(&gdi, &descriptor)?;
        Ok(())
    })();
    if let Err(error) = result {
        for path in &written {
            let _ = std::fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(Gdi {
        gdi,
        tracks: copies.into_iter().map(|copy| copy.output).collect(),
    })
}

/// The total size of the BINs a CUE names: what [`convert`] reports progress
/// against.
pub fn input_size(cue: &Path) -> Result<u64> {
    let directory = cue.parent().unwrap_or(Path::new(""));
    let tracks = cue::parse(&std::fs::read_to_string(cue)?)?.tracks;
    let mut size = 0;
    for (i, track) in tracks.iter().enumerate() {
        // Each BIN once, however many tracks it holds.
        if i == 0 || tracks[i - 1].file_index != track.file_index {
            size += std::fs::metadata(directory.join(&track.file))?.len();
        }
    }
    Ok(size)
}

/// Lay the disc out as gdidrop does, and check every track can be copied,
/// without writing anything.
fn plan(cue_path: &Path, output_dir: &Path) -> Result<(String, Vec<Copy>)> {
    let cue = cue::parse(&std::fs::read_to_string(cue_path)?)?;
    let directory = cue_path.parent().unwrap_or(Path::new(""));
    if !output_dir.is_dir() {
        return Err(Error::InvalidOption(format!(
            "output directory \"{}\" does not exist",
            output_dir.display()
        )));
    }
    let canonical_output = output_dir.canonicalize()?;

    let tracks = &cue.tracks;
    let shares_file = |i: usize, j: usize| {
        tracks
            .get(j)
            .is_some_and(|other| other.file_index == tracks[i].file_index)
    };

    let mut sector = 0;
    let mut descriptor = format!("{}\n", tracks.len());
    let mut copies: Vec<Copy> = Vec::with_capacity(tracks.len());
    let mut names = HashSet::new();
    for (i, track) in tracks.iter().enumerate() {
        let input = directory.join(&track.file);
        let size = std::fs::metadata(&input)?.len();
        let first = i == 0 || !shares_file(i, i - 1);
        let last = !shares_file(i, i + 1);

        // A BIN holding several tracks is split at each one's first INDEX, as
        // Redump splits them, so a pregap goes with its own track. A track
        // alone in its BIN has all of it, as in gdidrop.
        let start = if first {
            0
        } else {
            track.indices[0] * SECTOR_SIZE
        };
        let end = if last {
            size
        } else {
            tracks[i + 1].indices[0] * SECTOR_SIZE
        };
        // A track with a pregap starts at its INDEX 01; the pregap is dropped.
        let begin = track
            .indices
            .get(1)
            .map_or(start, |index| index * SECTOR_SIZE);
        if !(start <= begin && begin <= end && end <= size) {
            return Err(Error::Corrupt(format!(
                "track {}'s INDEX lies outside its part of \"{}\"",
                track.number, track.file
            )));
        }
        sector += (begin - start) / SECTOR_SIZE;

        let stem = Path::new(&track.file)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let extension = if track.audio { "raw" } else { "bin" };
        let name = if first && last {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} (Track {:02}).{extension}", track.number)
        };
        if !names.insert(name.clone()) {
            return Err(Error::InvalidOption(format!(
                "two tracks would both be written to \"{name}\""
            )));
        }
        descriptor.push_str(&format!(
            "{} {} {} {SECTOR_SIZE} \"{name}\" 0\n",
            track.number,
            sector,
            if track.audio { 0 } else { 4 }
        ));
        sector += (end - begin) / SECTOR_SIZE;

        // An exact match, as gdidrop does it.
        if track
            .comments
            .iter()
            .any(|comment| comment == "HIGH-DENSITY AREA")
        {
            sector = sector.max(HIGH_DENSITY_START);
        }
        copies.push(Copy {
            input,
            output: output_dir.join(name),
            start,
            begin,
            end,
        });
    }

    // No track may be written over a BIN the CUE reads.
    let inputs = copies
        .iter()
        .map(|copy| copy.input.canonicalize())
        .collect::<std::io::Result<HashSet<_>>>()?;
    for copy in &copies {
        let name = copy.output.file_name().unwrap_or_default();
        if inputs.contains(&canonical_output.join(name)) {
            return Err(Error::InvalidOption(format!(
                "writing \"{}\" to {} would overwrite a BIN it reads",
                name.to_string_lossy(),
                output_dir.display()
            )));
        }
    }
    Ok((descriptor, copies))
}

/// Copy one track, pregap dropped, reporting the pregap as consumed too.
fn copy_track(copy: &Copy, progress: &mut dyn FnMut(u64)) -> Result<()> {
    let mut input = File::open(&copy.input)?;
    input.seek(SeekFrom::Start(copy.begin))?;
    progress(copy.begin - copy.start);
    let mut input = input.take(copy.end - copy.begin);
    let mut output = File::create(&copy.output)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
        progress(n as u64);
    }
    output.flush()?;
    Ok(())
}
