//! The CRC-16 used by CHD version 5 files, matching MAME's `util::crc16`.
//!
//! CCITT-FALSE: polynomial 0x1021, reflected neither in nor out, initial
//! value 0xFFFF, no final XOR, computed a byte at a time from a 256-entry
//! table as MAME does.

/// The CRC of each byte value, shifted into the top of the register.
const TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut crc = (byte as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[byte] = crc;
        byte += 1;
    }
    table
};

/// The CRC-16 of `data`, as stored in version 5 CHD maps and hunk headers.
pub(crate) fn crc16(data: &[u8]) -> u16 {
    data.iter().fold(0xffff, |crc, &byte| {
        (crc << 8) ^ TABLE[usize::from((crc >> 8) as u8 ^ byte)]
    })
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
