//! The CD table of contents: its model, its parsers and its CHD metadata.
//!
//! This mirrors `cdrom_file` from MAME's `src/lib/util/cdrom.cpp`, keeping
//! its quirks verbatim: the plain `PGTYPE` branch reports the pregap track
//! type, not the track type, and the `V` prefix that marks a valid pregap
//! submode only appears when the pregap carries data.

use crate::container::{MDFLAGS_CHECKSUM, MTAG_GDROM_TRACK, MTAG_SESSION, MTAG_TRACK2};

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
/// tags, flags and bytes `cdrom_file::write_metadata()` writes them: one
/// `CHT2` or `CHGD` entry per track, and a `CHSE` entry before the first
/// track of each session once the disc has more than one.
pub(crate) fn metadata_entries(toc: &Toc) -> Vec<(u32, u8, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut sessionnum = u32::MAX;

    for (i, track) in toc.tracks.iter().enumerate() {
        // the pregap submode, with a 'V' prefix where it carries data
        let submode = if track.pgdatasize > 0 {
            format!("V{}", type_string(track.pgtype))
        } else {
            type_string(track.pgtype).to_owned()
        };

        if toc.numsessions > 1 && sessionnum != track.session {
            entries.push(cstr_entry(
                MTAG_SESSION,
                &format!("SESSION:{}", track.session + 1),
            ));
            sessionnum = track.session;
        }

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
                    submode,
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
        // a valid pregap submode carries the 'V' prefix; sessions come first
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
        let entries = metadata_entries(&toc);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].0, u32::from_be_bytes(*b"CHSE"));
        assert_eq!(entries[0].2, b"SESSION:1\0");
        assert_eq!(entries[1].2, b"TRACK:1 TYPE:AUDIO SUBTYPE:RW_RAW FRAMES:75 PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0\0");
        assert_eq!(entries[2].0, u32::from_be_bytes(*b"CHSE"));
        assert_eq!(entries[2].2, b"SESSION:2\0");
        assert_eq!(entries[3].2, b"TRACK:2 TYPE:AUDIO SUBTYPE:RW_RAW FRAMES:75 PREGAP:0 PGTYPE:VMODE1 PGSUB:RW POSTGAP:0\0");
    }
}
