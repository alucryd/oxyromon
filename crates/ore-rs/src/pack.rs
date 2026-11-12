//! Stored chunks gathered into groups, each compressed on its own, and
//! groups read back.

use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;

use rayon::prelude::*;

use super::codec;
use super::error::{Error, Result};
use super::format::*;
use super::io;

/// A group compressed, ready to be written behind its envelope.
pub struct Packed {
    pub group: Group,
    pub stored: Vec<u8>,
    /// The CRC-32C of what it decodes to.
    pub crc: u32,
}

/// Chunks gathered into groups, in the order they come, the groups
/// compressed several at once.
pub struct Packer {
    pub codec: Codec,
    group_size: usize,
    group: Vec<u8>,
    chunks: u64,
    /// Groups closed but not yet compressed.
    closed: Vec<(Vec<u8>, u64)>,
}

impl Packer {
    pub fn new(codec: Codec, group_size: usize) -> Self {
        Self {
            codec,
            group_size,
            group: Vec::with_capacity(group_size),
            chunks: 0,
            closed: Vec::new(),
        }
    }

    /// Add a chunk: which group it lands in, counted from the first
    /// [`Self::take`] has yet to give, and where in it.
    pub fn push(&mut self, bytes: &[u8]) -> (usize, u32) {
        if self.group.len() + bytes.len() > self.group_size {
            self.close();
        }
        let offset = self.group.len() as u32;
        self.group.extend_from_slice(bytes);
        self.chunks += 1;
        (self.closed.len(), offset)
    }

    fn close(&mut self) {
        if self.chunks > 0 {
            let group = std::mem::replace(&mut self.group, Vec::with_capacity(self.group_size));
            self.closed.push((group, self.chunks));
            self.chunks = 0;
        }
    }

    /// The groups closed, compressed, in order: once there are enough to
    /// keep every core busy, or with `all`, every one, the last closed as
    /// it is.
    pub fn take(&mut self, all: bool) -> Result<Vec<Packed>> {
        if all {
            self.close();
        } else if self.closed.len() < rayon::current_num_threads() {
            return Ok(Vec::new());
        }
        let (codec, group_size) = (self.codec, self.group_size);
        std::mem::take(&mut self.closed)
            .into_par_iter()
            .map(|(data, chunks)| {
                let stored = codec::compress(codec, &data, group_size)?;
                Ok(Packed {
                    group: Group {
                        codec,
                        chunks,
                        logical_len: data.len() as u64,
                        stored_len: stored.len() as u64,
                    },
                    stored,
                    crc: crc32c::crc32c(&data),
                })
            })
            .collect()
    }
}

/// Read the group whose envelope starts at `offset` of `file`, decoded and
/// checked against its envelope.
pub fn read_group(file: &File, offset: u64, envelope_len: u64, group: &Group) -> Result<Vec<u8>> {
    let mut bytes = vec![0; (envelope_len + group.stored_len) as usize];
    io::read_exact_at(file, &mut bytes, offset)?;
    let (envelope, stored) = bytes.split_at(envelope_len as usize);
    let mut cursor = Cursor::new(envelope);
    if cursor.take(2)? != ENVELOPE_MAGIC
        || cursor.u8()? != KIND_SOLID
        || cursor.u8()? != group.codec.id() << 4
    {
        return Err(Error::Corrupt(format!(
            "the envelope of the group at {offset:#x}"
        )));
    }
    cursor.uvarint()?;
    cursor.uvarint()?;
    let crc = u32::from_be_bytes(cursor.take(4)?.try_into().unwrap());
    let data = codec::decompress(group.codec, stored, group.logical_len as usize)?;
    if crc32c::crc32c(&data) != crc {
        return Err(Error::Corrupt(format!(
            "the group at {offset:#x} fails its CRC"
        )));
    }
    Ok(data)
}

/// The groups last decoded.
#[derive(Default)]
pub struct Cache(HashMap<u32, Arc<Vec<u8>>>);

impl Cache {
    pub fn get(
        &mut self,
        index: u32,
        decode: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<Arc<Vec<u8>>> {
        if let Some(group) = self.0.get(&index) {
            return Ok(group.clone());
        }
        // ponytail: forget every group past a few, an LRU if dedup-heavy
        // reads ever thrash
        if self.0.len() >= 16 {
            self.0.clear();
        }
        let data = Arc::new(decode()?);
        self.0.insert(index, data.clone());
        Ok(data)
    }
}
