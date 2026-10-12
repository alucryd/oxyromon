//! Extracting a CD CHD back to a CUE, GDI or cdrdao TOC and its BINs, the
//! way chdman 0.289's `extractcd` does: the same file names, the same
//! sheet, and the same bytes in the BINs.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cdrom::{
    self, FLAG_GDROMLE, SUB_NONE, TRACK_AUDIO, TRACK_MODE1, TRACK_MODE1_RAW, TRACK_MODE2,
    TRACK_MODE2_FORM_MIX, TRACK_MODE2_FORM1, TRACK_MODE2_FORM2, TRACK_MODE2_RAW, Toc,
    subtype_string, type_string,
};
use crate::container::{Chd, ChdType};
use crate::error::{Error, Result};
use crate::writer::part_path;

/// The size of the buffer frames are gathered in before being written:
/// chdman's is 32 MiB, which changes nothing in what is written.
const TEMP_BUFFER_SIZE: usize = 4 * 1024 * 1024;
/// Where the high-density area of a GD-ROM starts.
const HIGH_DENSITY_AREA: i32 = 45000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A cdrdao TOC, which keeps subcode.
    Normal,
    CueBin,
    Gdi,
}

/// The `%t` track number variables of a BIN name, replaced the way
/// chdman's regex walk replaces them: `(%*)(%([+-]?\d+)?([a-zA-Z]))`, an
/// even run of escaping `%`s before it making it a variable. Returns the
/// name and whether a track variable was found.
fn format_track_name(name: &str, tracknum: usize, splitbin: bool) -> (String, bool) {
    let bytes = name.as_bytes();
    let mut formatted = name.to_owned();
    let mut found = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        // the run of '%', the last of which opens the variable
        let run_start = i;
        while i < bytes.len() && bytes[i] == b'%' {
            i += 1;
        }
        // try each '%' of the run as the opening one, as the regex search
        // does, leftmost first; only the last can be followed by a letter
        let open = i - 1;
        let mut j = i;
        if j < bytes.len()
            && matches!(bytes[j], b'+' | b'-')
            && bytes.get(j + 1).is_some_and(u8::is_ascii_digit)
        {
            j += 1;
        }
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j >= bytes.len() || !bytes[j].is_ascii_alphabetic() {
            // no variable here; the regex moves on past the '%'s
            continue;
        }
        let escapes = open - run_start;
        let full = &name[open..=j];
        let format_part = &name[open + 1..j];
        if escapes % 2 == 0 && bytes[j] == b't' && splitbin {
            let replacement = printf_d(format_part, tracknum + 1);
            formatted = formatted.replace(full, &replacement);
            found = true;
        }
        i = j + 1;
    }
    (formatted, found)
}

/// `printf("%<format>d", value)` for the flags and width a BIN name can
/// carry: a sign, a zero-padding width.
fn printf_d(format: &str, value: usize) -> String {
    let (sign, digits) = match format.as_bytes().first() {
        Some(b'+') => ("+", &format[1..]),
        Some(b'-') => ("-", &format[1..]),
        _ => ("", format),
    };
    let width: usize = digits.parse().unwrap_or(0);
    let zero = digits.starts_with('0');
    let body = if sign == "+" {
        format!("+{value}")
    } else {
        value.to_string()
    };
    if sign == "-" {
        format!("{body:<width$}")
    } else if zero {
        format!("{body:0>width$}")
    } else {
        format!("{body:>width$}")
    }
}

/// `msf_string_from_frames`.
fn msf(frames: u32) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        frames / (75 * 60),
        (frames / 75) % 60,
        frames % 75
    )
}

/// `core_filename_extract_base`: the name after the last directory
/// separator.
fn basename(name: &str) -> &str {
    let separator = |c: char| c == '/' || (cfg!(windows) && (c == '\\' || c == ':'));
    match name.rfind(separator) {
        Some(pos) => &name[pos + 1..],
        None => name,
    }
}

/// `output_track_metadata`: a track's lines in the sheet.
fn track_metadata(
    mode: Mode,
    sheet: &mut String,
    tracknum: usize,
    info: &cdrom::TrackInfo,
    filename: &str,
    frameoffs: u32,
    outputoffs: u64,
) {
    let pregap = info.pregap as u32;
    match mode {
        Mode::Gdi => {
            let tracktype = if info.trktype == TRACK_AUDIO { 0 } else { 4 };
            let quote = if filename.contains(' ') { "\"" } else { "" };
            sheet.push_str(&format!(
                "{} {} {} {} {quote}{filename}{quote} {}\n",
                tracknum + 1,
                frameoffs as i32,
                tracktype,
                info.datasize,
                outputoffs as i32
            ));
        }
        Mode::CueBin => {
            if outputoffs == 0 {
                sheet.push_str(&format!("FILE \"{filename}\" BINARY\n"));
            }
            let submode = match info.trktype {
                TRACK_MODE1 | TRACK_MODE1_RAW => format!("MODE1/{:04}", info.datasize),
                TRACK_MODE2 | TRACK_MODE2_FORM1 | TRACK_MODE2_FORM2 | TRACK_MODE2_FORM_MIX
                | TRACK_MODE2_RAW => format!("MODE2/{:04}", info.datasize),
                TRACK_AUDIO => "AUDIO".to_owned(),
                _ => String::new(),
            };
            sheet.push_str(&format!("  TRACK {:02} {submode}\n", tracknum + 1));
            if info.pregap > 0 && info.pgdatasize == 0 {
                sheet.push_str(&format!("    PREGAP {}\n", msf(pregap)));
                sheet.push_str(&format!("    INDEX 01 {}\n", msf(frameoffs)));
            } else if info.pregap > 0 && info.pgdatasize > 0 {
                sheet.push_str(&format!("    INDEX 00 {}\n", msf(frameoffs)));
                sheet.push_str(&format!(
                    "    INDEX 01 {}\n",
                    msf(frameoffs.wrapping_add(pregap))
                ));
            }
            if info.pregap == 0 {
                sheet.push_str(&format!("    INDEX 01 {}\n", msf(frameoffs)));
            }
            if info.postgap > 0 {
                sheet.push_str(&format!("    POSTGAP {}\n", msf(info.postgap as u32)));
            }
        }
        Mode::Normal => {
            sheet.push_str(&format!("// Track {}\n", tracknum + 1));
            let modesubmode = if info.subtype != SUB_NONE {
                format!(
                    "{} {}",
                    type_string(info.trktype),
                    subtype_string(info.subtype)
                )
            } else {
                type_string(info.trktype).to_owned()
            };
            sheet.push_str(&format!("TRACK {modesubmode}\n"));
            sheet.push_str("NO COPY\n");
            if info.trktype == TRACK_AUDIO {
                sheet.push_str("NO PRE_EMPHASIS\n");
                sheet.push_str("TWO_CHANNEL_AUDIO\n");
            }
            if info.pregap > 0 {
                sheet.push_str(&format!("ZERO {modesubmode} {}\n", msf(pregap)));
            }
            let length =
                (info.frames as u32).wrapping_mul(info.datasize.wrapping_add(info.subsize)) as i32;
            if outputoffs == 0 {
                sheet.push_str(&format!(
                    "DATAFILE \"{filename}\" {} // length in bytes: {length}\n",
                    msf(info.frames as u32)
                ));
            } else {
                sheet.push_str(&format!(
                    "DATAFILE \"{filename}\" #{} {} // length in bytes: {length}\n",
                    outputoffs as u32 as i32,
                    msf(info.frames as u32)
                ));
            }
            if info.pregap > 0 {
                sheet.push_str(&format!("START {}\n", msf(pregap)));
            }
            sheet.push_str("\n\n");
        }
    }
}

/// `cdrom_file::physical_to_chd_lba`: the track a physical frame falls in,
/// and the frame of the CHD holding it.
fn physical_to_chd_lba(toc: &Toc, physlba: u32) -> (usize, u32) {
    for track in 0..toc.numtrks {
        if physlba < toc.tracks[track + 1].physframeofs as u32 {
            let chdlba = physlba
                .wrapping_sub(toc.tracks[track].physframeofs as u32)
                .wrapping_add(toc.tracks[track].chdframeofs as u32);
            return (track, chdlba);
        }
    }
    (0, physlba)
}

/// `cdrom_file::lba_to_msf`, in BCD.
fn lba_to_msf(lba: u32) -> u32 {
    let m = (lba / (60 * 75)) as u8 as u32;
    let lba = lba.wrapping_sub(m * 60 * 75);
    let s = (lba / 75) as u8 as u32;
    let f = (lba % 75) as u8 as u32;
    ((m / 10) << 20)
        | ((m % 10) << 16)
        | ((s / 10) << 12)
        | ((s % 10) << 8)
        | ((f / 10) << 4)
        | (f % 10)
}

/// The CD reader of `cdrom_file`, over the CHD it was opened on.
struct Reader<'a> {
    chd: &'a mut Chd,
    toc: &'a Toc,
}

impl Reader<'_> {
    /// `read_partial_sector`, physical addressing: the bytes of a CHD frame.
    fn read_partial(
        &mut self,
        dest: &mut [u8],
        chdsector: u32,
        track: usize,
        startoffs: usize,
    ) -> Result<()> {
        let offset = u64::from(chdsector) * u64::from(cdrom::FRAME_SIZE) + startoffs as u64;
        self.chd.read_bytes(offset, dest)?;
        // legacy GD-ROMs store their audio little-endian
        if self.toc.flags & FLAG_GDROMLE != 0 && self.toc.tracks[track].trktype == TRACK_AUDIO {
            let mut index = startoffs;
            while index + 1 < 2352 && index - startoffs + 1 < dest.len() {
                dest.swap(index - startoffs, index - startoffs + 1);
                index += 2;
            }
        }
        Ok(())
    }

    /// `read_data`, physical addressing, `datatype` converted to from the
    /// type of the track the frame lies in. A conversion chdman does not
    /// know leaves `dest` as it was.
    fn read_data(&mut self, lbasector: u32, dest: &mut [u8], datatype: u32) -> Result<()> {
        let (track, chdsector) = physical_to_chd_lba(self.toc, lbasector);
        let tracktype = self.toc.tracks[track].trktype;
        let datasize = self.toc.tracks[track].datasize as usize;
        if datatype == tracktype {
            return self.read_partial(&mut dest[..datasize], chdsector, track, 0);
        }
        match (datatype, tracktype) {
            (TRACK_MODE1, TRACK_MODE1_RAW) => {
                self.read_partial(&mut dest[..2048], chdsector, track, 16)
            }
            (TRACK_MODE1_RAW, TRACK_MODE1) => {
                dest[..12].copy_from_slice(&crate::codec::SYNC_HEADER);
                dest[12..15].copy_from_slice(&lba_to_msf(lbasector).to_be_bytes()[1..]);
                dest[15] = 1;
                self.read_partial(&mut dest[16..16 + 2048], chdsector, track, 0)
            }
            (TRACK_MODE1, TRACK_MODE2_FORM1 | TRACK_MODE2_RAW) => {
                self.read_partial(&mut dest[..2048], chdsector, track, 24)
            }
            (TRACK_MODE1, TRACK_MODE2_FORM_MIX) => {
                self.read_partial(&mut dest[..2048], chdsector, track, 8)
            }
            (TRACK_MODE2, TRACK_MODE1_RAW | TRACK_MODE2_RAW) => {
                self.read_partial(&mut dest[..2336], chdsector, track, 16)
            }
            _ => Ok(()),
        }
    }

    /// `read_subcode`, physical addressing: nothing when the track has none.
    fn read_subcode(&mut self, lbasector: u32, dest: &mut [u8]) -> Result<()> {
        let (track, chdsector) = physical_to_chd_lba(self.toc, lbasector);
        let info = self.toc.tracks[track];
        if info.subsize == 0 {
            return Ok(());
        }
        self.read_partial(
            &mut dest[..info.subsize as usize],
            chdsector,
            track,
            info.datasize as usize,
        )
    }
}

/// Extracts a CD CHD the way `chdman extractcd` does.
///
/// The sheet's extension picks its format: `.cue` for a CUE sheet, `.gdi`
/// for a GDI, anything else a cdrdao TOC, the only one which keeps
/// subcode. `bin` names the BIN, by default the sheet's name with `.bin`;
/// `splitbin` writes a BIN per track, as a GDI and a GD-ROM CUE always do,
/// the name then needing a `%t` for the track number (added to the default
/// one). Returns the files written, the sheet first.
///
/// Progress is reported as CHD logical bytes, adding up to its logical
/// size. Every file lands on `<file>.part` and is renamed into place once
/// all are complete; a failed run leaves none behind.
pub fn extract_cd(
    chd: &mut Chd,
    sheet: &Path,
    bin: Option<&Path>,
    splitbin: bool,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<PathBuf>> {
    if chd.info().chd_type != ChdType::Cd {
        return Err(Error::Unsupported("not a CD CHD".to_owned()));
    }
    let mut toc = cdrom::toc_from_chd(chd)?;
    let is_gdrom = toc.flags & (cdrom::FLAG_GDROM | FLAG_GDROMLE) != 0;
    let sheet_name = sheet.to_string_lossy().into_owned();
    let lower = sheet_name.to_ascii_lowercase();
    let mode = if lower.ends_with(".cue") {
        Mode::CueBin
    } else if lower.ends_with(".gdi") {
        Mode::Gdi
    } else {
        Mode::Normal
    };

    // the BIN names, from the sheet's or the one given
    let mut default_name = sheet_name.clone();
    if let Some(chop) = default_name.rfind('.') {
        default_name.truncate(chop);
    }
    // GDIs, and GD-ROM CUEs in the Redump layout, are always split by track
    let splitbin = splitbin || mode == Mode::Gdi || (is_gdrom && mode == Mode::CueBin);
    if splitbin {
        if mode == Mode::Gdi {
            default_name.push_str("%02t");
        } else {
            default_name.push_str(if toc.numtrks >= 10 {
                " (Track %02t)"
            } else {
                " (Track %t)"
            });
        }
    }
    let (bin_stem, bin_ext) = match bin {
        None => (default_name, ".bin".to_owned()),
        Some(bin) => {
            let mut name = bin.to_string_lossy().into_owned();
            match name.rfind('.') {
                Some(chop) => {
                    let ext = name[chop..].to_owned();
                    name.truncate(chop);
                    (name, ext)
                }
                None => (name, ".bin".to_owned()),
            }
        }
    };
    if bin_stem.contains('"') || bin_ext.contains('"') {
        return Err(Error::InvalidOption(format!(
            "output bin filename ({bin_stem}{bin_ext}) must not contain quotation marks"
        )));
    }
    let mut track_filenames = Vec::with_capacity(toc.numtrks);
    for t in 0..toc.numtrks {
        let ext = if mode == Mode::Gdi && toc.tracks[t].trktype == TRACK_AUDIO {
            ".raw"
        } else {
            &bin_ext
        };
        let (name, found) = format_track_name(&format!("{bin_stem}{ext}"), t, splitbin);
        if splitbin && !found {
            return Err(Error::InvalidOption(
                "a track number variable (%t) must be in the bin filename to split it by track"
                    .to_owned(),
            ));
        }
        track_filenames.push(name);
    }

    let mut outputs: Vec<PathBuf> = vec![sheet.to_path_buf()];
    for name in &track_filenames {
        let path = PathBuf::from(name);
        if !outputs.contains(&path) {
            outputs.push(path);
        }
    }
    let logical_size = chd.info().logical_size;
    let result = extract_inner(
        chd,
        &mut toc,
        mode,
        is_gdrom,
        &track_filenames,
        &outputs[0],
        logical_size,
        progress,
    );
    let result = result.and_then(|reported| {
        progress(logical_size.saturating_sub(reported));
        for output in &outputs {
            std::fs::rename(part_path(output), output)?;
        }
        Ok(())
    });
    match result {
        Ok(()) => Ok(outputs),
        Err(error) => {
            for output in &outputs {
                let _ = std::fs::remove_file(part_path(output));
            }
            Err(error)
        }
    }
}

/// Writes the sheet and the BINs to their `.part` files, returning the
/// progress reported.
#[allow(clippy::too_many_arguments)]
fn extract_inner(
    chd: &mut Chd,
    toc: &mut Toc,
    mode: Mode,
    is_gdrom: bool,
    track_filenames: &[String],
    sheet: &Path,
    logical_size: u64,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    let version = chd.info().version;
    let mut text = String::new();
    if mode == Mode::Gdi {
        text.push_str(&format!("{}\n", toc.numtrks));
    } else if mode == Mode::Normal {
        let (mut mode1, mut mode2, mut cdda) = (false, false, false);
        for track in &toc.tracks[..toc.numtrks] {
            match track.trktype {
                TRACK_MODE1 | TRACK_MODE1_RAW => mode1 = true,
                TRACK_MODE2 | TRACK_MODE2_FORM1 | TRACK_MODE2_FORM2 | TRACK_MODE2_FORM_MIX
                | TRACK_MODE2_RAW => mode2 = true,
                TRACK_AUDIO => cdda = true,
                _ => {}
            }
        }
        text.push_str(if mode2 {
            "CD_ROM_XA\n\n\n"
        } else if cdda && !mode1 {
            "CD_DA\n\n\n"
        } else {
            "CD_ROM\n\n\n"
        });
    }

    if is_gdrom && mode == Mode::CueBin {
        redump_gdrom_layout(toc);
    }

    let toc_ro = toc.clone();
    let mut reader = Reader { chd, toc: &toc_ro };
    let mut reported = 0u64;
    let mut current: Option<(String, File)> = None;
    let mut outputoffs = 0u64;
    let mut discoffs = 0u32;
    let mut buffer: Vec<u8> = Vec::new();
    #[allow(clippy::needless_range_loop)] // tracks refer to their neighbours
    for t in 0..toc.numtrks {
        if current
            .as_ref()
            .is_none_or(|(name, _)| *name != track_filenames[t])
        {
            if let Some((_, mut file)) = current.take() {
                file.flush()?;
            }
            outputoffs = 0;
            if mode != Mode::Gdi {
                discoffs = 0;
            }
            let file = File::create(part_path(Path::new(&track_filenames[t])))?;
            current = Some((track_filenames[t].clone(), file));
        }
        let file = &mut current.as_mut().unwrap().1;

        if is_gdrom && mode == Mode::CueBin {
            if t == 0 {
                text.push_str("REM SINGLE-DENSITY AREA\n");
            } else if toc.tracks[t].physframeofs == HIGH_DENSITY_AREA {
                text.push_str("REM HIGH-DENSITY AREA\n");
            }
        }
        let info = toc.tracks[t];
        track_metadata(
            mode,
            &mut text,
            t,
            &info,
            basename(&track_filenames[t]),
            discoffs,
            outputoffs,
        );

        // a CUE or a GDI cannot hold subcode, which is left out
        let mut frame_size = info.datasize as usize
            + if info.subtype != SUB_NONE {
                info.subsize as usize
            } else {
                0
            };
        if info.subtype != SUB_NONE && mode != Mode::Normal {
            frame_size = info.datasize as usize;
        }
        buffer.resize(
            (TEMP_BUFFER_SIZE / frame_size.max(1)) * frame_size.max(1),
            0,
        );

        let mut bufferoffs = 0usize;
        let actualframes = (info.frames as u32)
            .wrapping_sub(info.padframes as u32)
            .wrapping_add(info.splitframes as u32);
        for frame in 0..actualframes {
            // the first frames of a split track come from the previous one,
            // the reverse of how they are moved when creating the CHD
            let (trk, frameofs) = if t > 0 && frame < info.splitframes as u32 {
                (
                    t - 1,
                    (toc.tracks[t - 1].frames as u32)
                        .wrapping_sub(info.splitframes as u32)
                        .wrapping_add(frame),
                )
            } else {
                (t, frame.wrapping_sub(info.splitframes as u32))
            };
            let track = toc.tracks[trk];
            let lba = (track.physframeofs as u32).wrapping_add(frameofs);
            let need = bufferoffs + track.datasize as usize + track.subsize as usize;
            if buffer.len() < need {
                buffer.resize(need, 0);
            }
            reader.read_data(lba, &mut buffer[bufferoffs..], track.trktype)?;
            // CUE and GDI audio is little-endian; a GDI from a version 4
            // CHD is assumed to be a legacy GD-ROM, already little-endian
            if ((mode == Mode::Gdi && version > 4) || mode == Mode::CueBin)
                && track.trktype == TRACK_AUDIO
            {
                for pair in buffer[bufferoffs..bufferoffs + track.datasize as usize]
                    .as_chunks_mut::<2>()
                    .0
                {
                    pair.swap(0, 1);
                }
            }
            bufferoffs += track.datasize as usize;
            discoffs = discoffs.wrapping_add(1);
            if track.subtype != SUB_NONE && mode == Mode::Normal {
                reader.read_subcode(lba, &mut buffer[bufferoffs..])?;
                bufferoffs += track.subsize as usize;
            }
            let step = u64::from(cdrom::FRAME_SIZE).min(logical_size - reported);
            reported += step;
            progress(step);
            if bufferoffs >= buffer.len() || frame == actualframes - 1 {
                file.write_all(&buffer[..bufferoffs])?;
                outputoffs += bufferoffs as u64;
                bufferoffs = 0;
            }
        }
        discoffs = discoffs.wrapping_add(info.padframes as u32);
    }
    if let Some((_, mut file)) = current {
        file.flush()?;
    }
    std::fs::write(part_path(sheet), text)?;
    Ok(reported)
}

/// The TOC of a GD-ROM rewritten to match the Redump CUE layout as best as
/// chdman 0.289 can.
fn redump_gdrom_layout(toc: &mut Toc) {
    // TOSEC GDI-based CHDs have padframes set where the next pregap would be
    let has_physical_pregap = toc.tracks[0].padframes == 0;
    for t in 1..toc.numtrks {
        // pgdatasize is never set on GD-ROMs, unless the pregaps are right
        if toc.tracks[t].pgdatasize != 0 {
            break;
        }
        // the first tracks of the single- and high-density areas stay
        if toc.tracks[t].physframeofs == HIGH_DENSITY_AREA {
            continue;
        }
        let last = t + 1 >= toc.numtrks;
        if !has_physical_pregap {
            // the pregaps, not in the BINs, become PREGAP commands
            toc.tracks[t].pregap = toc.tracks[t]
                .pregap
                .wrapping_add(toc.tracks[t - 1].padframes);
            if last && toc.tracks[t].trktype != TRACK_AUDIO {
                if toc.tracks[t - 1].trktype != TRACK_AUDIO {
                    // a high-density area of two data tracks, the 3s pregap
                    // baked into the previous track
                    toc.tracks[t - 1].padframes = toc.tracks[t - 1].padframes.wrapping_add(225);
                    toc.tracks[t].pregap = toc.tracks[t].pregap.wrapping_add(225);
                    toc.tracks[t].splitframes = 225;
                    toc.tracks[t].pgdatasize = toc.tracks[t].datasize as i32;
                    toc.tracks[t].pgtype = toc.tracks[t].trktype;
                } else {
                    // data, audio, then data: 75 frames chdman drops
                    toc.tracks[t - 1].frames = toc.tracks[t - 1].frames.wrapping_sub(75);
                    toc.tracks[t].pregap = toc.tracks[t].pregap.wrapping_add(75);
                }
            }
        } else {
            let curextra = if last && toc.tracks[t].trktype != TRACK_AUDIO {
                225
            } else {
                150
            };
            toc.tracks[t - 1].padframes = curextra;
            toc.tracks[t].pregap = toc.tracks[t].pregap.wrapping_add(curextra);
            toc.tracks[t].splitframes = curextra;
            toc.tracks[t].pgdatasize = toc.tracks[t].datasize as i32;
            toc.tracks[t].pgtype = toc.tracks[t].trktype;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_variables_follow_chdmans_regex() {
        assert_eq!(
            format_track_name("disc%02t.bin", 0, true),
            ("disc01.bin".to_owned(), true)
        );
        assert_eq!(
            format_track_name("d (Track %t).bin", 11, true),
            ("d (Track 12).bin".to_owned(), true)
        );
        // an escaped '%' is no variable, and the escapes stay
        assert_eq!(
            format_track_name("d%%t.bin", 0, true),
            ("d%%t.bin".to_owned(), false)
        );
        assert_eq!(
            format_track_name("d%%%t.bin", 2, true),
            ("d%%3.bin".to_owned(), true)
        );
        // without splitting, a %t is left as it is
        assert_eq!(
            format_track_name("d%t.bin", 0, false),
            ("d%t.bin".to_owned(), false)
        );
        assert_eq!(
            format_track_name("d%-3t|", 4, true),
            ("d5  |".to_owned(), true)
        );
    }

    #[test]
    fn frames_print_as_minutes_seconds_frames() {
        assert_eq!(msf(0), "00:00:00");
        assert_eq!(msf(150), "00:02:00");
        assert_eq!(msf(4500 + 75 + 3), "01:01:03");
    }
}
