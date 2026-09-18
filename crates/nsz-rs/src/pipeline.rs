//! NSP <-> NSZ pipelines. Both stream one PFS0 member at a time straight into
//! the output file and back-patch the PFS0 header at the end, so memory use is
//! independent of the dump size. Mirrors `nsz/Decompressor.py` and
//! `nsz/SolidCompressor.py` / `nsz/BlockCompressor.py`.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::compress::{block_compress_ncz, solid_compress_ncz};
use crate::crypto::ctr;
use crate::decompress::decompress_ncz;
use crate::error::{Error, Result};
use crate::format::cnmt::Cnmt;
use crate::format::nca::{self, NcaHeader, HEADER_ENCRYPTED_SIZE};
use crate::format::ncz::{Section, INCOMPRESSIBLE_HEADER_SIZE};
use crate::format::pfs0::{self, Pfs0Entry, Pfs0Reader};
use crate::keys::Keys;

/// Rights id -> encrypted title key, as found in the container's tickets.
pub type TitleKeys = HashMap<[u8; 16], [u8; 16]>;

/// Compression settings.
#[derive(Debug, Clone, Copy)]
pub struct Compression {
    /// zstd level (nsz defaults to 18).
    pub level: i32,
    /// zstd long-distance matching.
    pub ldm: bool,
    /// `None` for a solid stream, `Some(exp)` for 2^exp-byte blocks.
    pub block_size_exponent: Option<i8>,
}

/// A seekable reader over a window `[base, base+len)` of an inner reader, so a
/// member inside a PFS0 can be treated as a standalone file.
struct SubReader<'a> {
    inner: &'a mut File,
    base: u64,
    len: u64,
    pos: u64,
}

impl<'a> SubReader<'a> {
    fn new(inner: &'a mut File, entry: &Pfs0Entry) -> Self {
        SubReader {
            inner,
            base: entry.offset,
            len: entry.size,
            pos: 0,
        }
    }
}

impl Read for SubReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.len.saturating_sub(self.pos);
        let want = (buf.len() as u64).min(remaining) as usize;
        if want == 0 {
            return Ok(0);
        }
        self.inner.seek(SeekFrom::Start(self.base + self.pos))?;
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "container truncated",
            ));
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for SubReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let new = match from {
            SeekFrom::Start(x) => Some(x),
            SeekFrom::End(x) => self.len.checked_add_signed(x),
            SeekFrom::Current(x) => self.pos.checked_add_signed(x),
        };
        self.pos =
            new.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "negative seek"))?;
        Ok(self.pos)
    }
}

/// Output of a container decompression.
#[derive(Debug, Default)]
pub struct Report {
    pub files: Vec<FileReport>,
    pub verified: usize,
    pub corrupted: usize,
}

#[derive(Debug)]
pub struct FileReport {
    pub name: String,
    pub sha256: String,
    pub verified: bool,
}

/// Decompress an NSZ file into an NSP at `output`.
///
/// - `fix_padding`: re-pad the output header to 0x20 alignment instead of
///   preserving the input's first-file offset.
/// - `verify`: check each decompressed NCA's SHA-256 against the CNMT content
///   table; a mismatch is reported, and returned as an error when `strict`.
///
/// On error the partial output file is removed.
pub fn decompress_nsz(
    input: &Path,
    output: &Path,
    keys: &Keys,
    fix_padding: bool,
    verify: bool,
    strict: bool,
) -> Result<Report> {
    let mut in_file = File::open(input)?;
    let (entries, in_header_size) = read_pfs0(&mut in_file)?;
    let content_hashes = if verify {
        let title_keys = collect_title_keys(&mut in_file, &entries)?;
        match collect_content_hashes(&mut in_file, &entries, keys, &title_keys) {
            Ok(h) => h,
            Err(e) if strict => return Err(e),
            Err(_) => HashSet::new(),
        }
    } else {
        HashSet::new()
    };

    let (header_size, string_table_size) = header_geometry(&entries, in_header_size, fix_padding);
    with_output(output, header_size, string_table_size, |out| {
        let mut report = Report::default();
        let mut laid = Vec::with_capacity(entries.len());
        for e in &entries {
            let start = out.stream_position()?;
            let mut sub = SubReader::new(&mut in_file, e);
            let (name, sha) = match e.name.strip_suffix(".ncz") {
                Some(stem) => (format!("{stem}.nca"), decompress_ncz(&mut sub, out)?.1),
                None => (e.name.clone(), copy_and_hash(&mut sub, out)?),
            };
            let verified = if verify && name.ends_with(".nca") && !name.ends_with(".cnmt.nca") {
                let ok = content_hashes.contains(&sha);
                if ok {
                    report.verified += 1;
                } else {
                    report.corrupted += 1;
                    if strict {
                        return Err(Error::Verification(format!(
                            "{name}: sha256 {sha} not found in CNMT content table"
                        )));
                    }
                }
                ok
            } else {
                false
            };
            laid.push((name.clone(), start, out.stream_position()? - start));
            report.files.push(FileReport {
                name,
                sha256: sha,
                verified,
            });
        }
        Ok((laid, report))
    })
}

/// Compress an NSP into an NSZ at `output`.
///
/// Program and PublicData NCAs whose sections tile the file are compressed to
/// `.ncz` members; everything else is copied verbatim (as nsz does). Title keys
/// for rights-managed NCAs come from the NSP's tickets, then `keys`'
/// `title.keys`. On error the partial output file is removed.
pub fn compress_nsp(
    input: &Path,
    output: &Path,
    keys: &Keys,
    compression: &Compression,
    fix_padding: bool,
) -> Result<()> {
    let mut in_file = File::open(input)?;
    let (entries, in_header_size) = read_pfs0(&mut in_file)?;
    let title_keys = collect_title_keys(&mut in_file, &entries)?;

    // `.nca` -> `.ncz` keeps name lengths, so the header size is known up front.
    let (header_size, string_table_size) = header_geometry(&entries, in_header_size, fix_padding);
    with_output(output, header_size, string_table_size, |out| {
        let mut laid = Vec::with_capacity(entries.len());
        for e in &entries {
            let start = out.stream_position()?;
            let mut sub = SubReader::new(&mut in_file, e);
            let compressed = match e.name.strip_suffix(".nca") {
                Some(stem) => compress_nca(&mut sub, e.size, keys, &title_keys, compression, out)?
                    .map(|_| format!("{stem}.ncz")),
                None => None,
            };
            let name = match compressed {
                Some(name) => name,
                None => {
                    sub.seek(SeekFrom::Start(0))?;
                    copy_and_hash(&mut sub, out)?;
                    e.name.clone()
                }
            };
            laid.push((name, start, out.stream_position()? - start));
        }
        Ok((laid, ()))
    })
}

/// Compress one NCA into an NCZ written to `out`.
///
/// Returns `Ok(None)` without writing anything when the NCA isn't eligible:
/// not a Program/PublicData NCA, too small, or its sections don't tile the file
/// (`isNcaPacked` in nsz) — compressing those would lose the bytes in between.
pub fn compress_nca<R: Read + Seek, W: Write + Seek>(
    nca: &mut R,
    nca_size: u64,
    keys: &Keys,
    title_keys: &TitleKeys,
    compression: &Compression,
    out: &mut W,
) -> Result<Option<u64>> {
    if nca_size <= INCOMPRESSIBLE_HEADER_SIZE {
        return Ok(None);
    }
    let hdr = decrypt_nca_header(nca, keys)?;
    if !matches!(hdr[0x205], nca::CONTENT_PROGRAM | nca::CONTENT_PUBLIC_DATA) {
        return Ok(None);
    }
    let header = parse_nca_header(&hdr, keys, title_keys)?;
    let mut sections = header.encryption_sections(&hdr, nca)?;
    sections.sort_by_key(|s| s.offset);
    let Some(segments) = body_segments(&sections, nca_size) else {
        return Ok(None);
    };
    let body_len = segments.iter().map(|s| s.end - s.start).sum();

    let mut raw_header = vec![0u8; INCOMPRESSIBLE_HEADER_SIZE as usize];
    nca.seek(SeekFrom::Start(0))?;
    nca.read_exact(&mut raw_header)?;
    let body = BodyReader {
        inner: nca,
        pos: segments[0].start,
        segments,
        idx: 0,
    };
    let Compression {
        level,
        ldm,
        block_size_exponent,
    } = *compression;
    let written = match block_size_exponent {
        None => solid_compress_ncz(&raw_header, &sections, body, level, ldm, out)?,
        Some(exp) => {
            block_compress_ncz(&raw_header, &sections, body, body_len, level, ldm, exp, out)?
        }
    };
    Ok(Some(written))
}

/// A contiguous byte range of the NCA body, optionally CTR-encrypted.
struct Segment {
    start: u64,
    end: u64,
    crypto: Option<([u8; 16], [u8; 16])>,
}

/// The ranges making up the NCZ body: the gap after the 0x4000 header, then
/// each section (minus any part inside the header). `None` when the sections
/// don't tile `[first section, nca_size)` exactly, since the NCZ format can't
/// represent gaps or trailing data.
fn body_segments(sections: &[Section], nca_size: u64) -> Option<Vec<Segment>> {
    let first = sections.first()?;
    let mut next = first.offset as u64;
    // The first section must reach past the verbatim header, or the
    // decompressor would replay the header bytes of the following sections.
    if next + (first.size as u64) < INCOMPRESSIBLE_HEADER_SIZE {
        return None;
    }
    let mut segments = Vec::with_capacity(sections.len() + 1);
    if next > INCOMPRESSIBLE_HEADER_SIZE {
        segments.push(Segment {
            start: INCOMPRESSIBLE_HEADER_SIZE,
            end: next,
            crypto: None,
        });
    }
    for s in sections {
        if s.offset as u64 != next || s.size <= 0 {
            return None;
        }
        let end = next + s.size as u64;
        let start = next.max(INCOMPRESSIBLE_HEADER_SIZE);
        if end > start {
            let crypto = nca::is_ctr(s.crypto_type).then_some((s.crypto_key, s.crypto_counter));
            segments.push(Segment { start, end, crypto });
        }
        next = end;
    }
    (next == nca_size).then_some(segments)
}

/// Streams the decrypted NCA body: CTR segments are decrypted, others copied.
struct BodyReader<'a, R> {
    inner: &'a mut R,
    segments: Vec<Segment>,
    idx: usize,
    pos: u64,
}

impl<R: Read + Seek> Read for BodyReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.idx < self.segments.len() && self.pos >= self.segments[self.idx].end {
            self.idx += 1;
            if let Some(s) = self.segments.get(self.idx) {
                self.pos = s.start;
            }
        }
        let Some(seg) = self.segments.get(self.idx) else {
            return Ok(0);
        };
        let n = (buf.len() as u64).min(seg.end - self.pos) as usize;
        self.inner.seek(SeekFrom::Start(self.pos))?;
        self.inner.read_exact(&mut buf[..n])?;
        if let Some((key, counter)) = &seg.crypto {
            ctr::keystream_xor(key, counter, self.pos, &mut buf[..n]);
        }
        self.pos += n as u64;
        Ok(n)
    }
}

/// Read and XTS-decrypt the first 0xC00 bytes of an NCA.
fn decrypt_nca_header<R: Read + Seek>(nca: &mut R, keys: &Keys) -> Result<Vec<u8>> {
    let mut hdr = vec![0u8; HEADER_ENCRYPTED_SIZE];
    nca.seek(SeekFrom::Start(0))?;
    nca.read_exact(&mut hdr)?;
    nca::decrypt_header_bytes(&keys.header_key()?, &mut hdr);
    Ok(hdr)
}

/// Parse a decrypted NCA header, resolving the title key of rights-managed NCAs
/// from the container's tickets, then `title.keys`.
fn parse_nca_header(hdr: &[u8], keys: &Keys, title_keys: &TitleKeys) -> Result<NcaHeader> {
    let rights_id: [u8; 16] = hdr[0x230..0x240].try_into().unwrap();
    let title_key = title_keys
        .get(&rights_id)
        .copied()
        .or_else(|| keys.title_key_by_rights(&hex::encode(rights_id)));
    NcaHeader::parse(hdr, keys, title_key.as_ref())
}

/// Parse the PFS0 header, returning its entries and header size.
fn read_pfs0(f: &mut File) -> Result<(Vec<Pfs0Entry>, u64)> {
    let reader = Pfs0Reader::new(BufReader::new(f))?;
    Ok((reader.entries, reader.header_size))
}

/// Output header size and string table size: either recomputed with 0x20
/// padding, or the input's first-file offset and string table preserved.
/// Member renames (`.nca` <-> `.ncz`) keep name lengths, so input names work.
fn header_geometry(entries: &[Pfs0Entry], in_header_size: u64, fix_padding: bool) -> (u64, usize) {
    let fixed = 0x10 + entries.len() * 0x18;
    if fix_padding {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let sts = pfs0::padded_string_table_size(&names);
        ((fixed + sts) as u64, sts)
    } else {
        let first = entries.first().map_or(in_header_size, |e| e.offset);
        (first, in_header_size as usize - fixed)
    }
}

/// Create `output`, run `write` with the file positioned after the reserved
/// header, then back-patch the PFS0 header from the `(name, offset, size)`
/// layout it returns. Removes the file if anything fails.
fn with_output<T>(
    output: &Path,
    header_size: u64,
    string_table_size: usize,
    write: impl FnOnce(&mut File) -> Result<(Vec<(String, u64, u64)>, T)>,
) -> Result<T> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(output)?;
    let result = (|| {
        out.seek(SeekFrom::Start(header_size))?;
        let (laid, value) = write(&mut out)?;
        out.seek(SeekFrom::Start(0))?;
        out.write_all(&pfs0::build_header(&laid, string_table_size))?;
        out.flush()?;
        Ok(value)
    })();
    if result.is_err() {
        drop(out);
        let _ = std::fs::remove_file(output);
    }
    result
}

/// Collect the title keys from every `.tik` member. Unparseable tickets are
/// skipped (nsz does the same).
fn collect_title_keys(f: &mut File, entries: &[Pfs0Entry]) -> Result<TitleKeys> {
    let mut title_keys = TitleKeys::new();
    for e in entries.iter().filter(|e| e.name.ends_with(".tik")) {
        let mut tik = Vec::new();
        SubReader::new(f, e).read_to_end(&mut tik)?;
        if let Some((rights_id, key)) = parse_ticket(&tik) {
            title_keys.insert(rights_id, key);
        }
    }
    Ok(title_keys)
}

/// Extract `(rights_id, encrypted title key)` from a ticket (`Fs/Ticket.py`).
fn parse_ticket(tik: &[u8]) -> Option<([u8; 16], [u8; 16])> {
    let sig_type = u32::from_le_bytes(tik.get(0..4)?.try_into().ok()?);
    let sig_size = match sig_type {
        0x010000 | 0x010003 => 0x200, // RSA-4096
        0x010001 | 0x010004 => 0x100, // RSA-2048
        0x010002 | 0x010005 => 0x3C,  // ECDSA
        _ => return None,
    };
    let data = 4 + sig_size + (0x40 - (sig_size + 4) % 0x40);
    let key = tik.get(data + 0x40..data + 0x50)?.try_into().ok()?;
    let rights_id = tik.get(data + 0x160..data + 0x170)?.try_into().ok()?;
    Some((rights_id, key))
}

/// Read the CNMT (inside the Meta NCA's first section, a PFS0) and return the
/// set of content-entry hashes (lowercase hex). Mirrors
/// `FileExistingChecks.ExtractHashes`.
fn collect_content_hashes(
    f: &mut File,
    entries: &[Pfs0Entry],
    keys: &Keys,
    title_keys: &TitleKeys,
) -> Result<HashSet<String>> {
    let entry = entries
        .iter()
        .find(|e| e.name.ends_with(".cnmt.nca") || e.name.ends_with(".cnmt.ncz"))
        .ok_or_else(|| Error::Corrupt("no cnmt member found in container".into()))?;
    let mut sub = SubReader::new(f, entry);
    let mut bytes = Vec::new();
    if entry.name.ends_with(".ncz") {
        decompress_ncz(&mut sub, &mut bytes)?;
    } else {
        sub.read_to_end(&mut bytes)?;
    }

    let mut nca = Cursor::new(&bytes);
    let hdr = decrypt_nca_header(&mut nca, keys)?;
    if hdr[0x205] != nca::CONTENT_META {
        return Err(Error::Corrupt(format!("{} is not a Meta NCA", entry.name)));
    }
    let header = parse_nca_header(&hdr, keys, title_keys)?;
    let st = header.section_tables[0];
    let mut section = bytes
        .get(st.offset as usize..st.end_offset as usize)
        .ok_or_else(|| Error::Corrupt("cnmt section beyond NCA".into()))?
        .to_vec();
    let fs_hdr = &hdr[nca::fs_header_offset(0)..];
    if nca::is_ctr(fs_hdr[0x4] as i64) {
        let counter = nca::section_counter(&hdr, 0);
        ctr::keystream_xor(&header.title_key_dec, &counter, st.offset, &mut section);
    }

    // The section holds a PFS0 at the superblock's offset, containing `*.cnmt`.
    let pfs0_offset = u64::from_le_bytes(fs_hdr[0x40..0x48].try_into().unwrap()) as usize;
    let pfs0 = section
        .get(pfs0_offset..)
        .ok_or_else(|| Error::Corrupt("cnmt PFS0 offset beyond section".into()))?;
    let mut reader = Pfs0Reader::new(Cursor::new(pfs0))?;
    let cnmt_entry = reader
        .entries
        .iter()
        .find(|e| e.name.ends_with(".cnmt"))
        .cloned()
        .ok_or_else(|| Error::Corrupt("no .cnmt file in Meta NCA".into()))?;
    let cnmt = Cnmt::parse(&reader.read_file(&cnmt_entry)?)?;
    Ok(cnmt
        .content_entries
        .iter()
        .map(|c| hex::encode(c.hash))
        .collect())
}

/// Copy a whole member to `out`, returning its hex SHA-256.
fn copy_and_hash<R: Read, W: Write>(r: &mut R, out: &mut W) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 0x100000];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    Ok(hex::encode(hasher.finalize()))
}
