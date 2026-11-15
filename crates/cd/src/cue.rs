// SPDX-License-Identifier: BSD-3-Clause
//! Cue sheets, as Redump writes them: a `FILE` a track, each `TRACK` raw,
//! 2352 bytes a sector, its `INDEX`es counted from the start of its file;
//! a GD-ROM's tracks after its `REM HIGH-DENSITY AREA` in that area.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Audio,
    Mode1,
    Mode2,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub number: u8,
    pub mode: Mode,
    pub file: PathBuf,
    /// Index number and where it starts in the file, in frames.
    pub indexes: Vec<(u8, u32)>,
    /// A `PREGAP` the file does not hold, in frames.
    pub pregap: u32,
    /// In a GD-ROM's high-density area, which starts at sector 45,000.
    pub high_density: bool,
    /// Its `FLAGS`, as the Q subchannel's control bits: 4CH 0x8, DCP 0x2,
    /// PRE 0x1.
    pub flags: u8,
}

/// Where a GD-ROM's high-density area starts, in sectors.
pub const HIGH_DENSITY_AREA: u32 = 45_000;

/// Where each track's file starts on the disc, in frames from 00:00:00:
/// after `lead_in`, the tracks before, its pregap, and for the first track
/// of a high-density area that area's start. `sectors` gives each track's
/// file length in sectors.
pub fn starts(cue: &Cue, lead_in: u32, sectors: impl IntoIterator<Item = u64>) -> Vec<u64> {
    let mut frame = u64::from(lead_in);
    let mut high_density = false;
    cue.tracks
        .iter()
        .zip(sectors)
        .map(|(track, sectors)| {
            if track.high_density && !high_density {
                frame = frame.max(u64::from(lead_in + HIGH_DENSITY_AREA));
            }
            high_density = track.high_density;
            frame += u64::from(track.pregap);
            let start = frame;
            frame += sectors;
            start
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct Cue {
    pub tracks: Vec<Track>,
}

fn invalid(what: String) -> Error {
    Error::new(ErrorKind::InvalidData, format!("cue sheet: {what}"))
}

/// `mm:ss:ff` in frames.
pub fn frames(msf: &str) -> Option<u32> {
    let mut parts = msf.split(':').map(|part| part.parse::<u32>().ok());
    let (m, s, f) = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && s < 60 && f < 75).then_some((m * 60 + s) * 75 + f)
}

/// Parse a cue sheet, its files found relative to `dir`.
pub fn parse(text: &str, dir: &Path) -> Result<Cue> {
    let mut cue = Cue::default();
    let mut file = None;
    let mut high_density = false;
    for line in text.lines() {
        let line = line.trim();
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        match command {
            "FILE" => {
                let name = rest
                    .strip_prefix('"')
                    .and_then(|rest| rest.rsplit_once('"'))
                    .map(|(name, _)| name)
                    .ok_or_else(|| invalid(format!("a file not in quotes: {line}")))?;
                file = Some(dir.join(name));
            }
            "TRACK" => {
                let mut words = rest.split_whitespace();
                let number = words.next().and_then(|n| n.parse().ok());
                let mode = match words.next() {
                    Some("AUDIO") => Mode::Audio,
                    Some("MODE1/2352") => Mode::Mode1,
                    Some("MODE2/2352") => Mode::Mode2,
                    other => return Err(invalid(format!("unsupported track {other:?}"))),
                };
                let (Some(number), Some(file)) = (number, file.clone()) else {
                    return Err(invalid(format!("a track without its file: {line}")));
                };
                cue.tracks.push(Track {
                    number,
                    mode,
                    file,
                    indexes: Vec::new(),
                    pregap: 0,
                    high_density,
                    flags: 0,
                });
            }
            "INDEX" | "PREGAP" => {
                let track = cue
                    .tracks
                    .last_mut()
                    .ok_or_else(|| invalid(format!("{command} before any track")))?;
                let mut words = rest.split_whitespace();
                if command == "PREGAP" {
                    track.pregap = words
                        .next()
                        .and_then(frames)
                        .ok_or_else(|| invalid(line.into()))?;
                } else {
                    let number = words.next().and_then(|n| n.parse().ok());
                    let at = words.next().and_then(frames);
                    let (Some(number), Some(at)) = (number, at) else {
                        return Err(invalid(line.into()));
                    };
                    track.indexes.push((number, at));
                }
            }
            "FLAGS" => {
                let track = cue
                    .tracks
                    .last_mut()
                    .ok_or_else(|| invalid(format!("{command} before any track")))?;
                for flag in rest.split_whitespace() {
                    track.flags |= match flag {
                        "4CH" => 0x8,
                        "DCP" => 0x2,
                        "PRE" => 0x1,
                        // Serial copy management, not in the Q subchannel
                        "SCMS" => 0,
                        other => return Err(invalid(format!("unknown flag {other:?}"))),
                    };
                }
            }
            "REM" => match rest.trim() {
                "HIGH-DENSITY AREA" => high_density = true,
                "SINGLE-DENSITY AREA" => high_density = false,
                _ => {}
            },
            _ => {}
        }
    }
    if cue.tracks.is_empty() {
        return Err(invalid("no track".into()));
    }
    Ok(cue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_redump_cue_sheet() {
        let text = "FILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\nFILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    FLAGS DCP PRE SCMS\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n";
        let cue = parse(text, Path::new("/d")).unwrap();
        assert_eq!(cue.tracks.len(), 2);
        assert_eq!(cue.tracks[0].mode, Mode::Mode2);
        assert_eq!(cue.tracks[1].file, Path::new("/d/Game (Track 2).bin"));
        assert_eq!(cue.tracks[1].indexes, [(0, 0), (1, 150)]);
        assert_eq!((cue.tracks[0].flags, cue.tracks[1].flags), (0, 0x3));
        assert!(parse("TRACK 01 MODE1/2048\n", Path::new("")).is_err());
        assert!(parse("FLAGS DCP\n", Path::new("")).is_err());
        let typo = text.replace("FLAGS DCP PRE SCMS", "FLAGS DCPP");
        assert!(parse(&typo, Path::new("")).is_err());
    }

    #[test]
    fn a_gd_rom_starts_its_high_density_area_at_45000() {
        let text = "REM SINGLE-DENSITY AREA\r\nFILE \"1.bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\nFILE \"2.bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\nREM HIGH-DENSITY AREA\r\nFILE \"3.bin\" BINARY\r\n  TRACK 03 MODE1/2352\r\n    INDEX 01 00:00:00\r\nFILE \"4.bin\" BINARY\r\n  TRACK 04 AUDIO\r\n    INDEX 01 00:00:00\r\n";
        let cue = parse(text, Path::new("")).unwrap();
        let areas: Vec<bool> = cue.tracks.iter().map(|t| t.high_density).collect();
        assert_eq!(areas, [false, false, true, true]);
        assert_eq!(
            starts(&cue, 150, [300, 750, 1000, 50]),
            [150, 450, 45150, 46150]
        );
        // A CD's tracks follow each other, a pregap between them
        let mut cd = cue.clone();
        cd.tracks.iter_mut().for_each(|t| t.high_density = false);
        cd.tracks[1].pregap = 150;
        assert_eq!(
            starts(&cd, 150, [300, 750, 1000, 50]),
            [150, 600, 1350, 2350]
        );
    }
}
