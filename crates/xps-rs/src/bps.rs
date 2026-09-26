//! BPS: sizes, metadata, then actions that read from the source, the patch or
//! the target so far, with CRC-32s of the source, the target and the patch.

use crate::error::{Error, Result};
use crate::read_at;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};

const SOURCE_READ: u64 = 0;
const TARGET_READ: u64 = 1;
const SOURCE_COPY: u64 = 2;

const CHUNK: usize = 1 << 16;

fn broken() -> Error {
    Error::Corrupt("the BPS patch is broken".into())
}

fn read_byte<R: Read>(r: &mut R) -> Result<u8> {
    let mut byte = [0];
    r.read_exact(&mut byte).map_err(|_| broken())?;
    Ok(byte[0])
}

/// BPS's integers: base 128, least significant group first, each group past
/// the first counting from one.
fn read_number<R: Read>(r: &mut R) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = read_byte(r)?;
        let group = u64::from(byte & 0x7F) + u64::from(shift > 0);
        value = 1u64
            .checked_shl(shift)
            .and_then(|scale| group.checked_mul(scale))
            .filter(|_| shift < 64)
            .and_then(|add| value.checked_add(add))
            .ok_or_else(broken)?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// Move `base` by a signed distance: its low bit is the sign.
fn step(base: u64, encoded: u64) -> Result<u64> {
    let distance = encoded >> 1;
    match encoded & 1 {
        0 => base.checked_add(distance),
        _ => base.checked_sub(distance),
    }
    .ok_or_else(broken)
}

fn crc_of(file: &File, len: u64) -> Result<u32> {
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0; CHUNK];
    let mut at = 0;
    while at < len {
        let n = read_at(file, &mut buf[..CHUNK.min((len - at) as usize)], at)?;
        if n == 0 {
            return Err(broken());
        }
        hasher.update(&buf[..n]);
        at += n as u64;
    }
    Ok(hasher.finalize())
}

/// A patch checked against its source, ready to apply.
pub struct Bps<'a> {
    patch: std::io::Take<BufReader<&'a File>>,
    source: &'a File,
    source_len: u64,
    target_len: u64,
    target_crc: u32,
    patch_len: u64,
}

impl<'a> Bps<'a> {
    /// Check the patch against its own CRC, then the source against its size and
    /// CRC, as Flips does before writing anything.
    pub fn open(patch: &'a File, source: &'a File) -> Result<Self> {
        let patch_len = patch.metadata()?.len();
        if patch_len < 4 + 3 + 12 {
            return Err(broken());
        }
        let mut footer = [0; 12];
        read_at(patch, &mut footer, patch_len - 12)?;
        let crc = |at: usize| u32::from_le_bytes(footer[at..at + 4].try_into().unwrap());
        let (source_crc, target_crc, patch_crc) = (crc(0), crc(4), crc(8));
        if crc_of(patch, patch_len - 4)? != patch_crc {
            return Err(Error::Corrupt(
                "the BPS patch fails its own checksum".into(),
            ));
        }

        let mut reader = BufReader::new(patch);
        reader.seek(SeekFrom::Start(0))?;
        let mut reader = reader.take(patch_len - 12);
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).map_err(|_| broken())?;
        if &magic != b"BPS1" {
            return Err(broken());
        }
        let expected_len = read_number(&mut reader)?;
        let target_len = read_number(&mut reader)?;

        let source_len = source.metadata()?.len();
        let actual_crc = crc_of(source, source_len)?;
        if source_len != expected_len || actual_crc != source_crc {
            return Err(Error::WrongSource(
                if source_len == target_len && actual_crc == target_crc {
                    "it is what the patch produces already".into()
                } else if source_len != expected_len {
                    format!("expected {expected_len} bytes, got {source_len}")
                } else {
                    format!("expected checksum {source_crc:08X}, got {actual_crc:08X}")
                },
            ));
        }

        let metadata = read_number(&mut reader)?;
        if std::io::copy(&mut (&mut reader).take(metadata), &mut std::io::sink())? != metadata {
            return Err(broken());
        }
        Ok(Bps {
            patch: reader,
            source,
            source_len,
            target_len,
            target_crc,
            patch_len,
        })
    }

    pub fn apply(mut self, output: &mut File, progress: &mut dyn FnMut(u64)) -> Result<()> {
        let mut target = Target::new(output);
        let mut buf = vec![0; CHUNK];
        let (mut source_at, mut target_at) = (0u64, 0u64);
        // Measured from the start of the patch, header included.
        let consumed = |patch: &std::io::Take<_>| self.patch_len - 12 - patch.limit();
        let mut reported = 0;

        while self.patch.limit() > 0 {
            let action = read_number(&mut self.patch)?;
            let len = (action >> 2) + 1;
            if len > self.target_len - target.len {
                return Err(broken());
            }
            match action & 3 {
                SOURCE_READ => {
                    let at = target.len;
                    self.copy_source(&mut target, &mut buf, at, len)?;
                }
                TARGET_READ => {
                    let mut left = len;
                    while left > 0 {
                        let n = (left as usize).min(CHUNK);
                        self.patch.read_exact(&mut buf[..n]).map_err(|_| broken())?;
                        target.push(&buf[..n])?;
                        left -= n as u64;
                    }
                }
                SOURCE_COPY => {
                    source_at = step(source_at, read_number(&mut self.patch)?)?;
                    self.copy_source(&mut target, &mut buf, source_at, len)?;
                    source_at += len;
                }
                _ => {
                    target_at = step(target_at, read_number(&mut self.patch)?)?;
                    if target_at >= target.len {
                        return Err(broken());
                    }
                    target.copy_within(target_at, len, &mut buf)?;
                    target_at += len;
                }
            }
            let now = consumed(&self.patch);
            if now - reported >= CHUNK as u64 {
                progress(now - reported);
                reported = now;
            }
        }
        if target.len != self.target_len {
            return Err(broken());
        }
        let crc = target.finish()?;
        progress(self.patch_len - reported);
        if crc != self.target_crc {
            return Err(Error::Corrupt(
                "the output does not match the patch's checksum".into(),
            ));
        }
        Ok(())
    }

    fn copy_source(&self, target: &mut Target, buf: &mut [u8], at: u64, len: u64) -> Result<()> {
        if at.checked_add(len).is_none_or(|end| end > self.source_len) {
            return Err(broken());
        }
        let mut done = 0;
        while done < len {
            let n = ((len - done) as usize).min(buf.len());
            let read = read_at(self.source, &mut buf[..n], at + done)?;
            if read == 0 {
                return Err(broken());
            }
            target.push(&buf[..read])?;
            done += read as u64;
        }
        Ok(())
    }
}

/// The output as it is written: flushed to disk in chunks, with what is not yet
/// flushed kept to copy from.
struct Target<'a> {
    file: &'a mut File,
    pending: Vec<u8>,
    flushed: u64,
    len: u64,
    crc: crc32fast::Hasher,
}

impl<'a> Target<'a> {
    fn new(file: &'a mut File) -> Self {
        Target {
            file,
            pending: Vec::with_capacity(CHUNK * 16),
            flushed: 0,
            len: 0,
            crc: crc32fast::Hasher::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.crc.update(bytes);
        self.pending.extend_from_slice(bytes);
        self.len += bytes.len() as u64;
        if self.pending.len() >= CHUNK * 16 {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        // Reading back may have moved the cursor, on Windows.
        self.file.seek(SeekFrom::Start(self.flushed))?;
        self.file.write_all(&self.pending)?;
        self.flushed += self.pending.len() as u64;
        self.pending.clear();
        Ok(())
    }

    /// Append `len` bytes from `from` on. What overlaps the bytes being written
    /// repeats them, so no more is taken at once than is already there.
    fn copy_within(&mut self, mut from: u64, mut len: u64, buf: &mut [u8]) -> Result<()> {
        while len > 0 {
            let n = if from >= self.flushed {
                let start = (from - self.flushed) as usize;
                let n = ((len as usize).min(self.pending.len() - start)).min(buf.len());
                buf[..n].copy_from_slice(&self.pending[start..start + n]);
                n
            } else {
                let n = ((len as usize).min((self.flushed - from) as usize)).min(buf.len());
                let read = read_at(self.file, &mut buf[..n], from)?;
                if read == 0 {
                    return Err(broken());
                }
                read
            };
            self.push(&buf[..n])?;
            from += n as u64;
            len -= n as u64;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<u32> {
        self.flush()?;
        self.file.flush()?;
        Ok(self.crc.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_count_each_group_from_one() {
        assert_eq!(read_number(&mut &[0x80][..]).unwrap(), 0);
        assert_eq!(read_number(&mut &[0xFF][..]).unwrap(), 127);
        assert_eq!(read_number(&mut &[0x00, 0x80][..]).unwrap(), 128);
        assert_eq!(read_number(&mut &[0x7F, 0x80][..]).unwrap(), 255);
        assert!(read_number(&mut &[0x7F; 12][..]).is_err());
    }

    #[test]
    fn distances_carry_their_sign_in_the_low_bit() {
        assert_eq!(step(10, 4).unwrap(), 12);
        assert_eq!(step(10, 5).unwrap(), 8);
        assert!(step(1, 5).is_err());
    }
}
