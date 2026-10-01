//! The CRC-16 used by CHD version 5 files, matching MAME's `util::crc16`.
//!
//! CCITT-FALSE: polynomial 0x1021, reflected neither in nor out, initial
//! value 0xFFFF, no final XOR. MAME computes it with a 256-entry table
//! derived from the same recurrence; this straightforward bit loop is
//! equivalent.

/// The CRC-16 of `data`, as stored in version 5 CHD maps and hunk headers.
pub(crate) fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answer() {
        // The CCITT-FALSE check value for the standard test vector.
        assert_eq!(crc16(b"123456789"), 0x29b1);
    }

    #[test]
    fn empty() {
        assert_eq!(crc16(b""), 0xffff);
    }
}
