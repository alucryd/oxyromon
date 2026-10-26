//! A streaming writer for ZArchive, the format of Cemu's WUA files.
//!
//! File data is concatenated into one stream cut into 64 KiB blocks, each
//! compressed with zstd unless that doesn't shrink it. Then come, all
//! big-endian: an offset record per 16 blocks, the name table, the file tree
//! in breadth-first order with each directory's children sorted
//! case-insensitively, two empty metadata sections, and a footer holding the
//! sections, a SHA-256 of the whole file with the footer's hash zeroed, the
//! total size, the version and the magic.

use std::collections::HashMap;
use std::io::Write;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const BLOCK_SIZE: usize = 64 * 1024;
const BLOCKS_PER_RECORD: usize = 16;
const ROOT_NAME: u32 = 0x7FFF_FFFF;
const VERSION: u32 = 0x61BF_3A01;
const MAGIC: u32 = 0x169F_52D6;
/// The level Cemu's ZArchive writes at.
const LEVEL: i32 = 6;

enum Node {
    File { name: u32, offset: u64, size: u64 },
    Dir { name: u32, children: Vec<usize> },
}

/// Write a ZArchive to `W`: [`Writer::dir`] and [`Writer::file`] build the
/// tree, [`Writer::append`] fills the last file, and [`Writer::finish`] writes
/// the tables.
pub struct Writer<W: Write> {
    output: W,
    hasher: Sha256,
    written: u64,
    /// Uncompressed bytes appended so far.
    input: u64,
    block: Vec<u8>,
    pending: Vec<Vec<u8>>,
    records: Vec<(u64, [u16; BLOCKS_PER_RECORD])>,
    blocks: usize,
    nodes: Vec<Node>,
    names: Vec<Vec<u8>>,
    name_index: HashMap<Vec<u8>, u32>,
    current: Option<usize>,
}

/// The root directory's node.
pub const ROOT: usize = 0;

impl<W: Write> Writer<W> {
    pub fn new(output: W) -> Writer<W> {
        Writer {
            output,
            hasher: Sha256::new(),
            written: 0,
            input: 0,
            block: Vec::with_capacity(BLOCK_SIZE),
            pending: Vec::new(),
            records: Vec::new(),
            blocks: 0,
            nodes: vec![Node::Dir {
                name: ROOT_NAME,
                children: Vec::new(),
            }],
            names: Vec::new(),
            name_index: HashMap::new(),
            current: None,
        }
    }

    /// Add directory `name` to directory `parent`, returning its node.
    pub fn dir(&mut self, parent: usize, name: &[u8]) -> Result<usize> {
        let name = self.name(name)?;
        self.add(
            parent,
            Node::Dir {
                name,
                children: Vec::new(),
            },
        )
    }

    /// Add file `name` to directory `parent`, and make it the one
    /// [`Writer::append`] fills.
    pub fn file(&mut self, parent: usize, name: &[u8]) -> Result<()> {
        let name = self.name(name)?;
        let node = self.add(
            parent,
            Node::File {
                name,
                offset: self.input,
                size: 0,
            },
        )?;
        self.current = Some(node);
        Ok(())
    }

    /// Append `data` to the last file added.
    pub fn append(&mut self, mut data: &[u8]) -> Result<()> {
        let Some(Node::File { size, .. }) = self.current.map(|i| &mut self.nodes[i]) else {
            return Err(Error::Unsupported("appending outside a file".into()));
        };
        *size += data.len() as u64;
        self.input += data.len() as u64;
        while !data.is_empty() {
            let take = (BLOCK_SIZE - self.block.len()).min(data.len());
            self.block.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.block.len() == BLOCK_SIZE {
                let block = std::mem::replace(&mut self.block, Vec::with_capacity(BLOCK_SIZE));
                self.queue(block)?;
            }
        }
        Ok(())
    }

    /// Write the last block and the tables, and hand back the output.
    pub fn finish(mut self) -> Result<W> {
        // Cemu's reader wants at least one block.
        if !self.block.is_empty() || self.blocks + self.pending.len() == 0 {
            let mut block = std::mem::take(&mut self.block);
            block.resize(BLOCK_SIZE, 0);
            self.pending.push(block);
        }
        self.flush_blocks()?;
        let mut sections = [(0, 0); 6];
        sections[0] = (0, self.written);
        let padding = self.written.next_multiple_of(8) - self.written;
        self.write(&vec![0; padding as usize])?;

        let records: Vec<u8> = self
            .records
            .iter()
            .flat_map(|(base, sizes)| {
                base.to_be_bytes()
                    .into_iter()
                    .chain(sizes.iter().flat_map(|s| s.to_be_bytes()))
            })
            .collect();
        sections[1] = self.section(&records)?;

        let mut name_offsets = Vec::with_capacity(self.names.len());
        let mut names = Vec::new();
        for name in &self.names {
            name_offsets.push(names.len() as u32);
            if name.len() < 0x80 {
                names.push(name.len() as u8);
            } else {
                names.extend([(name.len() as u8 & 0x7F) | 0x80, (name.len() >> 7) as u8]);
            }
            names.extend_from_slice(name);
        }
        if names.len() > ROOT_NAME as usize {
            return Err(Error::Unsupported("too many names for a ZArchive".into()));
        }
        sections[2] = self.section(&names)?;

        let tree = self.tree(&name_offsets)?;
        sections[3] = self.section(&tree)?;
        sections[4] = (self.written, 0);
        sections[5] = (self.written, 0);

        let mut footer = Vec::with_capacity(144);
        for (offset, size) in sections {
            footer.extend(offset.to_be_bytes());
            footer.extend(size.to_be_bytes());
        }
        let hash_at = footer.len();
        footer.extend([0; 32]);
        footer.extend((self.written + 144).to_be_bytes());
        footer.extend(VERSION.to_be_bytes());
        footer.extend(MAGIC.to_be_bytes());
        self.hasher.update(&footer);
        let hash = std::mem::take(&mut self.hasher).finalize();
        footer[hash_at..hash_at + 32].copy_from_slice(&hash);
        self.output.write_all(&footer)?;
        self.output.flush()?;
        Ok(self.output)
    }

    fn name(&mut self, name: &[u8]) -> Result<u32> {
        if name.is_empty() || name.len() > 0x7FFF {
            return Err(Error::Unsupported(format!(
                "a ZArchive name of {} bytes",
                name.len()
            )));
        }
        if let Some(&index) = self.name_index.get(name) {
            return Ok(index);
        }
        let index = self.names.len() as u32;
        self.names.push(name.to_vec());
        self.name_index.insert(name.to_vec(), index);
        Ok(index)
    }

    fn add(&mut self, parent: usize, node: Node) -> Result<usize> {
        self.current = None;
        let index = self.nodes.len();
        let Node::Dir { children, .. } = &mut self.nodes[parent] else {
            return Err(Error::Unsupported("adding to a file".into()));
        };
        children.push(index);
        self.nodes.push(node);
        Ok(index)
    }

    /// Entries in breadth-first order, each directory's children sorted and
    /// contiguous.
    fn tree(&self, name_offsets: &[u32]) -> Result<Vec<u8>> {
        let name_of = |node: &Node| match node {
            Node::File { name, .. } | Node::Dir { name, .. } => *name,
        };
        let lower =
            |index: usize| self.names[name_of(&self.nodes[index]) as usize].to_ascii_lowercase();
        let mut order = vec![ROOT];
        let mut first_child = vec![0u32; self.nodes.len()];
        let mut i = 0;
        while i < order.len() {
            if let Node::Dir { children, .. } = &self.nodes[order[i]] {
                let mut children = children.clone();
                children.sort_by_cached_key(|&c| lower(c));
                first_child[order[i]] = order.len() as u32;
                order.extend(children);
            }
            i += 1;
        }
        let mut tree = Vec::with_capacity(order.len() * 16);
        for index in order {
            let node = &self.nodes[index];
            let name = match name_of(node) {
                ROOT_NAME => ROOT_NAME,
                name => name_offsets[name as usize],
            };
            let words = match node {
                Node::File { offset, size, .. } => {
                    if *offset >> 48 != 0 || *size >> 48 != 0 {
                        return Err(Error::Unsupported("a file too large for a ZArchive".into()));
                    }
                    [
                        name | 0x8000_0000,
                        *offset as u32,
                        *size as u32,
                        ((offset >> 32) as u32 & 0xFFFF) | ((size >> 16) as u32 & 0xFFFF_0000),
                    ]
                }
                Node::Dir { children, .. } => [name, first_child[index], children.len() as u32, 0],
            };
            tree.extend(words.iter().flat_map(|w| w.to_be_bytes()));
        }
        Ok(tree)
    }

    fn queue(&mut self, block: Vec<u8>) -> Result<()> {
        self.pending.push(block);
        if self.pending.len() >= 2 * rayon::current_num_threads() {
            self.flush_blocks()?;
        }
        Ok(())
    }

    /// Compress the pending blocks in parallel, and write them in order.
    fn flush_blocks(&mut self) -> Result<()> {
        let compressed = std::mem::take(&mut self.pending)
            .into_par_iter()
            .map(|block| {
                let compressed = zstd::bulk::compress(&block, LEVEL)?;
                Ok(if compressed.len() < BLOCK_SIZE {
                    compressed
                } else {
                    block
                })
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        for block in compressed {
            let sub = self.blocks % BLOCKS_PER_RECORD;
            if sub == 0 {
                self.records.push((self.written, [0; BLOCKS_PER_RECORD]));
            }
            self.records.last_mut().unwrap().1[sub] = (block.len() - 1) as u16;
            self.blocks += 1;
            self.write(&block)?;
        }
        Ok(())
    }

    fn section(&mut self, bytes: &[u8]) -> Result<(u64, u64)> {
        let offset = self.written;
        self.write(bytes)?;
        Ok((offset, bytes.len() as u64))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.output.write_all(bytes)?;
        self.hasher.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }
}

/// A ZArchive, read: its tree, and its files' bytes, decompressed a block at
/// a time.
pub struct Reader {
    file: std::fs::File,
    records: Vec<(u64, [u16; BLOCKS_PER_RECORD])>,
    names: Vec<u8>,
    /// Each node: its name offset, and for a file its offset and size, for a
    /// directory its first child and child count.
    nodes: Vec<(u32, bool, u64, u64)>,
    cache: Option<(u64, Vec<u8>)>,
}

/// A node of the read tree; the root is node 0.
pub type NodeId = usize;

impl Reader {
    pub fn open(path: &std::path::Path) -> Result<Reader> {
        use crate::io::read_exact_at;
        let file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        let corrupt = |what: &str| Error::Corrupt(format!("the ZArchive's {what}"));
        if size < 144 {
            return Err(corrupt("footer is missing"));
        }
        let mut footer = [0; 144];
        read_exact_at(&file, &mut footer, size - 144)?;
        let u64_at = |b: &[u8], at: usize| u64::from_be_bytes(b[at..at + 8].try_into().unwrap());
        let u32_at = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
        if u32_at(&footer, 140) != MAGIC || u32_at(&footer, 136) != VERSION {
            return Err(Error::BadMagic {
                expected: format!("{MAGIC:#010x}"),
                found: format!("{:#010x}", u32_at(&footer, 140)),
            });
        }
        let section = |i: usize| -> Result<Vec<u8>> {
            let (offset, len) = (u64_at(&footer, i * 16), u64_at(&footer, i * 16 + 8));
            if offset.checked_add(len).is_none_or(|end| end > size) {
                return Err(corrupt("sections are out of bounds"));
            }
            let mut bytes = vec![0; len as usize];
            read_exact_at(&file, &mut bytes, offset)?;
            Ok(bytes)
        };
        let records = section(1)?
            .as_chunks::<40>()
            .0
            .iter()
            .map(|r| {
                let mut sizes = [0; BLOCKS_PER_RECORD];
                for (size, b) in sizes.iter_mut().zip(r[8..].as_chunks::<2>().0) {
                    *size = u16::from_be_bytes(*b);
                }
                (u64::from_be_bytes(r[..8].try_into().unwrap()), sizes)
            })
            .collect();
        let names = section(2)?;
        let nodes = section(3)?
            .as_chunks::<16>()
            .0
            .iter()
            .map(|e| {
                let (name, a, b, c) = (u32_at(e, 0), u32_at(e, 4), u32_at(e, 8), u32_at(e, 12));
                let is_file = name & 0x8000_0000 != 0;
                match is_file {
                    true => (
                        name & ROOT_NAME,
                        true,
                        u64::from(a) | (u64::from(c & 0xFFFF) << 32),
                        u64::from(b) | (u64::from(c & 0xFFFF_0000) << 16),
                    ),
                    false => (name, false, u64::from(a), u64::from(b)),
                }
            })
            .collect();
        Ok(Reader {
            file,
            records,
            names,
            nodes,
            cache: None,
        })
    }

    fn name(&self, node: NodeId) -> &[u8] {
        let at = self.nodes[node].0 as usize;
        let Some(&first) = self.names.get(at) else {
            return &[];
        };
        let (len, at) = match first & 0x80 {
            0 => (first as usize, at + 1),
            _ => (
                (first as usize & 0x7F) | (*self.names.get(at + 1).unwrap_or(&0) as usize) << 7,
                at + 2,
            ),
        };
        self.names.get(at..at + len).unwrap_or_default()
    }

    /// The children of directory `node`, with their names.
    pub fn children(&self, node: NodeId) -> Vec<(String, NodeId)> {
        match self.nodes.get(node) {
            Some(&(_, false, first, count)) => (first..first + count)
                .filter(|&child| (child as usize) < self.nodes.len())
                .map(|child| {
                    let child = child as usize;
                    (
                        String::from_utf8_lossy(self.name(child)).into_owned(),
                        child,
                    )
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The node at `path`, `/`-separated and case-insensitive, under `from`.
    pub fn lookup(&self, from: NodeId, path: &str) -> Option<NodeId> {
        path.split('/')
            .filter(|c| !c.is_empty())
            .try_fold(from, |node, component| {
                self.children(node)
                    .into_iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(component))
                    .map(|(_, child)| child)
            })
    }

    /// File `node`'s offset in the data stream and its size.
    pub fn file(&self, node: NodeId) -> Option<(u64, u64)> {
        match self.nodes.get(node) {
            Some(&(_, true, offset, size)) => Some((offset, size)),
            _ => None,
        }
    }

    /// Fill `buf` from the data stream at `offset`.
    pub fn read(&mut self, mut offset: u64, buf: &mut [u8]) -> Result<()> {
        let mut done = 0;
        while done < buf.len() {
            let index = offset / BLOCK_SIZE as u64;
            let block = self.block(index)?;
            let within = (offset % BLOCK_SIZE as u64) as usize;
            let take = (BLOCK_SIZE - within).min(buf.len() - done);
            buf[done..done + take].copy_from_slice(&block[within..within + take]);
            done += take;
            offset += take as u64;
        }
        Ok(())
    }

    /// Read a whole file.
    pub fn read_file(&mut self, node: NodeId) -> Result<Vec<u8>> {
        let (offset, size) = self
            .file(node)
            .ok_or_else(|| Error::Corrupt("not a file in the ZArchive".into()))?;
        let mut bytes = vec![0; size as usize];
        self.read(offset, &mut bytes)?;
        Ok(bytes)
    }

    fn block(&mut self, index: u64) -> Result<&[u8]> {
        if self
            .cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != index)
        {
            let (base, sizes) = *self
                .records
                .get((index / BLOCKS_PER_RECORD as u64) as usize)
                .ok_or_else(|| Error::Corrupt("a block past the ZArchive's end".into()))?;
            let sub = (index % BLOCKS_PER_RECORD as u64) as usize;
            let offset = base + sizes[..sub].iter().map(|&s| u64::from(s) + 1).sum::<u64>();
            let len = sizes[sub] as usize + 1;
            let mut stored = vec![0; len];
            crate::io::read_exact_at(&self.file, &mut stored, offset)?;
            let block = match len == BLOCK_SIZE {
                true => stored,
                false => zstd::bulk::decompress(&stored, BLOCK_SIZE)?,
            };
            if block.len() != BLOCK_SIZE {
                return Err(Error::Corrupt("a ZArchive block is cut short".into()));
            }
            self.cache = Some((index, block));
        }
        Ok(&self.cache.as_ref().unwrap().1)
    }
}
