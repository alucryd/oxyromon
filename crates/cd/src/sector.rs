//! A raw CD-ROM sector, 2352 bytes, as what has to be stored to rebuild it
//! and what can be generated.
//!
//! Every data sector begins with a 12-byte sync and a 4-byte header, its
//! address and mode, both generated from where the sector lies. Mode 1
//! holds 2048 bytes of data, then its EDC, 8 zeros and its ECC; Mode 2
//! holds an 8-byte subheader, its 4 bytes twice, then in Form 1 2048 bytes
//! of data, EDC and ECC, or in Form 2 2324 bytes of data and an EDC the
//! standard lets be zero. A sector that fits none of these exactly, or
//! whose header names another address, is kept as it is.

use super::ecc;
use super::edc::edc;

pub const SECTOR: usize = 2352;
pub const SYNC: [u8; 12] = [0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0];
/// Frames before the first sector of the disc: 00:02:00.
pub const LEAD_IN: u32 = 150;
pub const FORM1_DATA: usize = 2048;
pub const FORM2_DATA: usize = 2324;

/// What a raw sector is, as far as rebuilding it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    /// Kept as it is: nothing about it is generated.
    Raw = 0,
    /// All zeros.
    Zero = 1,
    /// Mode 1: 2048 bytes of data.
    Mode1 = 2,
    /// Mode 2 Form 1: a subheader and 2048 bytes of data.
    Form1 = 3,
    /// Mode 2 Form 2: a subheader and 2324 bytes of data.
    Form2 = 4,
    /// Mode 2 Form 2 with its EDC left zero.
    Form2NoEdc = 5,
}

impl Kind {
    /// The bytes of data it holds, what is left to store once the rest is
    /// generated: all of them for a raw sector, none for zeros.
    pub fn data_len(self) -> usize {
        match self {
            Self::Raw => SECTOR,
            Self::Zero => 0,
            Self::Mode1 | Self::Form1 => FORM1_DATA,
            Self::Form2 | Self::Form2NoEdc => FORM2_DATA,
        }
    }

    pub fn has_subheader(self) -> bool {
        matches!(self, Self::Form1 | Self::Form2 | Self::Form2NoEdc)
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Raw,
            1 => Self::Zero,
            2 => Self::Mode1,
            3 => Self::Form1,
            4 => Self::Form2,
            5 => Self::Form2NoEdc,
            _ => return None,
        })
    }
}

fn bcd(value: u32) -> u8 {
    (((value / 10) << 4) | (value % 10)) as u8
}

/// The header address of the sector `frame` frames from 00:00:00: its
/// minute, second and frame, in BCD.
pub fn address(frame: u32) -> [u8; 3] {
    [bcd(frame / 75 / 60), bcd(frame / 75 % 60), bcd(frame % 75)]
}

fn le(sector: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(sector[at..at + 4].try_into().unwrap())
}

/// What the sector `frame` frames from 00:00:00 is, checked against every
/// field it would be rebuilt with.
pub fn classify(sector: &[u8; SECTOR], frame: u32) -> Kind {
    if sector.iter().all(|&byte| byte == 0) {
        return Kind::Zero;
    }
    if sector[..12] != SYNC || sector[12..15] != address(frame) {
        return Kind::Raw;
    }
    match sector[15] {
        1 if edc(&sector[..0x810]) == le(sector, 0x810)
            && sector[0x814..0x81C].iter().all(|&byte| byte == 0)
            && ecc::ecc_verify(sector) =>
        {
            Kind::Mode1
        }
        2 if sector[18] & 0x20 == 0 => {
            if edc(&sector[16..0x818]) == le(sector, 0x818) && ecc::ecc_verify(sector) {
                Kind::Form1
            } else {
                Kind::Raw
            }
        }
        2 => {
            let stored = le(sector, 0x92C);
            if edc(&sector[16..0x92C]) == stored {
                Kind::Form2
            } else if stored == 0 {
                Kind::Form2NoEdc
            } else {
                Kind::Raw
            }
        }
        _ => Kind::Raw,
    }
}

/// Where a sector's data lies in it.
pub fn data_range(kind: Kind) -> std::ops::Range<usize> {
    match kind {
        Kind::Raw => 0..SECTOR,
        Kind::Zero => 0..0,
        Kind::Mode1 => 16..16 + FORM1_DATA,
        Kind::Form1 => 24..24 + FORM1_DATA,
        Kind::Form2 | Kind::Form2NoEdc => 24..24 + FORM2_DATA,
    }
}

/// Rebuild the sector `frame` frames from 00:00:00 of `kind`, from its
/// subheader, both copies, and its data, [`Kind::data_len`] bytes.
pub fn rebuild(kind: Kind, frame: u32, subheader: &[u8; 8], data: &[u8], out: &mut [u8; SECTOR]) {
    if kind == Kind::Raw {
        out.copy_from_slice(data);
        return;
    }
    out.fill(0);
    if kind == Kind::Zero {
        return;
    }
    out[..12].copy_from_slice(&SYNC);
    out[12..15].copy_from_slice(&address(frame));
    out[data_range(kind)].copy_from_slice(data);
    match kind {
        Kind::Mode1 => {
            out[15] = 1;
            let edc = edc(&out[..0x810]);
            out[0x810..0x814].copy_from_slice(&edc.to_le_bytes());
            ecc::ecc_generate(out);
        }
        Kind::Form1 => {
            out[15] = 2;
            out[16..24].copy_from_slice(subheader);
            let edc = edc(&out[16..0x818]);
            out[0x818..0x81C].copy_from_slice(&edc.to_le_bytes());
            ecc::ecc_generate(out);
        }
        Kind::Form2 | Kind::Form2NoEdc => {
            out[15] = 2;
            out[16..24].copy_from_slice(subheader);
            if kind == Kind::Form2 {
                let edc = edc(&out[16..0x92C]);
                out[0x92C..].copy_from_slice(&edc.to_le_bytes());
            }
        }
        Kind::Raw | Kind::Zero => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, mut state: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    #[test]
    fn every_kind_rebuilds_to_what_it_was_classified_from() {
        let frame = LEAD_IN + 12_345;
        let subheader = [1, 2, 0x08, 0, 1, 2, 0x08, 0];
        let form2 = [1, 2, 0x28, 0, 1, 2, 0x28, 0];
        for (kind, subheader) in [
            (Kind::Mode1, [0; 8]),
            (Kind::Form1, subheader),
            (Kind::Form2, form2),
            (Kind::Form2NoEdc, form2),
        ] {
            let data = noise(kind.data_len(), frame as u64 + kind as u64);
            let mut sector = [0; SECTOR];
            rebuild(kind, frame, &subheader, &data, &mut sector);
            assert_eq!(classify(&sector, frame), kind);
            assert_eq!(sector[data_range(kind)], data[..]);
            // Elsewhere on the disc, its header says otherwise
            assert_eq!(classify(&sector, frame + 1), Kind::Raw);
            // A byte of data changed breaks every kind but the one without
            // an EDC, which nothing checks
            sector[0x100] ^= 1;
            let changed = classify(&sector, frame);
            if kind == Kind::Form2NoEdc {
                assert_eq!(changed, Kind::Form2NoEdc);
            } else {
                assert_eq!(changed, Kind::Raw, "{kind:?}");
            }
        }
        assert_eq!(classify(&[0; SECTOR], frame), Kind::Zero);
        assert_eq!(address(LEAD_IN), [0x00, 0x02, 0x00]);
        assert_eq!(address(75 * 60 * 74 + 75 * 59 + 74), [0x74, 0x59, 0x74]);
    }
}
