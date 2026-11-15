//! A CD data track as an ORE stores it: its sectors' data in one stream,
//! and a map of what each sector is, from which the rest is generated.
//!
//! The map is a byte a sector, its [`Kind`], with bit 7 set when its
//! subheader's two copies differ and bit 6 when it is a video sector; then, for a sector with a subheader, its
//! first copy, and its second when it differs. The map is Zstandard
//! compressed in the area's metadata, a few KB a disc.
//!
//! The stream is three planes: the data of every other sector, in order;
//! then the data of the STR video sectors, Form 1 sectors of a video
//! sector header and Form 2 ones of Final Fantasy IX's, in order, as the
//! `str` codec takes them; then the XA
//! ADPCM sectors, in order, each a record of its subheader and its data,
//! as the `xa` codec takes them.

use std::io::Read;
use std::ops::Range;

use super::cd::sector::{self, Kind, SECTOR};
use super::error::{Error, Result};
use super::video;
use super::xa;

/// The planes of a data track's stream, in their order there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Data = 0,
    /// Form 1 sectors of STR video.
    Video = 1,
    /// Form 2 sectors of XA ADPCM.
    Xa = 2,
}

/// Whether a sector goes in the XA plane: Form 2, XA ADPCM.
pub fn is_xa(kind: Kind, subheader: &[u8]) -> bool {
    matches!(kind, Kind::Form2 | Kind::Form2NoEdc) && xa::is_adpcm(subheader)
}

/// What a plane stores of a sector of `data_len` bytes of data.
fn record_len(plane: Plane, data_len: usize) -> u64 {
    match plane {
        Plane::Data | Plane::Video => data_len as u64,
        Plane::Xa => xa::RECORD as u64,
    }
}

const DIFFERS: u8 = 0x80;
const VIDEO: u8 = 0x40;

/// The sectors of a data track: what each is, and where its data lies in
/// the track's stream.
#[derive(Debug)]
pub struct SectorMap {
    kinds: Vec<Kind>,
    subheaders: Vec<[u8; 8]>,
    planes: Vec<Plane>,
    /// Where each sector's record starts in each plane, or would, and
    /// where the last ends: the planes' lengths.
    starts: Vec<[u64; 3]>,
}

/// The map being written, a sector at a time.
#[derive(Default)]
pub struct MapWriter {
    bytes: Vec<u8>,
}

impl MapWriter {
    /// Add the raw sector `raw`, `frame` frames from 00:00:00; returns its
    /// kind, and so which of its bytes are its data, and its plane.
    pub fn push(&mut self, raw: &[u8; SECTOR], frame: u32) -> (Kind, Plane) {
        let kind = sector::classify(raw, frame);
        let form2 = matches!(kind, Kind::Form2 | Kind::Form2NoEdc);
        let plane = if (kind == Kind::Form1 || form2) && video::is_video(&raw[24..], form2) {
            Plane::Video
        } else if is_xa(kind, &raw[16..20]) {
            Plane::Xa
        } else {
            Plane::Data
        };
        if kind.has_subheader() {
            let differs = raw[16..20] != raw[20..24];
            self.bytes.push(
                kind as u8
                    | if differs { DIFFERS } else { 0 }
                    | if plane == Plane::Video { VIDEO } else { 0 },
            );
            self.bytes
                .extend_from_slice(&raw[16..if differs { 24 } else { 20 }]);
        } else {
            self.bytes.push(kind as u8);
        }
        (kind, plane)
    }

    /// The map, compressed.
    pub fn finish(self) -> Vec<u8> {
        zstd::bulk::compress(&self.bytes, 19).expect("compressing to memory does not fail")
    }
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(format!("a CD track's sector map: {what}"))
}

impl SectorMap {
    /// Read the map of `sectors` sectors. `sectors` comes from the ORE, so
    /// nothing is allocated for it: the map is decoded as far as it goes,
    /// 9 bytes a sector at most, and sized by what it holds, a byte a
    /// sector at least.
    pub fn read(compressed: &[u8], sectors: u64) -> Result<Self> {
        let limit = sectors.saturating_mul(9);
        let mut bytes = Vec::new();
        zstd::stream::read::Decoder::with_buffer(compressed)
            .map_err(|e| corrupt(&e.to_string()))?
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| corrupt(&e.to_string()))?;
        if bytes.len() as u64 > limit {
            return Err(corrupt("longer than its sectors"));
        }
        let count = bytes.len().min(sectors as usize);
        let mut map = Self {
            kinds: Vec::with_capacity(count),
            subheaders: Vec::with_capacity(count),
            planes: Vec::with_capacity(count),
            starts: Vec::with_capacity(count + 1),
        };
        let mut at = 0;
        let mut start = [0; 3];
        while at < bytes.len() {
            let byte = bytes[at];
            at += 1;
            let kind =
                Kind::from_u8(byte & !(DIFFERS | VIDEO)).ok_or_else(|| corrupt("a bad kind"))?;
            if byte & VIDEO != 0 && !kind.has_subheader() {
                return Err(corrupt("video in a sector not of Mode 2"));
            }
            let mut subheader = [0; 8];
            if kind.has_subheader() {
                let copies = if byte & DIFFERS != 0 { 8 } else { 4 };
                let stored = bytes
                    .get(at..at + copies)
                    .ok_or_else(|| corrupt("cut short"))?;
                subheader[..copies].copy_from_slice(stored);
                if copies == 4 {
                    subheader.copy_within(..4, 4);
                }
                at += copies;
            } else if byte & DIFFERS != 0 {
                return Err(corrupt("a second copy without a subheader"));
            }
            let plane = if byte & VIDEO != 0 {
                Plane::Video
            } else if is_xa(kind, &subheader) {
                Plane::Xa
            } else {
                Plane::Data
            };
            map.kinds.push(kind);
            map.subheaders.push(subheader);
            map.planes.push(plane);
            map.starts.push(start);
            start[plane as usize] += record_len(plane, kind.data_len());
        }
        map.starts.push(start);
        if map.kinds.len() as u64 != sectors {
            return Err(corrupt("not a sector a sector"));
        }
        Ok(map)
    }

    /// Where each plane starts in the stream, and where the last ends: the
    /// stream's length.
    pub fn plane_starts(&self) -> [u64; 4] {
        let [data, video, xa] = *self.starts.last().unwrap();
        [0, data, data + video, data + video + xa]
    }

    /// The length of the track's stream.
    pub fn stored_len(&self) -> u64 {
        self.plane_starts()[3]
    }

    /// Where the stored bytes of sectors `sectors` lie in the stream, in
    /// each plane.
    pub fn ranges(&self, sectors: Range<usize>) -> [Range<u64>; 3] {
        let base = self.plane_starts();
        let (from, to) = (self.starts[sectors.start], self.starts[sectors.end]);
        [0, 1, 2].map(|p| base[p] + from[p]..base[p] + to[p])
    }

    /// Where sector `i`'s data lies in the stream, and in which plane.
    pub fn at(&self, i: usize) -> (usize, Range<u64>) {
        let plane = self.planes[i] as usize;
        let range = self.ranges(i..i + 1)[plane].clone();
        if self.planes[i] == Plane::Xa {
            (plane, range.start + 4..range.end)
        } else {
            (plane, range)
        }
    }

    /// Rebuild sector `i` of the track, `frame` frames from 00:00:00, from
    /// its data.
    pub fn rebuild(&self, i: usize, frame: u32, data: &[u8], out: &mut [u8; SECTOR]) {
        sector::rebuild(self.kinds[i], frame, &self.subheaders[i], data, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cd::sector::LEAD_IN;

    #[test]
    fn a_map_claiming_more_sectors_than_it_holds_is_refused_unallocated() {
        let mut writer = MapWriter::default();
        let mut raw = [0; SECTOR];
        sector::rebuild(Kind::Zero, LEAD_IN, &[0; 8], &[], &mut raw);
        writer.push(&raw, LEAD_IN);
        let compressed = writer.finish();
        // Terabytes of CD claimed for a byte of map: an error, not an
        // allocation of what is claimed
        for sectors in [2, 1 << 40, u64::MAX] {
            assert!(SectorMap::read(&compressed, sectors).is_err());
        }
        assert!(SectorMap::read(&compressed, 1).is_ok());
        // A map that decodes to more than its sectors can take
        let long = zstd::bulk::compress(&[0; 64], 3).unwrap();
        assert!(SectorMap::read(&long, 1).is_err());
    }

    #[test]
    fn a_map_gives_back_what_it_was_written_from() {
        let mut sectors = Vec::new();
        for (i, (kind, subheader)) in [
            (Kind::Form1, [0, 0, 8, 0, 0, 0, 8, 0]),
            (Kind::Form2, [1, 1, 0x64, 1, 1, 1, 0x64, 1]),
            (Kind::Form2NoEdc, [1, 3, 0x64, 1, 1, 1, 0x64, 1]),
            (Kind::Zero, [0; 8]),
            (Kind::Form1, [0, 0, 8, 0, 0, 0, 8, 0]),
            (Kind::Form2, [0, 1, 0x68, 0, 0, 1, 0x68, 0]),
        ]
        .into_iter()
        .enumerate()
        {
            let mut data: Vec<u8> = (0..kind.data_len()).map(|b| (b * 7 + i) as u8).collect();
            if i == 4 {
                data[..8].copy_from_slice(&[0x60, 1, 1, 0x80, 0, 0, 1, 0]);
            }
            if i == 5 {
                data[..8].copy_from_slice(&[0x60, 1, 4, 0, 2, 0, 10, 0]);
            }
            let mut raw = [0; SECTOR];
            sector::rebuild(kind, LEAD_IN + i as u32, &subheader, &data, &mut raw);
            sectors.push((raw, data));
        }
        let mut weird = [0x5A; SECTOR];
        weird[..12].copy_from_slice(&sector::SYNC);
        sectors.push((weird, weird.to_vec()));

        let mut writer = MapWriter::default();
        let mut planes = [Vec::new(), Vec::new(), Vec::new()];
        for (i, (raw, data)) in sectors.iter().enumerate() {
            let (kind, plane) = writer.push(raw, LEAD_IN + i as u32);
            assert_eq!(&raw[sector::data_range(kind)], &data[..]);
            if plane == Plane::Xa {
                planes[2].extend_from_slice(&raw[16..20]);
            }
            planes[plane as usize].extend_from_slice(data);
        }
        assert_eq!(
            planes[1].len(),
            2048 + 2324,
            "the second Form 1 sector and the third Form 2 are video"
        );
        assert_eq!(
            planes[2].len(),
            2 * xa::RECORD,
            "the two Form 2 sectors are XA"
        );
        let stream = planes.concat();
        let map = SectorMap::read(&writer.finish(), sectors.len() as u64).unwrap();
        assert_eq!(map.stored_len(), stream.len() as u64);
        for (i, (raw, _)) in sectors.iter().enumerate() {
            let (_, range) = map.at(i);
            let mut out = [0; SECTOR];
            map.rebuild(
                i,
                LEAD_IN + i as u32,
                &stream[range.start as usize..range.end as usize],
                &mut out,
            );
            assert_eq!(&out, raw, "sector {i}");
        }
        assert!(SectorMap::read(&MapWriter::default().finish(), 1).is_err());
    }
}
