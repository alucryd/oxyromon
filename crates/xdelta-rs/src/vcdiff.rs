//! VCDIFF (RFC 3284) as xdelta3 writes it: the default code table, and its
//! extensions for a per-window Adler-32 and secondary compression.

use crate::error::{Error, Result};
use std::fs::File;
use std::io::{self, Read};

const MAGIC: [u8; 3] = [0xD6, 0xC3, 0xC4];

const VCD_SECONDARY: u8 = 1;
const VCD_CODETABLE: u8 = 2;
const VCD_APPHEADER: u8 = 4;

const VCD_SOURCE: u8 = 1;
const VCD_TARGET: u8 = 2;
const VCD_ADLER32: u8 = 4;

pub const VCD_DATACOMP: u8 = 1;
pub const VCD_INSTCOMP: u8 = 2;
pub const VCD_ADDRCOMP: u8 = 4;

/// xdelta3's `XD3_HARDMAXWINSIZE`: it rejects any window larger.
pub const MAX_WINDOW: u64 = 1 << 26;

pub fn corrupt(message: impl Into<String>) -> Error {
    Error::Corrupt(message.into())
}

fn truncated(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::UnexpectedEof => corrupt("the patch is truncated"),
        _ => Error::Io(error),
    }
}

pub fn read_byte<R: Read>(r: &mut R) -> Result<u8> {
    let mut byte = [0];
    r.read_exact(&mut byte).map_err(truncated)?;
    Ok(byte[0])
}

/// A base-128, most significant group first, integer.
pub fn read_integer<R: Read>(r: &mut R) -> Result<u64> {
    let mut value: u64 = 0;
    loop {
        let byte = read_byte(r)?;
        if value >> 57 != 0 {
            return Err(corrupt("an integer overflows 64 bits"));
        }
        value = (value << 7) | u64::from(byte & 0x7F);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
}

fn integer_len(mut value: u64) -> u64 {
    let mut len = 1;
    while value >= 0x80 {
        value >>= 7;
        len += 1;
    }
    len
}

fn read_bytes<R: Read>(r: &mut R, len: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    r.take(len).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(corrupt("the patch is truncated"));
    }
    Ok(bytes)
}

pub struct FileHeader {
    /// The secondary compressor's ID, when the patch declares one.
    pub secondary: Option<u8>,
    pub app_header: Vec<u8>,
}

pub fn read_file_header<R: Read>(r: &mut R) -> Result<FileHeader> {
    let mut magic = [0; 4];
    r.read_exact(&mut magic).map_err(truncated)?;
    if magic[..3] != MAGIC {
        return Err(Error::BadMagic {
            expected: "d6c3c4".into(),
            found: magic[..3].iter().map(|b| format!("{b:02x}")).collect(),
        });
    }
    if magic[3] != 0 {
        return Err(Error::Unsupported(format!("VCDIFF version {}", magic[3])));
    }
    let indicator = read_byte(r)?;
    if indicator & !7 != 0 {
        return Err(corrupt("invalid header indicator"));
    }
    let secondary = if indicator & VCD_SECONDARY != 0 {
        Some(read_byte(r)?)
    } else {
        None
    };
    if indicator & VCD_CODETABLE != 0 {
        return Err(Error::Unsupported("application-defined code tables".into()));
    }
    let app_header = if indicator & VCD_APPHEADER != 0 {
        let len = read_integer(r)?;
        read_bytes(r, len)?
    } else {
        Vec::new()
    };
    Ok(FileHeader {
        secondary,
        app_header,
    })
}

pub struct Window {
    /// Length and position of the source segment copies read from.
    pub source: Option<(u64, u64)>,
    pub target_len: usize,
    pub delta_indicator: u8,
    pub data: Vec<u8>,
    pub inst: Vec<u8>,
    pub addr: Vec<u8>,
    pub adler32: Option<u32>,
}

/// The next window, or `None` at the end of the patch.
pub fn read_window<R: Read>(r: &mut R, secondary: bool) -> Result<Option<Window>> {
    let mut indicator = [0];
    if r.read(&mut indicator)? == 0 {
        return Ok(None);
    }
    let indicator = indicator[0];
    if indicator & !7 != 0 || indicator & VCD_SOURCE != 0 && indicator & VCD_TARGET != 0 {
        return Err(corrupt("invalid window indicator"));
    }
    if indicator & VCD_TARGET != 0 {
        // xdelta3 does not implement these either.
        return Err(Error::Unsupported("windows copying from the target".into()));
    }
    let source = if indicator & VCD_SOURCE != 0 {
        Some((read_integer(r)?, read_integer(r)?))
    } else {
        None
    };
    let encoding_len = read_integer(r)?;
    let target_len = read_integer(r)?;
    if target_len > MAX_WINDOW {
        return Err(Error::Unsupported(format!(
            "a window of {target_len} bytes, past xdelta3's 64 MiB limit"
        )));
    }
    let delta_indicator = read_byte(r)?;
    if delta_indicator & !7 != 0 {
        return Err(corrupt("invalid delta indicator"));
    }
    if delta_indicator != 0 && !secondary {
        return Err(corrupt(
            "a compressed section without a secondary compressor",
        ));
    }
    let (data_len, inst_len, addr_len) = (read_integer(r)?, read_integer(r)?, read_integer(r)?);
    let adler32 = if indicator & VCD_ADLER32 != 0 {
        let mut checksum = [0; 4];
        r.read_exact(&mut checksum).map_err(truncated)?;
        Some(u32::from_be_bytes(checksum))
    } else {
        None
    };
    let expected = [target_len, data_len, inst_len, addr_len]
        .iter()
        .map(|&len| integer_len(len))
        .sum::<u64>()
        + 1
        + if adler32.is_some() { 4 } else { 0 };
    let sections = data_len
        .checked_add(inst_len)
        .and_then(|len| len.checked_add(addr_len))
        .and_then(|len| len.checked_add(expected));
    if sections != Some(encoding_len) {
        return Err(corrupt("a window's lengths disagree"));
    }
    Ok(Some(Window {
        source,
        target_len: target_len as usize,
        delta_indicator,
        data: read_bytes(r, data_len)?,
        inst: read_bytes(r, inst_len)?,
        addr: read_bytes(r, addr_len)?,
        adler32,
    }))
}

const NOOP: u8 = 0;
const ADD: u8 = 1;
const RUN: u8 = 2;
const COPY: u8 = 3;

/// `(type, size, mode)`, twice per code: RFC 3284's default code table.
type Code = [(u8, u8, u8); 2];

const CODE_TABLE: [Code; 256] = {
    let none = (NOOP, 0, 0);
    let mut table = [[none; 2]; 256];
    table[0][0] = (RUN, 0, 0);
    let mut i = 1;
    let mut size = 0;
    while size <= 17 {
        table[i][0] = (ADD, size, 0);
        i += 1;
        size += 1;
    }
    let mut mode = 0;
    while mode <= 8 {
        table[i][0] = (COPY, 0, mode);
        i += 1;
        let mut size = 4;
        while size <= 18 {
            table[i][0] = (COPY, size, mode);
            i += 1;
            size += 1;
        }
        mode += 1;
    }
    let mut mode = 0;
    while mode <= 5 {
        let mut add = 1;
        while add <= 4 {
            let mut copy = 4;
            while copy <= 6 {
                table[i] = [(ADD, add, 0), (COPY, copy, mode)];
                i += 1;
                copy += 1;
            }
            add += 1;
        }
        mode += 1;
    }
    let mut mode = 6;
    while mode <= 8 {
        let mut add = 1;
        while add <= 4 {
            table[i] = [(ADD, add, 0), (COPY, 4, mode)];
            i += 1;
            add += 1;
        }
        mode += 1;
    }
    let mut mode = 0;
    while mode <= 8 {
        table[i] = [(COPY, 4, mode), (ADD, 1, 0)];
        i += 1;
        mode += 1;
    }
    assert!(i == 256);
    table
};

/// RFC 3284's address cache, with its default 4 near and 3 same slots.
struct AddressCache {
    near: [u64; 4],
    next: usize,
    same: [u64; 3 * 256],
}

impl AddressCache {
    fn new() -> Self {
        AddressCache {
            near: [0; 4],
            next: 0,
            same: [0; 3 * 256],
        }
    }

    fn decode(&mut self, addresses: &mut &[u8], mode: u8, here: u64) -> Result<u64> {
        let address = match mode {
            0 => Some(read_integer(addresses)?),
            1 => here.checked_sub(read_integer(addresses)?),
            2..=5 => self.near[mode as usize - 2].checked_add(read_integer(addresses)?),
            6..=8 => {
                let byte = read_byte(addresses)?;
                Some(self.same[(mode as usize - 6) * 256 + byte as usize])
            }
            _ => None,
        }
        .filter(|&address| address < here)
        .ok_or_else(|| corrupt("a copy from past the current position"))?;
        self.near[self.next] = address;
        self.next = (self.next + 1) % self.near.len();
        self.same[(address % self.same.len() as u64) as usize] = address;
        Ok(address)
    }
}

fn take<'a>(section: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if section.len() < len {
        return Err(corrupt("an instruction runs past its section"));
    }
    let (taken, rest) = section.split_at(len);
    *section = rest;
    Ok(taken)
}

/// Rebuild a window's target into `target` from its sections, which must
/// already be decompressed.
pub fn apply(window: &Window, source: Option<&File>, target: &mut Vec<u8>) -> Result<()> {
    let (segment_len, segment_pos) = window.source.unwrap_or((0, 0));
    let source = match (window.source, source) {
        (Some(_), None) => {
            return Err(Error::InvalidOption("the patch needs a source file".into()));
        }
        (_, source) => source,
    };
    let (mut data, mut inst, mut addr) = (&window.data[..], &window.inst[..], &window.addr[..]);
    let mut cache = AddressCache::new();
    target.clear();
    target.reserve(window.target_len);

    while !inst.is_empty() {
        let code = CODE_TABLE[read_byte(&mut inst)? as usize];
        for (kind, size, mode) in code {
            if kind == NOOP {
                continue;
            }
            let size = match size {
                0 => read_integer(&mut inst)?,
                size => u64::from(size),
            };
            if size > (window.target_len - target.len()) as u64 {
                return Err(corrupt("instructions overrun the target window"));
            }
            let size = size as usize;
            match kind {
                ADD => target.extend_from_slice(take(&mut data, size)?),
                RUN => {
                    let byte = take(&mut data, 1)?[0];
                    target.resize(target.len() + size, byte);
                }
                _ => {
                    let here = segment_len + target.len() as u64;
                    let mut address = cache.decode(&mut addr, mode, here)?;
                    let mut left = size;
                    if address < segment_len {
                        let len = left.min((segment_len - address) as usize);
                        let start = target.len();
                        target.resize(start + len, 0);
                        read_source(source, &mut target[start..], segment_pos + address)?;
                        left -= len;
                        address += len as u64;
                    }
                    if left > 0 {
                        // What overlaps the bytes being written repeats them, so byte by byte.
                        let start = (address - segment_len) as usize;
                        for i in start..start + left {
                            let byte = target[i];
                            target.push(byte);
                        }
                    }
                }
            }
        }
    }
    if target.len() != window.target_len || !data.is_empty() || !addr.is_empty() {
        return Err(corrupt("a window's instructions do not match its sections"));
    }
    Ok(())
}

fn read_source(source: Option<&File>, buf: &mut [u8], offset: u64) -> Result<()> {
    let source = source.ok_or_else(|| corrupt("a copy from a window without a source"))?;
    let mut done = 0;
    while done < buf.len() {
        match read_at(source, &mut buf[done..], offset + done as u64)? {
            0 => {
                return Err(Error::WrongSource(
                    "it is shorter than the patch expects".into(),
                ));
            }
            n => done += n,
        }
    }
    Ok(())
}

fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(file, buf, offset)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(file, buf, offset)
    }
}

pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    // The most bytes that cannot overflow `b` before the modulo.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_are_base_128_most_significant_first() {
        assert_eq!(read_integer(&mut &[0x7F][..]).unwrap(), 127);
        assert_eq!(read_integer(&mut &[0x81, 0x00][..]).unwrap(), 128);
        assert_eq!(
            read_integer(&mut &[0xBA, 0xEF, 0x9A, 0x15][..]).unwrap(),
            123_456_789
        );
        assert!(read_integer(&mut &[0xFF; 11][..]).is_err());
        for value in [0, 127, 128, 16383, 16384, u64::MAX >> 7] {
            let mut bytes = Vec::new();
            let mut v = value;
            bytes.push((v & 0x7F) as u8);
            while v >= 0x80 {
                v >>= 7;
                bytes.insert(0, (v & 0x7F) as u8 | 0x80);
            }
            assert_eq!(integer_len(value), bytes.len() as u64);
            assert_eq!(read_integer(&mut &bytes[..]).unwrap(), value);
        }
    }

    #[test]
    fn the_code_table_is_rfc_3284s() {
        assert_eq!(CODE_TABLE[0][0], (RUN, 0, 0));
        assert_eq!(CODE_TABLE[18][0], (ADD, 17, 0));
        assert_eq!(CODE_TABLE[19][0], (COPY, 0, 0));
        assert_eq!(CODE_TABLE[162][0], (COPY, 18, 8));
        assert_eq!(CODE_TABLE[163], [(ADD, 1, 0), (COPY, 4, 0)]);
        assert_eq!(CODE_TABLE[235], [(ADD, 1, 0), (COPY, 4, 6)]);
        assert_eq!(CODE_TABLE[255], [(COPY, 4, 8), (ADD, 1, 0)]);
    }

    #[test]
    fn adler32_matches_its_reference() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}
