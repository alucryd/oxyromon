//! The CD-ROM EDC: a CRC-32 of polynomial 0x8001801B, reflected, without
//! initial value or final xor, as ECMA-130 specifies.

const TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xD801_8001 } else { 0 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// The EDC of `data`.
pub fn edc(data: &[u8]) -> u32 {
    data.iter().fold(0, |crc, &b| {
        (crc >> 8) ^ TABLE[((crc ^ b as u32) & 0xFF) as usize]
    })
}
