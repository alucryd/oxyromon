//! Cue sheets, as Redump writes them: a `FILE` a track, each `TRACK` raw,
//! 2352 bytes a sector, its `INDEX`es counted from the start of its file.

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
        let text = "FILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\nFILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n";
        let cue = parse(text, Path::new("/d")).unwrap();
        assert_eq!(cue.tracks.len(), 2);
        assert_eq!(cue.tracks[0].mode, Mode::Mode2);
        assert_eq!(cue.tracks[1].file, Path::new("/d/Game (Track 2).bin"));
        assert_eq!(cue.tracks[1].indexes, [(0, 0), (1, 150)]);
        assert!(parse("TRACK 01 MODE1/2048\n", Path::new("")).is_err());
    }
}
