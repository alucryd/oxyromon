//! A CD data track as an ORE stores it: its sectors' data in one stream,
//! and a map of what each sector is, from which the rest is generated.
//!
//! The map is a byte a sector, its [`Kind`], with bit 7 set when its
//! subheader's two copies differ; then, for a sector with a subheader, its
//! first copy, and its second when it differs. The map is Zstandard
//! compressed in the area's metadata, a few KB a disc.

use super::cd::sector::{self, Kind, SECTOR};
use super::error::{Error, Result};

const DIFFERS: u8 = 0x80;

/// The sectors of a data track: what each is, and where its data lies in
/// the track's stream.
#[derive(Debug)]
pub struct SectorMap {
    kinds: Vec<Kind>,
    subheaders: Vec<[u8; 8]>,
    /// Where each sector's data starts in the stream, and where the last
    /// ends.
    starts: Vec<u64>,
}

/// The map being written, a sector at a time.
#[derive(Default)]
pub struct MapWriter {
    bytes: Vec<u8>,
}

impl MapWriter {
    /// Add the raw sector `raw`, `frame` frames from 00:00:00; returns its
    /// kind, and so which of its bytes are its data.
    pub fn push(&mut self, raw: &[u8; SECTOR], frame: u32) -> Kind {
        let kind = sector::classify(raw, frame);
        if kind.has_subheader() {
            let differs = raw[16..20] != raw[20..24];
            self.bytes
                .push(kind as u8 | if differs { DIFFERS } else { 0 });
            self.bytes
                .extend_from_slice(&raw[16..if differs { 24 } else { 20 }]);
        } else {
            self.bytes.push(kind as u8);
        }
        kind
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
    /// Read the map of `sectors` sectors.
    pub fn read(compressed: &[u8], sectors: u64) -> Result<Self> {
        // A sector takes 9 bytes of map at most
        let bytes = zstd::bulk::decompress(compressed, sectors as usize * 9)
            .map_err(|e| corrupt(&e.to_string()))?;
        let mut map = Self {
            kinds: Vec::with_capacity(sectors as usize),
            subheaders: Vec::with_capacity(sectors as usize),
            starts: Vec::with_capacity(sectors as usize + 1),
        };
        let mut at = 0;
        let mut start = 0;
        while at < bytes.len() {
            let byte = bytes[at];
            at += 1;
            let kind = Kind::from_u8(byte & !DIFFERS).ok_or_else(|| corrupt("a bad kind"))?;
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
            map.kinds.push(kind);
            map.subheaders.push(subheader);
            map.starts.push(start);
            start += kind.data_len() as u64;
        }
        map.starts.push(start);
        if map.kinds.len() as u64 != sectors {
            return Err(corrupt("not a sector a sector"));
        }
        Ok(map)
    }

    /// The length of the track's stream.
    pub fn data_len(&self) -> u64 {
        *self.starts.last().unwrap()
    }

    /// Where the data of sectors `range` lies in the stream.
    pub fn data_range(&self, sectors: std::ops::Range<usize>) -> std::ops::Range<u64> {
        self.starts[sectors.start]..self.starts[sectors.end]
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
    fn a_map_gives_back_what_it_was_written_from() {
        let mut sectors = Vec::new();
        for (i, (kind, subheader)) in [
            (Kind::Form1, [0, 0, 8, 0, 0, 0, 8, 0]),
            (Kind::Form2, [1, 1, 0x64, 1, 1, 1, 0x64, 1]),
            (Kind::Form2NoEdc, [1, 3, 0x64, 1, 1, 1, 0x64, 1]),
            (Kind::Zero, [0; 8]),
        ]
        .into_iter()
        .enumerate()
        {
            let data: Vec<u8> = (0..kind.data_len()).map(|b| (b * 7 + i) as u8).collect();
            let mut raw = [0; SECTOR];
            sector::rebuild(kind, LEAD_IN + i as u32, &subheader, &data, &mut raw);
            sectors.push((raw, data));
        }
        let mut weird = [0x5A; SECTOR];
        weird[..12].copy_from_slice(&sector::SYNC);
        sectors.push((weird, weird.to_vec()));

        let mut writer = MapWriter::default();
        let mut stream = Vec::new();
        for (i, (raw, data)) in sectors.iter().enumerate() {
            let kind = writer.push(raw, LEAD_IN + i as u32);
            assert_eq!(&raw[sector::data_range(kind)], &data[..]);
            stream.extend_from_slice(data);
        }
        let map = SectorMap::read(&writer.finish(), sectors.len() as u64).unwrap();
        assert_eq!(map.data_len(), stream.len() as u64);
        for (i, (raw, _)) in sectors.iter().enumerate() {
            let range = map.data_range(i..i + 1);
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
