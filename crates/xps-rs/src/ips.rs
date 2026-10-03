//! IPS: records of bytes to write at 24-bit offsets, run-length encoded or not,
//! ended by `EOF` and optionally a 24-bit length to truncate to.

use crate::error::{Error, Result};
use crate::{Warning, read_at};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const EOF: u32 = 0x45_4F_46;

enum Record<'a> {
    Bytes(&'a [u8]),
    Run(usize, u8),
}

pub struct Parsed<'a> {
    records: Vec<(u64, Record<'a>)>,
    /// How far the records reach.
    end: u64,
    truncate: Option<u64>,
}

fn broken() -> Error {
    Error::Corrupt("the IPS patch is broken".into())
}

/// Check the whole patch before anything is written, as Flips does.
pub fn parse(patch: &[u8]) -> Result<Parsed<'_>> {
    if patch.len() < 8 {
        return Err(broken());
    }
    let mut rest = &patch[5..];
    let mut take = |len: usize| -> Result<&[u8]> {
        if rest.len() < len {
            return Err(broken());
        }
        let (bytes, tail) = rest.split_at(len);
        rest = tail;
        Ok(bytes)
    };
    let be = |bytes: &[u8]| {
        bytes
            .iter()
            .fold(0u32, |value, &byte| value << 8 | u32::from(byte))
    };

    let mut records = Vec::new();
    let mut end = 0;
    loop {
        let offset = be(take(3)?);
        if offset == EOF {
            break;
        }
        let record = match be(take(2)?) as usize {
            0 => {
                // Flips refuses an empty run, unsure what one would mean.
                let size = be(take(2)?) as usize;
                if size == 0 {
                    return Err(broken());
                }
                Record::Run(size, take(1)?[0])
            }
            size => Record::Bytes(take(size)?),
        };
        let size = match record {
            Record::Bytes(bytes) => bytes.len(),
            Record::Run(size, _) => size,
        };
        end = end.max(u64::from(offset) + size as u64);
        records.push((u64::from(offset), record));
    }
    // What follows `EOF`: nothing, or a length to truncate to.
    let truncate = match take(3) {
        Ok(bytes) => Some(u64::from(be(bytes))),
        Err(_) => None,
    };
    // Anything left but exactly those three bytes, or nothing, is not IPS.
    if take(1).is_ok() {
        return Err(broken());
    }
    Ok(Parsed {
        records,
        end,
        truncate,
    })
}

/// Apply to `source`, into `output`, a new file open to read and write.
pub fn apply(parsed: Parsed, source: &mut File, output: &mut File) -> Result<Option<Warning>> {
    let source_len = source.metadata()?.len();

    // Flips clamps the source's length between what the records need and the
    // truncation, when there is one.
    let minimum = parsed
        .truncate
        .map_or(parsed.end, |truncate| parsed.end.min(truncate));
    let len = source_len.clamp(minimum, parsed.truncate.unwrap_or(u64::MAX));
    let mut warning = if parsed
        .truncate
        .is_some_and(|truncate| parsed.end > truncate)
    {
        Some(Warning::Scrambled)
    } else {
        None
    };
    // Truncating what does not need it: this was made for a longer file.
    if parsed
        .truncate
        .is_some_and(|truncate| source_len <= truncate)
    {
        warning = Some(Warning::NotThis);
    }

    std::io::copy(&mut Read::take(&mut *source, len), output)?;
    output.set_len(len)?;

    let mut changed = len != source_len;
    let mut current = Vec::new();
    for (offset, record) in &parsed.records {
        let bytes = match record {
            Record::Bytes(bytes) => bytes.to_vec(),
            Record::Run(size, byte) => vec![*byte; *size],
        };
        if !changed {
            current.resize(bytes.len(), 0);
            let read = read_at(output, &mut current, *offset)?;
            changed = current[..read] != bytes[..read] || read < bytes.len();
        }
        output.seek(SeekFrom::Start(*offset))?;
        output.write_all(&bytes)?;
    }
    // Records past a truncation were written, then cut, as Flips does.
    output.set_len(len)?;
    if !changed {
        warning = Some(Warning::AlreadyApplied);
    }
    Ok(warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_runs_and_a_truncation_parse() {
        let patch = b"PATCH\x00\x00\x02\x00\x02AB\x00\x00\x10\x00\x00\x00\x03ZEOF\x00\x00\x08";
        let parsed = parse(patch).unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.end, 0x13);
        assert_eq!(parsed.truncate, Some(8));
    }

    #[test]
    fn broken_patches_are_refused() {
        for patch in [
            &b"PATCH"[..],
            b"PATCH\x00\x00\x02\x00\x05AB",
            b"PATCH\x00\x00\x02\x00\x00\x00\x00ZEOF",
            b"PATCHEOF\x00",
        ] {
            assert!(parse(patch).is_err(), "{patch:?}");
        }
    }
}
