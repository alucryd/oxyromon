//! The CIA container: a 0x2020-byte header, then up to five sections laid out
//! by it, each started at a 64-byte boundary.
//!
//! The header carries no magic. What identifies a CIA is its declared header
//! size, its type and format version, and the arithmetic working out: every
//! section sits where the sizes before it say it must, and the file must be at
//! least as long as the last one ends. That arithmetic is reproduced here
//! byte for byte, because an extracted content's hash against a DAT is only
//! right if its offset is.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::tmd;

/// The one header size a CIA may declare.
const HEADER_SIZE: u64 = 0x2020;
/// Sections start on a 64-byte boundary...
const SECTION_ALIGNMENT: u64 = 64;
/// ...and contents, relative to the content section, on a 16-byte one.
const CONTENT_ALIGNMENT: u64 = 16;
/// The content-present bitarray covers every content index there is.
const BITARRAY_SIZE: usize = 0x2000;

/// Which CIA layout a file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiaFormat {
    /// The standard container: ticket, title metadata and content set.
    Normal,
    /// The homebrew variant (format version 0xFF): a single content, and no
    /// title metadata to describe it.
    Simple,
}

/// A section of the container, in the order they sit in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Certificates,
    Ticket,
    TitleMetaData,
    /// The contents themselves, packed one after another.
    Contents,
    Footer,
}

/// One content of the title: what the TMD's record says, plus where its bytes
/// sit in the file. They are copied out as they are; an encrypted content
/// stays encrypted, the way ctrtool's `--plain` mode leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Content {
    pub id: u32,
    pub index: u16,
    pub size: u64,
    /// The TMD marks it AES-encrypted; unpacking leaves that alone.
    pub encrypted: bool,
    /// The TMD marks it optional to install.
    pub optional: bool,
    /// The TMD's SHA-256 of the content's stored bytes.
    pub hash: [u8; 32],
    /// Offset of the content's `size` bytes within the CIA.
    pub offset: u64,
}

/// A parsed CIA: header, sections, contents. Read by [`Cia::parse`], which
/// consumes only the header and the title metadata.
#[derive(Debug, Clone)]
pub struct Cia {
    format: CiaFormat,
    title_id: u64,
    title_version: u16,
    tmd_meta_size: u32,
    sections: [Option<(u64, u64)>; 5],
    contents: Vec<Content>,
}

impl Cia {
    /// Read a CIA's header and title metadata. `progress` is called with the
    /// input bytes consumed since the last call, seek gaps included; the
    /// calls add up to just past the title metadata, the only parts read.
    pub fn parse(input: &Path, progress: &mut dyn FnMut(u64)) -> Result<Cia> {
        let mut file = File::open(input)?;
        Self::read(&mut file, progress)
    }

    /// Read from an open file. `progress` sees the header and the TMD, gaps
    /// included; `unpack` passes a sink and re-walks the whole file itself.
    pub(crate) fn read(file: &mut File, progress: &mut dyn FnMut(u64)) -> Result<Cia> {
        let length = file.metadata()?.len();
        if length < HEADER_SIZE {
            return Err(Error::Corrupt(
                "file is too small to hold a CIA header".into(),
            ));
        }

        let mut header = [0u8; HEADER_SIZE as usize];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)?;
        progress(HEADER_SIZE);

        if le32(&header, 0x00) != HEADER_SIZE as u32 {
            return Err(Error::Corrupt(format!(
                "CIA header declares a size of {}, not {HEADER_SIZE}",
                le32(&header, 0x00)
            )));
        }
        if le16(&header, 0x04) != 0 {
            return Err(Error::Unsupported(format!(
                "CIA type 0x{:04x}",
                le16(&header, 0x04)
            )));
        }
        let format = match le16(&header, 0x06) {
            0x00 => CiaFormat::Normal,
            0xFF => CiaFormat::Simple,
            other => {
                return Err(Error::Unsupported(format!(
                    "CIA format version 0x{other:04x}"
                )));
            }
        };

        // Lay the sections out the way the reference walks them: aligned up
        // from the previous one, present only when non-empty.
        let mut pos = HEADER_SIZE;
        let mut sections = [None; 5];
        for (section, size) in [
            (Section::Certificates, le32(&header, 0x08) as u64),
            (Section::Ticket, le32(&header, 0x0C) as u64),
            (Section::TitleMetaData, le32(&header, 0x10) as u64),
            (Section::Contents, le64(&header, 0x18)),
            (Section::Footer, le32(&header, 0x14) as u64),
        ] {
            if size == 0 {
                continue;
            }
            pos = align(pos, SECTION_ALIGNMENT);
            let end = pos
                .checked_add(size)
                .ok_or_else(|| Error::Corrupt("CIA section sizes overflow".into()))?;
            sections[index(section)] = Some((pos, size));
            pos = end;
        }
        if length < pos {
            return Err(Error::Corrupt(
                "file is too small for the size its header announces".into(),
            ));
        }

        let (title_id, title_version, contents) = match format {
            CiaFormat::Simple => {
                let Some((offset, size)) = sections[index(Section::Contents)] else {
                    return Err(Error::Corrupt("CIA has no contents".into()));
                };
                (
                    0,
                    0,
                    vec![Content {
                        id: 0,
                        index: 0,
                        size,
                        encrypted: false,
                        optional: false,
                        hash: [0; 32],
                        offset,
                    }],
                )
            }
            CiaFormat::Normal => {
                // The reference requires all three; a container missing one
                // has nothing importable in it.
                for (section, name) in [
                    (Section::Ticket, "ticket"),
                    (Section::TitleMetaData, "title metadata"),
                    (Section::Contents, "contents"),
                ] {
                    if sections[index(section)].is_none() {
                        return Err(Error::Corrupt(format!("CIA has no {name}")));
                    }
                }

                let (parsed, tmd_end) =
                    read_tmd(file, sections[index(Section::TitleMetaData)].unwrap())?;
                // The header was already reported: this covers the gap up to
                // the TMD plus everything it read.
                progress(tmd_end.saturating_sub(HEADER_SIZE));

                // Map the records onto the packed content section: the
                // header's bitarray says which are present, and each present
                // one is aligned up from the previous. Bit N sits in byte
                // N / 8 as its most significant bit, the way the reference's
                // bitarray reads it — retail CIAs spell their first three
                // contents as `E0`.
                let bitarray = &header[0x20..0x20 + BITARRAY_SIZE];
                let (contents_at, contents_size) = sections[index(Section::Contents)].unwrap();
                let mut running = 0u64;
                let mut contents: Vec<Content> = Vec::with_capacity(parsed.records.len());
                for record in parsed.records {
                    let bit = 1 << (7 - record.index as usize % 8);
                    if bitarray[record.index as usize / 8] & bit == 0 {
                        continue;
                    }
                    let offset = contents_at + align(running, CONTENT_ALIGNMENT);
                    running = running.saturating_add(record.size);
                    if offset.saturating_add(record.size) > contents_at + contents_size {
                        return Err(Error::Corrupt(format!(
                            "content {:08x} extends past the CIA's data",
                            record.id
                        )));
                    }
                    let content = Content {
                        id: record.id,
                        index: record.index,
                        size: record.size,
                        encrypted: record.encrypted,
                        optional: record.optional,
                        hash: record.hash,
                        offset,
                    };
                    // A repeated index means a corrupt TMD; the reference's
                    // map keeps the last record written under it, so do we.
                    match contents.iter().position(|c| c.index == content.index) {
                        Some(at) => contents[at] = content,
                        None => contents.push(content),
                    }
                }
                contents.sort_by_key(|content| content.index);

                (parsed.title_id, parsed.title_version, contents)
            }
        };

        Ok(Cia {
            format,
            title_id,
            title_version,
            tmd_meta_size: le32(&header, 0x10),
            sections,
            contents,
        })
    }

    /// Which layout the file uses.
    pub fn format(&self) -> CiaFormat {
        self.format
    }

    /// The title's 64-bit id, as the TMD spells it; 0 for a SimpleCIA, which
    /// carries none.
    pub fn title_id(&self) -> u64 {
        self.title_id
    }

    /// The title's update version; the TMD's, so 0 for a SimpleCIA.
    pub fn title_version(&self) -> u16 {
        self.title_version
    }

    /// The TMD's size as the CIA header announces it — the size of the `tmd`
    /// file [`crate::unpack`] writes.
    pub fn tmd_meta_size(&self) -> u32 {
        self.tmd_meta_size
    }

    /// The title's contents, in content index order.
    pub fn contents(&self) -> &[Content] {
        &self.contents
    }

    /// Where a section sits in the file: `(offset, size)` when the CIA has
    /// one. Offsets are into the parsed input.
    pub fn section(&self, section: Section) -> Option<(u64, u64)> {
        self.sections[index(section)]
    }
}

/// Read and parse the TMD section: the signature type comes first, the fixed
/// part behind it counts the records, and the records follow. Every read is
/// gated on the section's declared size, so a truncated TMD is a clean error.
/// Returns the TMD and the file position just past what was read.
fn read_tmd(file: &mut File, (at, size): (u64, u64)) -> Result<(tmd::Tmd, u64)> {
    if size < 4 {
        return Err(Error::Corrupt("TMD is too small".into()));
    }
    let mut head = [0u8; 4];
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut head)?;
    let fixed_len = tmd::fixed_len(tmd::sig_type(&head))? as u64;
    if size < fixed_len + tmd::RECORD_SIZE as u64 {
        return Err(Error::Corrupt("TMD is too small".into()));
    }

    let mut data = vec![0u8; fixed_len as usize];
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut data)?;

    let records = tmd::record_count(&data)?;
    let records_len = tmd::RECORD_SIZE
        .checked_mul(records)
        .ok_or_else(|| Error::Corrupt("TMD content group is too large".into()))?;
    if size < fixed_len + records_len as u64 {
        return Err(Error::Corrupt("TMD is too small".into()));
    }
    let mut tail = vec![0u8; records_len];
    file.read_exact(&mut tail)?;
    data.extend(tail);

    let parsed = tmd::parse(&data)?;
    Ok((parsed, at + data.len() as u64))
}

pub(crate) fn index(section: Section) -> usize {
    match section {
        Section::Certificates => 0,
        Section::Ticket => 1,
        Section::TitleMetaData => 2,
        Section::Contents => 3,
        Section::Footer => 4,
    }
}

/// Copy `at..at + size` of `file` into `output` as it was written, reporting
/// every byte — the gap from `cursor` included — to `progress`, and leave the
/// cursor just past the copy. Returns the new cursor.
pub(crate) fn copy_range(
    file: &mut File,
    cursor: u64,
    at: u64,
    size: u64,
    output: &mut File,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    file.seek(SeekFrom::Start(at))?;
    if at > cursor {
        progress(at - cursor);
    }
    let mut buffer = vec![0u8; 0x10000];
    let mut left = size;
    while left > 0 {
        let want = left.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..want])?;
        output.write_all(&buffer[..want])?;
        left -= want as u64;
        progress(want as u64);
    }
    Ok(at + size)
}

fn align(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn le16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}

fn le32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn le64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}
