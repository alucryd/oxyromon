//! The part of a CUE sheet the GDI conversion reads, parsed the way gdidrop's
//! CueSharp does.

use crate::error::{Error, Result};

/// CD frames (sectors) per second, the unit of a CUE's `mm:ss:ff` timestamps.
const FRAMES_PER_SECOND: u64 = 75;

pub struct Cue {
    pub tracks: Vec<Track>,
}

pub struct Track {
    pub number: u32,
    pub audio: bool,
    /// The BIN holding this track, as the CUE names it.
    pub file: String,
    /// Which FILE line named it: tracks sharing one share a BIN.
    pub file_index: usize,
    /// Each INDEX, in frames from the start of the BIN, in CUE order.
    pub indices: Vec<u64>,
    /// INDEX 01, where the track proper starts, past any pregap (INDEX 00).
    pub index_01: Option<u64>,
    /// `REM` lines read while this was the current track.
    pub comments: Vec<String>,
}

/// Parse a CUE sheet.
///
/// A track is in the FILE named last before it, which may hold several. A
/// `REM` belongs to the track being read when it appears, as in gdidrop, so
/// Redump's `REM HIGH-DENSITY AREA`, written before track 3's FILE, lands on
/// track 2.
pub fn parse(text: &str) -> Result<Cue> {
    let mut tracks: Vec<Track> = Vec::new();
    let mut file: Option<String> = None;
    let mut files = 0;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let keyword = line.split_whitespace().next().unwrap_or_default();
        match keyword.to_ascii_uppercase().as_str() {
            "FILE" => {
                file = Some(file_name(line)?);
                files += 1;
            }
            "TRACK" => {
                let mut words = line.split_whitespace().skip(1);
                let (Some(number), Some(mode)) = (words.next(), words.next()) else {
                    return Err(Error::Corrupt(format!("invalid TRACK line \"{line}\"")));
                };
                let number = number
                    .parse()
                    .map_err(|_| Error::Corrupt(format!("invalid track number in \"{line}\"")))?;
                let file = file.clone().ok_or_else(|| {
                    Error::Corrupt(format!("track {number} comes before any FILE"))
                })?;
                tracks.push(Track {
                    number,
                    audio: mode.eq_ignore_ascii_case("AUDIO"),
                    file,
                    file_index: files - 1,
                    indices: Vec::new(),
                    index_01: None,
                    comments: Vec::new(),
                });
            }
            "INDEX" => {
                let track = tracks
                    .last_mut()
                    .ok_or_else(|| Error::Corrupt("INDEX before any TRACK".into()))?;
                let invalid = || Error::Corrupt(format!("invalid INDEX line \"{line}\""));
                let mut words = line.split_whitespace().skip(1);
                let number: u32 = words
                    .next()
                    .and_then(|number| number.parse().ok())
                    .ok_or_else(invalid)?;
                let frames = frames(words.next().ok_or_else(invalid)?)?;
                if number == 1 {
                    track.index_01 = Some(frames);
                }
                track.indices.push(frames);
            }
            "REM" => {
                let comment = line[keyword.len()..].trim();
                if let (Some(track), false) = (tracks.last_mut(), comment.is_empty()) {
                    track.comments.push(comment.to_string());
                }
            }
            _ => {}
        }
    }
    if tracks.is_empty() {
        return Err(Error::Corrupt("no TRACK in the CUE".into()));
    }
    if let Some(track) = tracks.iter().find(|track| track.indices.is_empty()) {
        return Err(Error::Corrupt(format!(
            "track {} has no INDEX",
            track.number
        )));
    }
    Ok(Cue { tracks })
}

/// The name in a FILE line, between its first and last quote, or its second
/// word when it is not quoted.
fn file_name(line: &str) -> Result<String> {
    let name = match (line.find('"'), line.rfind('"')) {
        (Some(start), Some(end)) if start < end => &line[start + 1..end],
        _ => line.split_whitespace().nth(1).unwrap_or_default(),
    };
    if name.is_empty() {
        return Err(Error::Corrupt(format!("invalid FILE line \"{line}\"")));
    }
    Ok(name.to_string())
}

/// A `mm:ss:ff` timestamp, in frames.
fn frames(time: &str) -> Result<u64> {
    let parts: Vec<u64> = time
        .split(':')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| Error::Corrupt(format!("invalid CUE timestamp \"{time}\"")))?;
    match parts[..] {
        [minutes, seconds, frames] => Ok((minutes * 60 + seconds) * FRAMES_PER_SECOND + frames),
        _ => Err(Error::Corrupt(format!("invalid CUE timestamp \"{time}\""))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_belongs_to_the_track_being_read() {
        let cue = parse(
            "REM SINGLE-DENSITY AREA\r\nFILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\nREM HIGH-DENSITY AREA\r\nFILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n",
        )
        .unwrap();
        assert_eq!(cue.tracks[0].comments, ["HIGH-DENSITY AREA"]);
        assert!(cue.tracks[1].comments.is_empty());
        assert_eq!(cue.tracks[1].file, "Game (Track 2).bin");
        assert!(cue.tracks[1].audio);
        assert_eq!(cue.tracks[1].indices, [0, 150]);
        assert_eq!(cue.tracks[1].index_01, Some(150));
    }

    #[test]
    fn tracks_can_share_a_file() {
        let cue = parse(
            "FILE \"game.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:30:00\nFILE \"other.bin\" BINARY\n  TRACK 03 MODE1/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        assert_eq!(cue.tracks[1].file, "game.bin");
        assert_eq!(cue.tracks[0].file_index, cue.tracks[1].file_index);
        assert_ne!(cue.tracks[1].file_index, cue.tracks[2].file_index);
    }

    #[test]
    fn index_01_is_found_by_its_number() {
        let cue = parse(
            "FILE \"game.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n    INDEX 02 00:04:00\n",
        )
        .unwrap();
        assert_eq!(cue.tracks[0].index_01, Some(150));
        assert!(parse("FILE \"game.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 1\n").is_err());
    }

    #[test]
    fn a_track_needs_a_file() {
        let result = parse("  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n");
        assert!(matches!(result, Err(Error::Corrupt(_))));
    }

    #[test]
    fn a_track_needs_an_index() {
        let result = parse("FILE \"game.bin\" BINARY\n  TRACK 01 MODE1/2352\n");
        assert!(matches!(result, Err(Error::Corrupt(_))));
    }

    #[test]
    fn timestamps_count_frames() {
        assert_eq!(frames("00:02:00").unwrap(), 150);
        assert_eq!(frames("01:00:05").unwrap(), 4505);
        assert!(frames("00:02").is_err());
        assert!(frames("aa:02:00").is_err());
    }
}
