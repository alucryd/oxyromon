//! The CD table of contents: its model, its parsers and its CHD metadata.
//!
//! This mirrors `cdrom_file` from MAME's `src/lib/util/cdrom.cpp`, keeping
//! its quirks verbatim: the plain `PGTYPE` branch reports the pregap track
//! type, not the track type, and the `V` prefix that marks a valid pregap
//! submode only appears when the pregap carries data.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::container::{MDFLAGS_CHECKSUM, MTAG_GDROM_TRACK, MTAG_TRACK2};
use crate::error::{Error, Result};

/// Frames a CD track is padded to a multiple of when written to a CHD.
pub(crate) const TRACK_PADDING: u32 = 4;
/// The highest track count a CD table of contents can hold.
pub(crate) const MAX_TRACKS: usize = 99;
/// The highest index a CUE track can declare.
const MAX_INDEX: usize = 99;
/// The physical frame offset at which a GDI high-density area starts.
pub(crate) const GDI_HIGH_DENSITY_AREA: i32 = 45000;

// track types, the indices `get_type_string()` maps
/// Mode 1, 2048 bytes per sector.
pub(crate) const TRACK_MODE1: u32 = 0;
/// Mode 1 raw, 2352 bytes per sector.
pub(crate) const TRACK_MODE1_RAW: u32 = 1;
/// Mode 2, 2336 bytes per sector.
pub(crate) const TRACK_MODE2: u32 = 2;
/// Mode 2 form 1, 2048 bytes per sector.
pub(crate) const TRACK_MODE2_FORM1: u32 = 3;
/// Mode 2 form 2, 2324 bytes per sector.
pub(crate) const TRACK_MODE2_FORM2: u32 = 4;
/// Mode 2 form mix, 2336 bytes per sector.
pub(crate) const TRACK_MODE2_FORM_MIX: u32 = 5;
/// Mode 2 raw, 2352 bytes per sector.
pub(crate) const TRACK_MODE2_RAW: u32 = 6;
/// Redbook audio, 2352 bytes per sector.
pub(crate) const TRACK_AUDIO: u32 = 7;

// subcode types, the indices `get_subtype_string()` maps
/// Cooked subcode, 96 bytes per frame.
pub(crate) const SUB_NORMAL: u32 = 0;
/// Raw uninterleaved subcode, 96 bytes per frame.
pub(crate) const SUB_RAW: u32 = 1;
/// No subcode at all.
pub(crate) const SUB_NONE: u32 = 2;

// table of contents flags
/// The disc is a GD-ROM, so its tracks carry GD-ROM metadata.
pub(crate) const FLAG_GDROM: u32 = 0x01;
/// A legacy GD-ROM, whose audio is stored little-endian.
pub(crate) const FLAG_GDROMLE: u32 = 0x02;
/// The disc spans more than one session.
pub(crate) const FLAG_MULTISESSION: u32 = 0x04;

/// The name of a track type, as it appears in metadata.
pub(crate) fn type_string(track_type: u32) -> &'static str {
    match track_type {
        TRACK_MODE1 => "MODE1",
        TRACK_MODE1_RAW => "MODE1_RAW",
        TRACK_MODE2 => "MODE2",
        TRACK_MODE2_FORM1 => "MODE2_FORM1",
        TRACK_MODE2_FORM2 => "MODE2_FORM2",
        TRACK_MODE2_FORM_MIX => "MODE2_FORM_MIX",
        TRACK_MODE2_RAW => "MODE2_RAW",
        TRACK_AUDIO => "AUDIO",
        _ => "UNKNOWN",
    }
}

/// The name of a subcode type, as it appears in metadata.
pub(crate) fn subtype_string(sub_type: u32) -> &'static str {
    match sub_type {
        SUB_NORMAL => "RW",
        SUB_RAW => "RW_RAW",
        _ => "NONE",
    }
}

/// One track of a CD table of contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackInfo {
    /// The track type, one of the `TRACK_*` constants.
    pub trktype: u32,
    /// The subcode type, one of the `SUB_*` constants.
    pub subtype: u32,
    /// The track data size in bytes: 2352 for raw tracks and audio.
    pub datasize: u32,
    /// The subcode size in bytes, 0 or 96.
    pub subsize: u32,
    /// The length of the track in frames.
    pub frames: i32,
    /// The number of padding frames added when written to a CHD.
    pub extraframes: i32,
    /// The length of the pregap in frames.
    pub pregap: i32,
    /// The length of the postgap in frames.
    pub postgap: i32,
    /// The pregap track type.
    pub pgtype: u32,
    /// The pregap subcode type.
    pub pgsub: u32,
    /// The pregap data size, 0 when the pregap carries no data.
    pub pgdatasize: i32,
    /// The pregap subcode size.
    pub pgsubsize: i32,
    /// The Q-channel control bits of the track.
    pub control_flags: u32,
    /// The zero-based session the track belongs to.
    pub session: u32,
    /// Frames of zero padding that trail the track in its input file.
    pub padframes: i32,
    /// Frames that spill into the next input file of a split track.
    pub splitframes: i32,
    /// The physical frame offset of the track on the disc.
    pub physframeofs: i32,
    /// The frame the track starts at in a CHD, once read back from one.
    pub chdframeofs: i32,
    /// The GDI density area the track belongs to.
    pub multicuearea: i32,
}

impl Default for TrackInfo {
    /// A silent track with no subcode: the CUE and GDI parsers reset both
    /// subcode types to `SUB_NONE` for every track, where the derived zero
    /// would read as `SUB_NORMAL`.
    fn default() -> Self {
        Self {
            trktype: 0,
            subtype: SUB_NONE,
            datasize: 0,
            subsize: 0,
            frames: 0,
            extraframes: 0,
            pregap: 0,
            postgap: 0,
            pgtype: 0,
            pgsub: SUB_NONE,
            pgdatasize: 0,
            pgsubsize: 0,
            control_flags: 0,
            session: 0,
            padframes: 0,
            splitframes: 0,
            physframeofs: 0,
            chdframeofs: 0,
            multicuearea: 0,
        }
    }
}

/// A CD table of contents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Toc {
    /// The number of tracks.
    pub numtrks: usize,
    /// The number of sessions.
    pub numsessions: u32,
    /// The `FLAG_*` bits of the disc.
    pub flags: u32,
    /// The tracks, `numtrks` of them.
    pub tracks: Vec<TrackInfo>,
}

/// The metadata entries of a table of contents, in the order and with the
/// tags, flags and bytes chdman 0.289's `cdrom_file::write_metadata()`
/// writes them: one `CHT2` or `CHGD` entry per track. Sessions get no entry
/// of their own there; later MAME adds `CHSE` ones.
pub(crate) fn metadata_entries(toc: &Toc) -> Vec<(u32, u8, Vec<u8>)> {
    let mut entries = Vec::new();

    for (i, track) in toc.tracks.iter().enumerate() {
        // the pregap submode, with a 'V' prefix where it carries data
        let submode = if track.pgdatasize > 0 {
            format!("V{}", type_string(track.pgtype))
        } else {
            type_string(track.pgtype).to_owned()
        };

        let (tag, metadata) = if toc.flags & FLAG_GDROM != 0 {
            (
                MTAG_GDROM_TRACK,
                format!(
                    "TRACK:{} TYPE:{} SUBTYPE:{} FRAMES:{} PAD:{} PREGAP:{} PGTYPE:{} PGSUB:{} POSTGAP:{}",
                    i + 1,
                    type_string(track.trktype),
                    subtype_string(track.subtype),
                    track.frames,
                    track.padframes,
                    track.pregap,
                    type_string(track.pgtype),
                    subtype_string(track.pgsub),
                    track.postgap,
                ),
            )
        } else {
            (
                MTAG_TRACK2,
                format!(
                    "TRACK:{} TYPE:{} SUBTYPE:{} FRAMES:{} PREGAP:{} PGTYPE:{} PGSUB:{} POSTGAP:{}",
                    i + 1,
                    type_string(track.trktype),
                    subtype_string(track.subtype),
                    track.frames,
                    track.pregap,
                    submode,
                    subtype_string(track.pgsub),
                    track.postgap,
                ),
            )
        };
        entries.push(cstr_entry(tag, &metadata));
    }

    entries
}

/// A metadata entry: text entries are NUL-terminated and checksummed.
fn cstr_entry(tag: u32, text: &str) -> (u32, u8, Vec<u8>) {
    let mut data = text.as_bytes().to_vec();
    data.push(0);
    (tag, MDFLAGS_CHECKSUM, data)
}

// ---------------------------------------------------------------------------
// Parsing a table of contents: `cdrom_file::parse_toc` and the parsers it
// dispatches to. The C reads lines with `fgets` into a 512-byte buffer and
// splits them with its own `tokenize`; both are reproduced byte for byte,
// quirks included, because they decide what ends up in the metadata.
// ---------------------------------------------------------------------------

/// A track slot of the parsers' tables: `toc::tracks` holds `MAX_TRACKS + 1`.
const TRACK_SLOTS: usize = MAX_TRACKS + 1;
/// The longest line `fgets(linebuffer, 511, file)` returns at once.
const LINE_CHUNK: usize = 510;
/// The Q-channel control bits a CUE `FLAGS` line sets.
const CONTROL_PREEMPHASIS: u32 = 1;
const CONTROL_DIGITAL_COPY_PERMITTED: u32 = 2;
const CONTROL_4CH: u32 = 8;
/// The GD-ROM density areas of a multi-CUE Dreamcast dump.
const SINGLE_DENSITY: i32 = 0;
const HIGH_DENSITY: i32 = 1;

/// Where the frames of one track come from: `track_input_entry`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackInput {
    /// The file holding the track, its directory prefixed the way the C does.
    pub fname: String,
    /// The byte offset of the track's first frame in that file.
    pub offset: u32,
    /// Whether the samples are byte-swapped on the way in.
    pub swap: bool,
    /// The CUE `INDEX` frames, -1 where absent.
    pub idx: [i32; MAX_INDEX + 1],
    /// The lead-in and lead-out of a multisession CUE, -1 where absent.
    pub leadin: i32,
    pub leadout: i32,
}

impl Default for TrackInput {
    fn default() -> Self {
        Self {
            fname: String::new(),
            offset: 0,
            swap: false,
            idx: [-1; MAX_INDEX + 1],
            leadin: -1,
            leadout: -1,
        }
    }
}

/// A track as `memset(&outtoc, 0, ...)` leaves it: every field zero.
fn zeroed_track() -> TrackInfo {
    TrackInfo {
        subtype: 0,
        pgsub: 0,
        ..TrackInfo::default()
    }
}

/// `isspace` in the C locale.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The lines `fgets(linebuffer, 511, file)` returns: at most 510 bytes, up
/// to and including a newline, each seen as the C string it holds — up to
/// its first NUL.
fn c_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let limit = rest.len().min(LINE_CHUNK);
        let end = rest[..limit]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(limit, |newline| newline + 1);
        let line = &rest[..end];
        lines.push(&line[..line.iter().position(|&b| b == 0).unwrap_or(line.len())]);
        rest = &rest[end..];
    }
    lines
}

/// The byte at `i` of a C string, NUL past its end.
fn at(line: &[u8], i: usize) -> u8 {
    line.get(i).copied().unwrap_or(0)
}

/// `cdrom_file::tokenize`: skips leading spaces, then gathers bytes up to
/// the next unquoted space, dropping the quote characters themselves. The
/// space that ends a token is not consumed.
fn tokenize(line: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut token = Vec::new();
    let mut singlequote = false;
    let mut doublequote = false;
    while is_space(at(line, i)) {
        i += 1;
    }
    while at(line, i) != 0 {
        let byte = line[i];
        if !singlequote && byte == b'"' {
            doublequote = !doublequote;
        } else if !doublequote && byte == b'\'' {
            singlequote = !singlequote;
        } else if !singlequote && !doublequote && is_space(byte) {
            break;
        } else {
            token.push(byte);
        }
        i += 1;
    }
    (token, i)
}

/// Skips the spaces at `i`, the way the parsers do before a `strncmp`.
fn skip_spaces(line: &[u8], mut i: usize) -> usize {
    while is_space(at(line, i)) {
        i += 1;
    }
    i
}

/// The digits after optional spaces and sign, as `atoi`/`strtoul` read
/// them: `(negative, value, digits seen)`, the value saturating.
fn scan_digits(text: &[u8], mut i: usize) -> (bool, u64, usize, usize) {
    while is_space(at(text, i)) {
        i += 1;
    }
    let negative = at(text, i) == b'-';
    if matches!(at(text, i), b'-' | b'+') {
        i += 1;
    }
    let mut value = 0u64;
    let mut digits = 0;
    while at(text, i).is_ascii_digit() {
        value = value
            .saturating_mul(10)
            .saturating_add(u64::from(at(text, i) - b'0'));
        digits += 1;
        i += 1;
    }
    (negative, value, digits, i)
}

/// `atoi`: 0 when there is no number.
fn atoi(text: &[u8]) -> i32 {
    let (negative, value, _, _) = scan_digits(text, 0);
    let value = value as i32;
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// `strtoul(text, nullptr, 10)`, a negative number wrapping around.
fn strtoul(text: &[u8]) -> u64 {
    let (negative, value, _, _) = scan_digits(text, 0);
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// `cdrom_file::msf_to_frames`: `sscanf(token, "%d:%d:%d")`, where a lone
/// number is a frame count and anything else minutes, seconds and frames.
fn msf_to_frames(token: &[u8]) -> i32 {
    let mut values = [0i32; 3];
    let mut count = 0;
    let mut i = 0;
    for (n, value) in values.iter_mut().enumerate() {
        if n > 0 {
            if at(token, i) != b':' {
                break;
            }
            i += 1;
        }
        let (negative, digits_value, digits, next) = scan_digits(token, i);
        if digits == 0 {
            break;
        }
        let parsed = digits_value as i32;
        *value = if negative {
            parsed.wrapping_neg()
        } else {
            parsed
        };
        count += 1;
        i = next;
    }
    let [m, s, f] = values;
    if count == 1 {
        m
    } else {
        f.wrapping_add(s.wrapping_add(m.wrapping_mul(60)).wrapping_mul(75))
    }
}

/// `cdrom_file::get_file_path`: the directory part of a path, separator
/// included, or nothing.
fn get_file_path(path: &str) -> String {
    match path.rfind('\\').or_else(|| path.rfind('/')) {
        Some(pos) => path[..=pos].to_owned(),
        None => String::new(),
    }
}

/// `cdrom_file::get_file_size`: 0 when the file cannot be opened.
fn get_file_size(fname: &str) -> u64 {
    std::fs::metadata(fname).map_or(0, |metadata| metadata.len())
}

/// A token as part of a file name.
fn token_str(token: &[u8]) -> String {
    String::from_utf8_lossy(token).into_owned()
}

/// `get_info_from_type_string`: the track type and data size a CUE or TOC
/// track type names, `None` where it names none.
fn info_from_type_string(name: &[u8]) -> Option<(u32, u32)> {
    Some(match name {
        b"MODE1" | b"MODE1/2048" => (TRACK_MODE1, 2048),
        b"MODE1_RAW" | b"MODE1/2352" => (TRACK_MODE1_RAW, 2352),
        b"MODE2" | b"MODE2/2336" => (TRACK_MODE2, 2336),
        b"MODE2_FORM1" | b"MODE2/2048" => (TRACK_MODE2_FORM1, 2048),
        b"MODE2_FORM2" | b"MODE2/2324" => (TRACK_MODE2_FORM2, 2324),
        b"MODE2_FORM_MIX" => (TRACK_MODE2_FORM_MIX, 2336),
        b"MODE2_RAW" | b"MODE2/2352" | b"CDI/2352" => (TRACK_MODE2_RAW, 2352),
        b"AUDIO" => (TRACK_AUDIO, 2352),
        _ => return None,
    })
}

/// A slot of the parsers' tables, or the out-of-bounds access the C would
/// make reported as corrupt input.
fn slot(trknum: i64) -> Result<usize> {
    usize::try_from(trknum)
        .ok()
        .filter(|&slot| slot < TRACK_SLOTS)
        .ok_or_else(|| Error::Corrupt(format!("track {} is out of range", trknum + 1)))
}

/// Opens a table of contents the way `fopen` would fail: as an I/O error.
fn read_toc(tocfname: &str) -> Result<Vec<u8>> {
    Ok(std::fs::read(tocfname)?)
}

/// A file read the way `osd_file::read` does: as much as is there at
/// `offset`, the rest of `buf` left as it was.
fn read_at(file: &mut File, offset: u64, buf: &mut [u8]) -> usize {
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return 0;
    }
    let mut done = 0;
    while done < buf.len() {
        match file.read(&mut buf[done..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => done += n,
        }
    }
    done
}

/// `cdrom_file::parse_wav_sample`: the length and offset of the sample data
/// of a 16-bit stereo 44.1 kHz PCM WAV, a length of 0 when it is not one.
/// Chunks are walked within the size the RIFF header declares; where the C
/// would loop forever on a truncated file this gives up instead.
fn parse_wav_sample(fname: &str) -> (u32, u32) {
    let Ok(mut file) = File::open(fname) else {
        return (0, 0);
    };
    let mut buf = [0u8; 32];
    let mut word = [0u8; 4];
    let mut half = [0u8; 2];
    let mut offset: u64 = 0;

    offset += read_at(&mut file, offset, &mut buf[..4]) as u64;
    if offset < 4 || &buf[..4] != b"RIFF" {
        return (0, 0);
    }
    offset += read_at(&mut file, offset, &mut word) as u64;
    if offset < 8 {
        return (0, 0);
    }
    let filesize = u64::from(u32::from_le_bytes(word));
    offset += read_at(&mut file, offset, &mut buf[..4]) as u64;
    if offset < 12 || &buf[..4] != b"WAVE" {
        return (0, 0);
    }

    // walks the chunks for a tag, returning its length with `offset` on its body
    let mut length = 0u32;
    let find =
        |file: &mut File, offset: &mut u64, buf: &mut [u8; 32], length: &mut u32, tag: &[u8]| {
            loop {
                let a = read_at(file, *offset, &mut buf[..4]);
                *offset += a as u64;
                let mut word = length.to_le_bytes();
                let b = read_at(file, *offset, &mut word);
                *offset += b as u64;
                *length = u32::from_le_bytes(word);
                if &buf[..4] == tag {
                    return true;
                }
                if a == 0 && b == 0 && *length == 0 {
                    return false;
                }
                *offset += u64::from(*length);
                if *offset >= filesize {
                    return false;
                }
            }
        };

    if !find(&mut file, &mut offset, &mut buf, &mut length, b"fmt ") {
        return (0, 0);
    }
    let read_u16 = |file: &mut File, offset: &mut u64, half: &mut [u8; 2]| {
        *offset += read_at(file, *offset, half) as u64;
        u16::from_le_bytes(*half)
    };
    if read_u16(&mut file, &mut offset, &mut half) != 1 {
        return (0, 0);
    }
    if read_u16(&mut file, &mut offset, &mut half) != 2 {
        return (0, 0);
    }
    offset += read_at(&mut file, offset, &mut word) as u64;
    if u32::from_le_bytes(word) != 44100 {
        return (0, 0);
    }
    // bytes per second and block alignment are ignored
    offset += read_at(&mut file, offset, &mut buf[..6]) as u64;
    if read_u16(&mut file, &mut offset, &mut half) != 16 {
        return (0, 0);
    }
    // past any extra format data, the subtraction wrapping as in the C
    offset += u64::from(length.wrapping_sub(16));

    if !find(&mut file, &mut offset, &mut buf, &mut length, b"data") || length == 0 {
        return (0, 0);
    }
    (length, offset as u32)
}

/// `cdrom_file::parse_toc`: dispatches on the extension, lowercased.
pub(crate) fn parse_toc(tocfname: &Path) -> Result<(Toc, Vec<TrackInput>)> {
    let name = tocfname.to_string_lossy();
    let ext = name
        .rfind('.')
        .map(|pos| name[pos + 1..].to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "gdi" => parse_gdi(&name),
        "cue" => parse_cue(&name),
        "iso" | "cdr" | "toast" => parse_iso(&name),
        "nrg" => Err(Error::Unsupported("Nero images".to_owned())),
        _ => Err(Error::Unsupported(format!(
            "{name} is not a CUE, GDI or ISO"
        ))),
    }
}

/// The parsers' tables cut down to the tracks found.
fn finish(mut toc: Toc, mut info: Vec<TrackInput>) -> (Toc, Vec<TrackInput>) {
    toc.tracks.truncate(toc.numtrks);
    info.truncate(toc.numtrks);
    (toc, info)
}

fn empty_tables() -> (Toc, Vec<TrackInput>) {
    (
        Toc {
            tracks: vec![zeroed_track(); TRACK_SLOTS],
            ..Toc::default()
        },
        vec![TrackInput::default(); TRACK_SLOTS],
    )
}

/// `cdrom_file::parse_iso`: a single data track, its mode guessed from the
/// file size.
fn parse_iso(tocfname: &str) -> Result<(Toc, Vec<TrackInput>)> {
    let size = std::fs::metadata(tocfname)?.len();
    let (mut toc, mut info) = empty_tables();
    toc.numtrks = 1;
    toc.numsessions = 1;
    info[0].fname = tocfname.to_owned();
    info[0].idx[0] = 0;
    info[0].idx[1] = 0;
    let (trktype, datasize) = if size.is_multiple_of(2048) {
        (TRACK_MODE1, 2048)
    } else if size.is_multiple_of(2336) {
        (TRACK_MODE2, 2336)
    } else if size.is_multiple_of(2352) {
        (TRACK_MODE2_RAW, 2352)
    } else {
        return Err(Error::Unsupported("unrecognized track type".to_owned()));
    };
    let track = &mut toc.tracks[0];
    track.trktype = trktype;
    track.datasize = datasize;
    track.frames = (size / u64::from(datasize)) as i32;
    track.subtype = SUB_NONE;
    track.pgsub = SUB_NONE;
    Ok(finish(toc, info))
}

/// `cdrom_file::parse_gdi`: a Sega GD-ROM rip, a track count then one line
/// of six fields per track.
fn parse_gdi(tocfname: &str) -> Result<(Toc, Vec<TrackInput>)> {
    let data = read_toc(tocfname)?;
    let path = get_file_path(tocfname);
    let (mut toc, mut info) = empty_tables();
    toc.flags = FLAG_GDROM;
    let lines = c_lines(&data);
    let Some(first) = lines.first() else {
        return Err(Error::Corrupt(
            "GDI doesn't have track count (blank file?)".to_owned(),
        ));
    };
    let numtracks = atoi(&tokenize(first, 0).0);
    if numtracks > 0 && numtracks - 1 > MAX_TRACKS as i32 {
        return Err(Error::Corrupt(format!(
            "GDI expects too many tracks. Expected {numtracks} tracks but only up to {} tracks allowed",
            MAX_TRACKS + 1
        )));
    }
    if numtracks == 0 {
        return Err(Error::Corrupt("GDI header specifies no tracks".to_owned()));
    }
    let mut trackcnt = 0;
    for line in &lines[1..] {
        let mut paramcnt = 0;
        let (token, mut i) = tokenize(line, 0);
        // empty lines do not count toward the tracks
        if token.is_empty() {
            continue;
        }
        paramcnt += 1;
        let trknum = atoi(&token) - 1;
        if trknum < 0 || trknum > MAX_TRACKS as i32 || trknum + 1 > numtracks {
            return Err(Error::Corrupt(format!(
                "Track {} is out of expected range of 1 to {numtracks}",
                trknum + 1
            )));
        }
        let t = trknum as usize;
        if toc.tracks[t].datasize == 0 {
            trackcnt += 1;
        }
        info[t].swap = false;
        info[t].offset = 0;
        toc.tracks[t].datasize = 0;
        toc.tracks[t].subtype = SUB_NONE;
        toc.tracks[t].subsize = 0;
        toc.tracks[t].pgsub = SUB_NONE;

        let next = |i: &mut usize, paramcnt: &mut i32| {
            let (token, j) = tokenize(line, *i);
            *i = j;
            if !token.is_empty() {
                *paramcnt += 1;
            }
            token
        };
        toc.tracks[t].physframeofs = atoi(&next(&mut i, &mut paramcnt));
        let trktype = atoi(&next(&mut i, &mut paramcnt));
        let trksize = atoi(&next(&mut i, &mut paramcnt));
        match (trktype, trksize) {
            (4, 2352) => {
                toc.tracks[t].trktype = TRACK_MODE1_RAW;
                toc.tracks[t].datasize = 2352;
            }
            (4, 2048) => {
                toc.tracks[t].trktype = TRACK_MODE1;
                toc.tracks[t].datasize = 2048;
            }
            (0, _) => {
                toc.tracks[t].trktype = TRACK_AUDIO;
                toc.tracks[t].datasize = 2352;
                info[t].swap = true;
            }
            _ => {
                return Err(Error::Corrupt(format!(
                    "Unknown track type {trktype} and track size {trksize} combination encountered"
                )));
            }
        }
        let pi = skip_spaces(line, i);
        if at(line, pi) == b'"' && !line[pi + 1..].contains(&b'"') {
            return Err(Error::Corrupt(format!(
                "Track {} filename does not having closing quotation mark: '{}'",
                trknum + 1,
                String::from_utf8_lossy(&line[pi..])
            )));
        }
        let token = next(&mut i, &mut paramcnt);
        info[t].fname = format!("{path}{}", token_str(&token));
        let size = get_file_size(&info[t].fname);
        // a zero size is still divided by, which the C does not survive
        if trksize == 0 {
            return Err(Error::Corrupt(format!(
                "track {} has no sector size",
                trknum + 1
            )));
        }
        toc.tracks[t].frames = (size / trksize as i64 as u64) as i32;
        toc.tracks[t].padframes = 0;
        if t != 0 {
            // the gap up to this track pads the one before it; later MAME
            // makes a virtual pregap of it outside the high-density area
            let previous = toc.tracks[t - 1];
            let dif = toc.tracks[t]
                .physframeofs
                .wrapping_sub(previous.frames.wrapping_add(previous.physframeofs));
            toc.tracks[t - 1].frames = previous.frames.wrapping_add(dif);
            toc.tracks[t - 1].padframes = dif;
        }
        // the offset field, unused, then anything that should not be there
        let mut token = next(&mut i, &mut paramcnt);
        while !token.is_empty() {
            token = next(&mut i, &mut paramcnt);
        }
        if paramcnt != 6 {
            return Err(Error::Corrupt(format!(
                "GDI track entry should have 6 parameters, found {paramcnt}"
            )));
        }
    }
    let mut missing = trackcnt != numtracks;
    for track in &toc.tracks[..numtracks as usize] {
        if track.datasize == 0 {
            missing = true;
        }
    }
    if missing {
        return Err(Error::Corrupt("GDI is missing tracks".to_owned()));
    }
    toc.numtrks = numtracks as usize;
    toc.numsessions = 1;
    Ok(finish(toc, info))
}

/// `cdrom_file::is_gdicue`: whether a CUE is a Redump multi-CUE Dreamcast
/// dump, marking both of its density areas.
fn is_gdicue(data: &[u8]) -> bool {
    let mut single = false;
    let mut high = false;
    for line in c_lines(data) {
        let (token, i) = tokenize(line, 0);
        if token == b"REM" {
            let rest = &line[skip_spaces(line, i).min(line.len())..];
            if rest.starts_with(b"SINGLE-DENSITY AREA") {
                single = true;
            } else if rest.starts_with(b"HIGH-DENSITY AREA") {
                high = true;
            }
        }
    }
    single && high
}

/// `cdrom_file::parse_cue`: a CUE sheet, with the IsoBuster, Redump and
/// DiscImageCreator extensions for multisession discs and the Redump
/// multi-CUE layout of GD-ROMs.
fn parse_cue(tocfname: &str) -> Result<(Toc, Vec<TrackInput>)> {
    let data = read_toc(tocfname)?;
    let path = get_file_path(tocfname);
    let is_gdrom = is_gdicue(&data);
    let (mut toc, mut info) = empty_tables();
    let mut current_area = SINGLE_DENSITY;
    let mut is_multibin = false;
    let mut leadin: i32 = -1;
    let mut lastfname = String::new();
    let mut trknum: i64 = -1;
    let mut wavlen: u32 = 0;
    let mut wavoffs: u32 = 0;
    let mut sessionnum: i64 = 0;
    let mut session_pregap: i32 = 0;
    if is_gdrom {
        toc.flags = FLAG_GDROM;
    }

    for line in c_lines(&data) {
        let (token, mut i) = tokenize(line, 0);
        let next = |i: &mut usize| {
            let (token, j) = tokenize(line, *i);
            *i = j;
            token
        };
        match token.as_slice() {
            b"REM" => {
                i = skip_spaces(line, i);
                let rest = &line[i.min(line.len())..];
                if rest.starts_with(b"SESSION") {
                    // the IsoBuster extension
                    next(&mut i);
                    sessionnum = strtoul(&next(&mut i)).wrapping_sub(1) as i32 as i64;
                    if sessionnum >= 1 {
                        toc.flags |= FLAG_MULTISESSION;
                    }
                } else if toc.flags & FLAG_MULTISESSION != 0 && rest.starts_with(b"PREGAP") {
                    // a pregap tied to the session rather than the track
                    next(&mut i);
                    session_pregap = msf_to_frames(&next(&mut i));
                } else if rest.starts_with(b"LEAD-OUT") {
                    next(&mut i);
                    let leadout = msf_to_frames(&next(&mut i));
                    info[slot(trknum)?].leadout = leadout;
                } else if rest.starts_with(b"LEAD-IN") {
                    next(&mut i);
                    leadin = msf_to_frames(&next(&mut i));
                } else if is_gdrom && rest.starts_with(b"SINGLE-DENSITY AREA") {
                    current_area = SINGLE_DENSITY;
                } else if is_gdrom && rest.starts_with(b"HIGH-DENSITY AREA") {
                    current_area = HIGH_DENSITY;
                }
            }
            b"FILE" => {
                let token = next(&mut i);
                let prevfname = std::mem::take(&mut lastfname);
                lastfname = format!("{path}{}", token_str(&token));
                if !is_multibin {
                    is_multibin = !prevfname.is_empty() && lastfname != prevfname;
                }
                let filetype = next(&mut i);
                match filetype.as_slice() {
                    b"BINARY" => info[slot(trknum + 1)?].swap = false,
                    b"MOTOROLA" => info[slot(trknum + 1)?].swap = true,
                    b"WAVE" => {
                        (wavlen, wavoffs) = parse_wav_sample(&lastfname);
                        if wavlen == 0 {
                            return Err(Error::Corrupt(format!(
                                "couldn't read [{lastfname}] or not a valid .WAV"
                            )));
                        }
                    }
                    _ => {
                        return Err(Error::Unsupported(format!(
                            "Unhandled track type {}",
                            token_str(&filetype)
                        )));
                    }
                }
            }
            b"TRACK" => {
                trknum = strtoul(&next(&mut i)).wrapping_sub(1) as i32 as i64;
                let t = slot(trknum)?;
                let typename = next(&mut i);
                let track = &mut toc.tracks[t];
                track.session = sessionnum as u32;
                track.subtype = SUB_NONE;
                track.subsize = 0;
                track.pgsub = SUB_NONE;
                track.pregap = 0;
                track.padframes = 0;
                track.datasize = 0;
                track.multicuearea = if is_gdrom { current_area } else { 0 };
                let input = &mut info[t];
                input.offset = 0;
                input.idx = [-1; MAX_INDEX + 1];
                input.leadout = -1;
                input.leadin = leadin;
                leadin = -1;
                if session_pregap != 0 {
                    // the session's pregap is folded into the lead-in
                    input.leadin = if input.leadin == -1 {
                        session_pregap
                    } else {
                        input.leadin.wrapping_add(session_pregap)
                    };
                    session_pregap = 0;
                }
                if wavlen != 0 {
                    track.frames = (wavlen / 2352) as i32;
                    input.offset = wavoffs;
                    wavoffs = 0;
                    wavlen = 0;
                }
                input.fname = lastfname.clone();
                if let Some((trktype, datasize)) = info_from_type_string(&typename) {
                    track.trktype = trktype;
                    track.datasize = datasize;
                }
                if track.datasize == 0 {
                    return Err(Error::Unsupported(format!(
                        "Unknown track type [{}].  Contact MAMEDEV.",
                        token_str(&typename)
                    )));
                }
                // the optional subcode type
                match next(&mut i).as_slice() {
                    b"RW" => {
                        track.subtype = SUB_NORMAL;
                        track.subsize = 96;
                    }
                    b"RW_RAW" => {
                        track.subtype = SUB_RAW;
                        track.subsize = 96;
                    }
                    _ => {}
                }
            }
            b"INDEX" => {
                let idx = strtoul(&next(&mut i)) as i32;
                let frames = msf_to_frames(&next(&mut i));
                if !(0..=MAX_INDEX as i32).contains(&idx) {
                    return Err(Error::Corrupt(format!("encountered invalid index {idx}")));
                }
                let t = slot(trknum)?;
                info[t].idx[idx as usize] = frames;
                if idx == 1 {
                    let track = &mut toc.tracks[t];
                    if track.pregap == 0 && info[t].idx[0] != -1 {
                        track.pregap = frames.wrapping_sub(info[t].idx[0]);
                        track.pgtype = track.trktype;
                        track.pgdatasize = track.datasize as i32;
                    } else if info[t].idx[0] == -1 {
                        // the pregap is not in the file, but index 0 is what
                        // the track lengths are computed from
                        info[t].idx[0] = frames;
                    }
                }
            }
            b"PREGAP" => {
                let frames = msf_to_frames(&next(&mut i));
                toc.tracks[slot(trknum)?].pregap = frames;
            }
            b"POSTGAP" => {
                let frames = msf_to_frames(&next(&mut i));
                toc.tracks[slot(trknum)?].postgap = frames;
            }
            b"FLAGS" => {
                let t = slot(trknum)?;
                toc.tracks[t].control_flags = 0;
                loop {
                    let last = i;
                    let flag = next(&mut i);
                    if i == last {
                        break;
                    }
                    match flag.as_slice() {
                        b"DCP" => toc.tracks[t].control_flags |= CONTROL_DIGITAL_COPY_PERMITTED,
                        b"4CH" => toc.tracks[t].control_flags |= CONTROL_4CH,
                        b"PRE" => toc.tracks[t].control_flags |= CONTROL_PREEMPHASIS,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    if trknum < 0 {
        return Err(Error::Corrupt("CUE declares no tracks".to_owned()));
    }
    let numtrks = (trknum + 1) as usize;
    toc.numtrks = numtrks;
    toc.numsessions = (sessionnum + 1) as u32;

    // the lengths, now that every file is known
    for t in 0..numtrks {
        if info[t].idx[1] == -1 {
            return Err(Error::Corrupt(format!(
                "track {} is missing INDEX 01 marker",
                t + 1
            )));
        }
        // true of cue/bin and cue/iso, and of cue/wav since WAV is little-endian
        if toc.tracks[t].trktype == TRACK_AUDIO {
            info[t].swap = true;
        }
        // WAV tracks already have their length and offset
        if info[t].offset != 0 {
            continue;
        }
        let bytes = |track: &TrackInfo| track.datasize.wrapping_add(track.subsize);
        if t + 1 >= numtrks && t > 0 && info[t].fname == info[t - 1].fname {
            // the last track, sharing the previous track's file
            let tlen = get_file_size(&info[t].fname);
            if tlen == 0 {
                return Err(missing_bin(&info[t].fname));
            }
            let previous = toc.tracks[t - 1];
            info[t].offset = info[t - 1]
                .offset
                .wrapping_add((previous.frames as u32).wrapping_mul(bytes(&previous)));
            toc.tracks[t].frames = (tlen.wrapping_sub(u64::from(info[t].offset))
                / u64::from(bytes(&toc.tracks[t]))) as i32;
        } else if t + 1 < numtrks && info[t].fname == info[t + 1].fname {
            // a track sharing the next track's file
            toc.tracks[t].frames = info[t + 1].idx[0].wrapping_sub(info[t].idx[0]);
            if toc.tracks[t].frames == 0 {
                return Err(Error::Corrupt(format!(
                    "unable to determine size of track {}, missing INDEX 01 markers?",
                    t + 1
                )));
            }
            if t > 0 {
                let previous = toc.tracks[t - 1];
                info[t].offset = info[t - 1]
                    .offset
                    .wrapping_add((previous.frames as u32).wrapping_mul(bytes(&previous)));
            }
        } else if toc.tracks[t].frames == 0 {
            // a file of its own
            let tlen = get_file_size(&info[t].fname);
            if tlen == 0 {
                return Err(missing_bin(&info[t].fname));
            }
            toc.tracks[t].frames = (tlen / u64::from(bytes(&toc.tracks[t]))) as i32;
            info[t].offset = 0;
        }

        if toc.flags & FLAG_MULTISESSION != 0 {
            if is_multibin {
                if info[t].leadout == -1
                    && t + 1 < numtrks
                    && toc.tracks[t].session != toc.tracks[t + 1].session
                {
                    // a standard lead-out before the session changes, the
                    // first one (1m30s) longer than the rest (30s)
                    info[t].leadout = if toc.tracks[t].session == 0 {
                        6750
                    } else {
                        2250
                    };
                }
                if info[t].leadin == -1
                    && t > 0
                    && toc.tracks[t].session != toc.tracks[t - 1].session
                {
                    // a standard lead-in (1m) opening a new session
                    info[t].leadin = 4500;
                }
            } else {
                if info[t].leadout != -1 {
                    // the lead-out time trims the track, rather than the
                    // next track's index 0
                    let endframes = info[t].leadout.wrapping_sub(info[t].idx[0]);
                    if toc.tracks[t].frames as u32 >= endframes as u32 {
                        toc.tracks[t].frames = endframes;
                        if t + 1 < numtrks {
                            // what remains is the gap up to the next pregap
                            info[t].leadout = info[t + 1].idx[0].wrapping_sub(info[t].leadout);
                        }
                    }
                }
                if t > 0 && info[t - 1].leadout != -1 {
                    // ImgBurn pads a BIN between the lead-out and the next
                    // track, DiscImageCreator's IMG does not
                    if !ends_with_ignore_case(&info[t - 1].fname, ".img") {
                        let leadout = info[t - 1].leadout;
                        toc.tracks[t - 1].padframes =
                            toc.tracks[t - 1].padframes.wrapping_add(leadout);
                        toc.tracks[t].frames = toc.tracks[t].frames.wrapping_sub(leadout);
                        info[t].offset = info[t]
                            .offset
                            .wrapping_add((leadout as u32).wrapping_mul(bytes(&toc.tracks[t])));
                    }
                }
            }
        }
    }

    if is_gdrom {
        #[allow(clippy::needless_range_loop)] // the C's indices, kept
        // the Redump pregaps stripped into the previous track, read from
        // the next file, to match the TOSEC layout
        for t in 1..numtrks {
            let pregap = toc.tracks[t].pregap as u32;
            let bytes = toc.tracks[t].datasize.wrapping_add(toc.tracks[t].subsize);
            let previous = &mut toc.tracks[t - 1];
            previous.frames = (previous.frames as u32).wrapping_add(pregap) as i32;
            previous.splitframes = (previous.splitframes as u32).wrapping_add(pregap) as i32;
            info[t].offset = info[t].offset.wrapping_add(pregap.wrapping_mul(bytes));
            info[t].idx[1] = (info[t].idx[1] as u32).wrapping_sub(pregap) as i32;
            let track = &mut toc.tracks[t];
            track.frames = (track.frames as u32).wrapping_sub(pregap) as i32;
            track.pregap = 0;
            track.pgtype = 0;
        }
        adjust_high_density_area(&mut toc);
    }
    Ok(finish(toc, info))
}

fn missing_bin(fname: &str) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("couldn't find bin file [{fname}]"),
    ))
}

/// `core_filename_ends_with`, which ignores case.
fn ends_with_ignore_case(name: &str, ext: &str) -> bool {
    name.len() >= ext.len()
        && name.as_bytes()[name.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
}

/// Lays out a GD-ROM's tracks back to back, the high-density area starting
/// at frame 45000 and the single-density track before it padded up to
/// there: the end of 0.289's `parse_cue`, later MAME's
/// `adjust_high_density_area`.
fn adjust_high_density_area(toc: &mut Toc) {
    for t in 1..toc.numtrks {
        let previous = toc.tracks[t - 1];
        if toc.tracks[t].multicuearea == HIGH_DENSITY && previous.multicuearea == SINGLE_DENSITY {
            toc.tracks[t].physframeofs = GDI_HIGH_DENSITY_AREA;
            let dif = GDI_HIGH_DENSITY_AREA
                .wrapping_sub(previous.frames.wrapping_add(previous.physframeofs));
            toc.tracks[t - 1].frames = previous.frames.wrapping_add(dif);
            toc.tracks[t - 1].padframes = dif;
        } else {
            toc.tracks[t].physframeofs = previous.physframeofs.wrapping_add(previous.frames);
        }
    }
}

// ---------------------------------------------------------------------------
// Reading a table of contents back from a CHD: 0.289's
// `cdrom_file::parse_metadata`, and the frame offsets the `cdrom_file`
// constructor lays out from it.
// ---------------------------------------------------------------------------

/// A conversion `sscanf` made.
enum Scanned {
    Int(i32),
    Str(Vec<u8>),
}

/// `sscanf` for the `%d` and `%s` conversions the metadata formats use: a
/// space in the format matches any run of spaces, other characters match
/// themselves, and scanning stops at the first conversion that fails.
fn scanf(text: &[u8], format: &[u8]) -> Vec<Scanned> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut f = 0;
    while f < format.len() {
        match format[f] {
            byte if is_space(byte) => i = skip_spaces(text, i),
            b'%' => {
                f += 1;
                match format.get(f) {
                    Some(b'd') => {
                        let (negative, value, digits, next) = scan_digits(text, i);
                        if digits == 0 {
                            break;
                        }
                        let value = value as i32;
                        out.push(Scanned::Int(if negative {
                            value.wrapping_neg()
                        } else {
                            value
                        }));
                        i = next;
                    }
                    Some(b's') => {
                        i = skip_spaces(text, i);
                        let start = i;
                        while at(text, i) != 0 && !is_space(at(text, i)) {
                            i += 1;
                        }
                        if i == start {
                            break;
                        }
                        out.push(Scanned::Str(text[start..i].to_vec()));
                    }
                    _ => break,
                }
            }
            byte => {
                if at(text, i) != byte {
                    break;
                }
                i += 1;
            }
        }
        f += 1;
    }
    out
}

/// The fields of a track entry, by the format of its tag.
struct TrackFields {
    tracknum: i32,
    trktype: Vec<u8>,
    subtype: Vec<u8>,
    frames: i32,
    padframes: i32,
    pregap: i32,
    pgtype: Vec<u8>,
    pgsub: Vec<u8>,
    postgap: i32,
}

/// Scans a track entry: `None` unless every conversion of `format`, in the
/// order of `layout`, succeeded.
fn scan_track(data: &[u8], format: &[u8], layout: &[&str]) -> Option<TrackFields> {
    let text = &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())];
    let scanned = scanf(text, format);
    if scanned.len() != layout.len() {
        return None;
    }
    let mut fields = TrackFields {
        tracknum: -1,
        trktype: Vec::new(),
        subtype: Vec::new(),
        frames: 0,
        padframes: 0,
        pregap: 0,
        pgtype: Vec::new(),
        pgsub: Vec::new(),
        postgap: 0,
    };
    for (value, name) in scanned.into_iter().zip(layout) {
        match (value, *name) {
            (Scanned::Int(v), "tracknum") => fields.tracknum = v,
            (Scanned::Int(v), "frames") => fields.frames = v,
            (Scanned::Int(v), "padframes") => fields.padframes = v,
            (Scanned::Int(v), "pregap") => fields.pregap = v,
            (Scanned::Int(v), "postgap") => fields.postgap = v,
            (Scanned::Str(v), "type") => fields.trktype = v,
            (Scanned::Str(v), "subtype") => fields.subtype = v,
            (Scanned::Str(v), "pgtype") => fields.pgtype = v,
            (Scanned::Str(v), "pgsub") => fields.pgsub = v,
            _ => return None,
        }
    }
    Some(fields)
}

/// The subcode type and size a metadata subtype names, if any.
fn info_from_subtype_string(name: &[u8]) -> Option<(u32, u32)> {
    match name {
        b"RW" => Some((SUB_NORMAL, 96)),
        b"RW_RAW" => Some((SUB_RAW, 96)),
        _ => None,
    }
}

/// The table of contents a CD CHD's metadata describes, laid out the way
/// the `cdrom_file` constructor lays it out: each track's physical and CHD
/// frame offsets, and past the last track a dummy one marking the end.
pub(crate) fn toc_from_chd(chd: &crate::Chd) -> Result<Toc> {
    use crate::container::{MTAG_CDROM_OLD, MTAG_GDROM_OLD, MTAG_TRACK};

    let invalid = || Error::Corrupt("invalid CD metadata".to_owned());
    let find = |tag: u32, index: usize| {
        chd.metadata()
            .iter()
            .find(|entry| entry.tag == tag && entry.index as usize == index)
    };
    let mut toc = Toc {
        numsessions: 1,
        tracks: vec![zeroed_track(); TRACK_SLOTS],
        ..Toc::default()
    };

    while toc.numtrks < MAX_TRACKS {
        let n = toc.numtrks;
        let fields = if let Some(entry) = find(MTAG_TRACK, n) {
            scan_track(
                &entry.data,
                b"TRACK:%d TYPE:%s SUBTYPE:%s FRAMES:%d",
                &["tracknum", "type", "subtype", "frames"],
            )
            .ok_or_else(invalid)?
        } else if let Some(entry) = find(MTAG_TRACK2, n) {
            scan_track(
                &entry.data,
                b"TRACK:%d TYPE:%s SUBTYPE:%s FRAMES:%d PREGAP:%d PGTYPE:%s PGSUB:%s POSTGAP:%d",
                &[
                    "tracknum", "type", "subtype", "frames", "pregap", "pgtype", "pgsub", "postgap",
                ],
            )
            .ok_or_else(invalid)?
        } else {
            // GD-ROMs, the legacy ones storing their audio little-endian
            let entry = match find(MTAG_GDROM_OLD, n) {
                Some(entry) => {
                    toc.flags |= FLAG_GDROMLE;
                    entry
                }
                None => match find(MTAG_GDROM_TRACK, n) {
                    Some(entry) => entry,
                    None => break,
                },
            };
            let fields = scan_track(
                &entry.data,
                b"TRACK:%d TYPE:%s SUBTYPE:%s FRAMES:%d PAD:%d PREGAP:%d PGTYPE:%s PGSUB:%s POSTGAP:%d",
                &[
                    "tracknum", "type", "subtype", "frames", "padframes", "pregap", "pgtype",
                    "pgsub", "postgap",
                ],
            )
            .ok_or_else(invalid)?;
            toc.flags |= FLAG_GDROM;
            fields
        };
        if fields.tracknum <= 0 || fields.tracknum > MAX_TRACKS as i32 {
            return Err(invalid());
        }
        let track = &mut toc.tracks[fields.tracknum as usize - 1];
        let (trktype, datasize) = info_from_type_string(&fields.trktype).ok_or_else(invalid)?;
        track.trktype = trktype;
        track.datasize = datasize;
        (track.subtype, track.subsize) =
            info_from_subtype_string(&fields.subtype).unwrap_or((SUB_NONE, 0));
        track.frames = fields.frames;
        track.padframes = fields.padframes;
        let frames = fields.frames as u32;
        let padded = frames.wrapping_add(TRACK_PADDING - 1) / TRACK_PADDING;
        track.extraframes = padded.wrapping_mul(TRACK_PADDING).wrapping_sub(frames) as i32;
        track.pregap = fields.pregap;
        track.pgtype = TRACK_MODE1;
        track.pgsub = SUB_NONE;
        track.pgdatasize = 0;
        track.pgsubsize = 0;
        if track.pregap > 0 {
            if let Some(name) = fields.pgtype.strip_prefix(b"V")
                && let Some((pgtype, pgdatasize)) = info_from_type_string(name)
            {
                track.pgtype = pgtype;
                track.pgdatasize = pgdatasize as i32;
            }
            if let Some((pgsub, pgsubsize)) = info_from_subtype_string(&fields.pgsub) {
                track.pgsub = pgsub;
                track.pgsubsize = pgsubsize as i32;
            }
        }
        track.postgap = fields.postgap;
        toc.numtrks += 1;
    }

    if toc.numtrks == 0 {
        // the version 3 binary form: a track count then six words a track,
        // in whichever byte order makes the count sensible
        let entry = find(MTAG_CDROM_OLD, 0).ok_or_else(invalid)?;
        let words: Vec<u32> = entry
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_ne_bytes(*word))
            .collect();
        if words.len() < 1 + MAX_TRACKS * 6 {
            return Err(invalid());
        }
        let swap = words[0] as usize > MAX_TRACKS;
        let word = |i: usize| {
            if swap {
                words[i].swap_bytes()
            } else {
                words[i]
            }
        };
        toc.numtrks = word(0) as usize;
        if toc.numtrks > MAX_TRACKS {
            return Err(invalid());
        }
        for (t, track) in toc.tracks.iter_mut().take(MAX_TRACKS).enumerate() {
            let base = 1 + t * 6;
            *track = TrackInfo {
                trktype: word(base),
                subtype: word(base + 1),
                datasize: word(base + 2),
                subsize: word(base + 3),
                frames: word(base + 4) as i32,
                extraframes: word(base + 5) as i32,
                ..zeroed_track()
            };
        }
    }

    // each track's frame offsets, the CHD ones counting the padding to four
    let mut physofs = 0i32;
    let mut chdofs = 0i32;
    for track in &mut toc.tracks[..toc.numtrks] {
        track.physframeofs = physofs;
        track.chdframeofs = chdofs;
        physofs = physofs.wrapping_add(track.frames);
        chdofs = chdofs
            .wrapping_add(track.frames)
            .wrapping_add(track.extraframes);
    }
    let end = &mut toc.tracks[toc.numtrks];
    end.physframeofs = physofs;
    end.chdframeofs = chdofs;
    toc.tracks.truncate(toc.numtrks + 1);
    Ok(toc)
}

/// The size of a CD frame in a CHD: 2352 bytes of sector data, 96 of
/// subcode. chdman's CD hunks hold eight.
pub const FRAME_SIZE: u32 = 2448;

/// The total size of the files a CUE, GDI or ISO names, each counted once:
/// what [`crate::create_cd`] reports progress against.
pub fn input_size(toc: &Path) -> Result<u64> {
    let (_, info) = parse_toc(toc)?;
    let mut seen: Vec<&str> = Vec::new();
    let mut size = 0;
    for input in &info {
        if !seen.contains(&input.fname.as_str()) {
            seen.push(&input.fname);
            size += get_file_size(&input.fname);
        }
    }
    Ok(size)
}

/// The frames of a CD, laid out the way `chd_cd_compressor::read_data`
/// lays them out: each track padded with zero frames to a multiple of
/// four, each frame its sector data then subcode, zero-padded to 2448
/// bytes, audio byte-swapped to big-endian. Progress is the input bytes in
/// proportion to the frames read, so it adds up to [`input_size`].
pub(crate) struct CdSource<'a> {
    toc: &'a Toc,
    info: &'a [TrackInput],
    lastfile: Option<String>,
    file: Option<BufReader<File>>,
    pos: u64,
    input_size: u64,
    logical_size: u64,
    reported: u64,
}

impl<'a> CdSource<'a> {
    pub(crate) fn new(
        toc: &'a Toc,
        info: &'a [TrackInput],
        input_size: u64,
        logical_size: u64,
    ) -> Self {
        Self {
            toc,
            info,
            lastfile: None,
            file: None,
            pos: 0,
            input_size,
            logical_size,
            reported: 0,
        }
    }

    fn open(&mut self, fname: &str) -> Result<()> {
        self.file = None;
        self.lastfile = Some(fname.to_owned());
        let file = File::open(fname).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("error opening input file ({fname}): {error}"),
            ))
        })?;
        self.file = Some(BufReader::with_capacity(1 << 20, file));
        self.pos = 0;
        Ok(())
    }

    fn is_open(&self, fname: &str) -> bool {
        self.file.is_some() && self.lastfile.as_deref() == Some(fname)
    }

    fn read_frame(&mut self, pos: u64, dest: &mut [u8]) -> Result<()> {
        let name = self.lastfile.clone().unwrap_or_default();
        let Some(file) = self.file.as_mut() else {
            return Err(Error::Corrupt(format!("error reading input file ({name})")));
        };
        if pos != self.pos {
            file.seek(SeekFrom::Start(pos))?;
        }
        file.read_exact(dest).map_err(|_| {
            self.pos = u64::MAX;
            Error::Corrupt(format!("error reading input file ({name})"))
        })?;
        self.pos = pos + dest.len() as u64;
        Ok(())
    }
}

impl crate::writer::Source for CdSource<'_> {
    fn read(&mut self, mut offset: u64, dest: &mut [u8]) -> Result<u64> {
        let frame_size = u64::from(FRAME_SIZE);
        dest.fill(0);
        let length = dest.len() as u64;
        let end = offset + length;
        let mut remaining = length;
        let mut d = 0usize;
        let mut startoffs = 0u64;
        for t in 0..self.toc.numtrks {
            let track = self.toc.tracks[t];
            let endoffs = startoffs
                + u64::from((track.frames as u32).wrapping_add(track.extraframes as u32))
                    * frame_size;
            if offset >= startoffs && offset < endoffs {
                if !self.is_open(&self.info[t].fname) {
                    let fname = self.info[t].fname.clone();
                    self.open(&fname)?;
                }
                let bytesperframe = u64::from(track.datasize.wrapping_add(track.subsize));
                let src_track_start = u64::from(self.info[t].offset);
                let src_track_end =
                    src_track_start.wrapping_add(bytesperframe * u64::from(track.frames as u32));
                let mut split_track_start =
                    src_track_end.wrapping_sub(u64::from(track.splitframes as u32) * bytesperframe);
                let pad_track_start = split_track_start
                    .wrapping_sub(u64::from(track.padframes as u32) * bytesperframe);
                // no split unless a split-bin read is needed
                if track.splitframes == 0 {
                    split_track_start = u64::MAX;
                }
                while remaining != 0 && offset < endoffs {
                    let src_frame_start = src_track_start
                        .wrapping_add((offset - startoffs) / frame_size * bytesperframe);
                    // a split-bin read moves on to the next track's file
                    if src_frame_start >= split_track_start && src_frame_start < src_track_end {
                        let next = self.info.get(t + 1).map(|input| input.fname.clone());
                        if let Some(next) = next
                            && self.lastfile.as_deref() != Some(next.as_str())
                        {
                            self.open(&next)?;
                        }
                    }
                    if src_frame_start < src_track_end {
                        let frame = &mut dest[d..d + bytesperframe as usize];
                        if src_frame_start >= pad_track_start && src_frame_start < split_track_start
                        {
                            frame.fill(0);
                        } else {
                            let pos = if src_frame_start >= split_track_start {
                                src_frame_start - split_track_start
                            } else {
                                src_frame_start
                            };
                            self.read_frame(pos, frame)?;
                        }
                        if self.info[t].swap {
                            for pair in dest[d..d + 2352].as_chunks_mut::<2>().0 {
                                pair.swap(0, 1);
                            }
                        }
                    }
                    offset += frame_size;
                    d += frame_size as usize;
                    remaining -= frame_size;
                }
            }
            startoffs = endoffs;
        }
        // input bytes in proportion to the logical bytes covered
        let target = (u128::from(self.input_size) * u128::from(end.min(self.logical_size))
            / u128::from(self.logical_size.max(1))) as u64;
        let delta = target.saturating_sub(self.reported);
        self.reported += delta;
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_entries_of_a_single_track_cd() {
        // the table of contents chdman builds for tests' cd1.cue
        let toc = Toc {
            numtrks: 1,
            numsessions: 1,
            flags: 0,
            tracks: vec![TrackInfo {
                trktype: TRACK_MODE1,
                subtype: SUB_NONE,
                datasize: 2048,
                frames: 130,
                ..Default::default()
            }],
        };
        let entries = metadata_entries(&toc);
        assert_eq!(entries.len(), 1);
        let (tag, flags, data) = &entries[0];
        assert_eq!(*tag, u32::from_be_bytes(*b"CHT2"));
        assert_eq!(*flags, MDFLAGS_CHECKSUM);
        assert_eq!(
            data,
            b"TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:130 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0"
        );
    }

    #[test]
    fn metadata_entries_of_a_gdrom() {
        let toc = Toc {
            numtrks: 1,
            numsessions: 1,
            flags: FLAG_GDROM,
            tracks: vec![TrackInfo {
                trktype: TRACK_MODE1_RAW,
                subtype: SUB_NONE,
                datasize: 2352,
                frames: 3,
                padframes: 2,
                pregap: 0,
                pgtype: TRACK_MODE1,
                ..Default::default()
            }],
        };
        let entries = metadata_entries(&toc);
        assert_eq!(entries.len(), 1);
        let (tag, _, data) = &entries[0];
        assert_eq!(*tag, u32::from_be_bytes(*b"CHGD"));
        assert_eq!(
            data,
            b"TRACK:1 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:3 PAD:2 PREGAP:0 PGTYPE:MODE1 PGSUB:NONE POSTGAP:0\0"
        );
    }

    #[test]
    fn metadata_entries_of_a_multisession_cd() {
        // a valid pregap submode carries the 'V' prefix
        let track = |session, pgdatasize| TrackInfo {
            trktype: TRACK_AUDIO,
            subtype: SUB_RAW,
            datasize: 2352,
            subsize: 96,
            frames: 75,
            pgtype: TRACK_MODE1,
            pgsub: SUB_NORMAL,
            pgdatasize,
            session,
            ..Default::default()
        };
        let toc = Toc {
            numtrks: 2,
            numsessions: 2,
            flags: FLAG_MULTISESSION,
            tracks: vec![track(0, 0), track(1, 2352)],
        };
        // chdman 0.289 writes no session entries
        let entries = metadata_entries(&toc);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, u32::from_be_bytes(*b"CHT2"));
        assert_eq!(entries[0].2, b"TRACK:1 TYPE:AUDIO SUBTYPE:RW_RAW FRAMES:75 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0\0");
        assert_eq!(entries[1].2, b"TRACK:2 TYPE:AUDIO SUBTYPE:RW_RAW FRAMES:75 PREGAP:0 PGTYPE:VMODE1 PGSUB:RW POSTGAP:0\0");
    }
}
