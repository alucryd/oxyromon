//! ISO 9660 directory reader.
//!
//! Vendored from the `opticaldiscs` crate v0.14.0 (GPL-3.0), which is licence
//! compatible with oxyromon's GPL-3.0+: <https://github.com/danifunker/opticaldiscs-rs>
//!
//! It lives here rather than as a dependency because `opticaldiscs` depends
//! unconditionally on `nod` 1.4, which pins `liblzma-sys` 0.3, while `nod` 2.x
//! needs `liblzma-sys` 0.4. Both declare `links = "lzma"` and Cargo permits only
//! one package per `links` value, so the two cannot be resolved together —
//! and oxyromon wants `nod` 2 for native RVZ/WBFS conversion. Once `nod` is
//! optional upstream, this module goes away and the crate comes back.
//!
//! Trimmed to the directory walking `import-irds` needs: volume descriptors, the
//! Rock Ridge / Joliet / primary tree preference, and directory records. File
//! reading, El Torito, the other filesystems, and the Rock Ridge timestamp,
//! POSIX and symlink entries are all omitted. The API surface that remains is
//! named as upstream names it, so swapping back is mostly deleting this file.

use anyhow::{Result, bail};

/// Size of a cooked ISO 9660 logical sector.
pub const SECTOR_SIZE: u64 = 2048;

/// The volume descriptor set starts here (ECMA-119 §6.2.1).
const PVD_SECTOR: u64 = 16;
const PVD_TYPE: u8 = 0x01;
const SVD_TYPE: u8 = 0x02;
const VD_SET_TERMINATOR_TYPE: u8 = 0xFF;
const ISO9660_ID: &[u8; 5] = b"CD001";
const HIGH_SIERRA_ID: &[u8; 5] = b"CDROM";

/// Abstraction over whatever is holding the image bytes.
///
/// Implementations always return 2048-byte cooked sectors.
pub trait SectorReader {
    /// Read a single 2048-byte cooked sector at the given Logical Block Address.
    fn read_sector(&mut self, lba: u64) -> Result<Vec<u8>>;

    /// Read `length` bytes starting at `byte_offset` (cooked address space).
    fn read_bytes(&mut self, byte_offset: u64, length: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(length);
        let mut remaining = length;
        let mut lba = byte_offset / SECTOR_SIZE;
        let mut offset = (byte_offset % SECTOR_SIZE) as usize;

        while remaining > 0 {
            let sector = self.read_sector(lba)?;
            let available = sector.len().saturating_sub(offset);
            // Guard absent upstream: a reader returning a short sector would
            // otherwise spin here forever, since `remaining` would never shrink
            if available == 0 {
                break;
            }
            let take = remaining.min(available);
            out.extend_from_slice(&sector[offset..offset + take]);
            remaining -= take;
            lba += 1;
            offset = 0;
        }
        Ok(out)
    }
}

/// The subset of the Primary Volume Descriptor needed to walk the tree.
struct PrimaryVolumeDescriptor {
    root_directory_lba: u32,
    root_directory_size: u32,
    /// High Sierra Format puts the directory record's file flags at offset 24
    /// rather than 25, so the record parser has to be told which it is reading.
    high_sierra: bool,
}

impl PrimaryVolumeDescriptor {
    fn read_from(reader: &mut dyn SectorReader) -> Result<Self> {
        let sector = reader.read_sector(PVD_SECTOR)?;
        Self::parse(&sector)
    }

    fn parse(sector: &[u8]) -> Result<Self> {
        if sector.len() < SECTOR_SIZE as usize {
            bail!("sector too small: {} bytes", sector.len());
        }

        // High Sierra is distinguished by its `CDROM` identifier at byte 9
        // (descriptor type at byte 8), versus ISO 9660's `CD001` at byte 1
        if &sector[9..14] == HIGH_SIERRA_ID {
            return Self::parse_high_sierra(sector);
        }

        match sector[0] {
            PVD_TYPE => {}
            VD_SET_TERMINATOR_TYPE => {
                bail!("reached Volume Descriptor Set Terminator before PVD")
            }
            t => bail!("unexpected volume descriptor type 0x{t:02X} (expected 0x01)"),
        }

        if &sector[1..6] != ISO9660_ID {
            bail!("missing ISO 9660 identifier 'CD001'");
        }

        if sector[6] != 1 {
            bail!("unsupported PVD version {}", sector[6]);
        }

        // Root Directory Record is embedded at offset 156 (34 bytes)
        let rdr = &sector[156..190];
        Ok(Self {
            root_directory_lba: u32::from_le_bytes(rdr[2..6].try_into().unwrap()),
            root_directory_size: u32::from_le_bytes(rdr[10..14].try_into().unwrap()),
            high_sierra: false,
        })
    }

    /// High Sierra prepends an 8-byte logical block number to every descriptor,
    /// so every field is shifted and the root record lands at offset 180.
    fn parse_high_sierra(sector: &[u8]) -> Result<Self> {
        if sector[8] != PVD_TYPE {
            bail!(
                "High Sierra descriptor at sector 16 has type {} (expected 1)",
                sector[8]
            );
        }

        let rdr = &sector[180..214];
        Ok(Self {
            root_directory_lba: u32::from_le_bytes(rdr[2..6].try_into().unwrap()),
            root_directory_size: u32::from_le_bytes(rdr[10..14].try_into().unwrap()),
            high_sierra: true,
        })
    }
}

/// Decode a big-endian UTF-16 (UCS-2) byte slice, the encoding Joliet uses for
/// its identifiers. A trailing odd byte is ignored, invalid sequences become
/// U+FFFD.
fn decode_utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// The root of a Joliet Supplementary Volume Descriptor's directory tree.
struct JolietVolumeDescriptor {
    root_directory_lba: u32,
    root_directory_size: u32,
}

impl JolietVolumeDescriptor {
    /// Scan the volume descriptor set for a Supplementary Volume Descriptor
    /// whose escape sequences select a UCS-2 level. `None` when there is none.
    fn find(reader: &mut dyn SectorReader) -> Option<Self> {
        // Bound the scan so damaged media can't spin forever
        for i in 0..32u64 {
            let sector = match reader.read_sector(PVD_SECTOR + i) {
                Ok(sector) => sector,
                Err(_) => break,
            };
            if sector.len() < 190 || &sector[1..6] != ISO9660_ID {
                break;
            }
            if sector[0] == VD_SET_TERMINATOR_TYPE {
                break;
            }
            if sector[0] == SVD_TYPE && is_joliet_escape(&sector[88..120]) {
                let rdr = &sector[156..190];
                return Some(Self {
                    root_directory_lba: u32::from_le_bytes(rdr[2..6].try_into().unwrap()),
                    root_directory_size: u32::from_le_bytes(rdr[10..14].try_into().unwrap()),
                });
            }
        }
        None
    }
}

/// True if a descriptor's escape sequence field selects a Joliet UCS-2 level.
fn is_joliet_escape(escape: &[u8]) -> bool {
    escape
        .windows(3)
        .any(|w| w == b"%/@" || w == b"%/C" || w == b"%/E")
}

/// An entry in an ISO 9660 directory.
///
/// Files larger than 4 GiB span several extents, each of which gets its own
/// record and therefore its own `FileEntry` under the same `path`.
pub struct FileEntry {
    /// Name within the parent directory.
    pub name: String,
    /// Absolute path from the root of the volume.
    pub path: String,
    /// Size of this extent in bytes.
    pub size: u64,
    /// Location of this extent (LBA).
    pub location: u64,
    directory: bool,
}

impl FileEntry {
    /// True when this entry is a directory rather than a file.
    pub fn is_directory(&self) -> bool {
        self.directory
    }
}

/// A parsed ISO 9660 directory record (ECMA-119 §9.1).
struct DirectoryRecord {
    extent_location: u32,
    data_length: u32,
    file_flags: u8,
    /// Raw identifier bytes: may carry a `;1` version suffix and, on a Joliet
    /// tree, UTF-16BE code units.
    file_identifier: Vec<u8>,
    /// The Rock Ridge / SUSP "System Use" bytes trailing the identifier.
    system_use: Vec<u8>,
}

impl DirectoryRecord {
    /// Parse a directory record, or `None` when the slice is too short or the
    /// record length field is zero.
    fn parse(data: &[u8], high_sierra: bool) -> Option<Self> {
        if data.len() < 33 || data[0] == 0 {
            return None;
        }

        let extent_location = u32::from_le_bytes(data[2..6].try_into().ok()?);
        let data_length = u32::from_le_bytes(data[10..14].try_into().ok()?);
        let file_flags = if high_sierra { data[24] } else { data[25] };
        let id_len = data[32] as usize;

        if data.len() < 33 + id_len {
            return None;
        }

        let file_identifier = data[33..33 + id_len].to_vec();

        // The System Use area follows the identifier, after a padding byte
        // present only when `id_len` is even
        let su_start = 33 + id_len + (1 - id_len % 2);
        let system_use = data.get(su_start..).map(|s| s.to_vec()).unwrap_or_default();

        Some(Self {
            extent_location,
            data_length,
            file_flags,
            file_identifier,
            system_use,
        })
    }

    fn is_directory(&self) -> bool {
        (self.file_flags & 0x02) != 0
    }

    /// True for the `.` (current directory) entry, identifier `\x00`.
    fn is_self(&self) -> bool {
        self.file_identifier.is_empty() || self.file_identifier == [0x00]
    }

    /// True for the `..` (parent directory) entry, identifier `\x01`.
    fn is_parent(&self) -> bool {
        self.file_identifier == [0x01]
    }

    /// The display name, decoded as UTF-16BE on a Joliet tree and as (lossy)
    /// UTF-8 otherwise. The `;1` version suffix is stripped from everything and
    /// trailing dots from directory names.
    fn clean_name(&self, joliet: bool) -> String {
        let decoded = if joliet {
            decode_utf16be(&self.file_identifier)
        } else {
            String::from_utf8_lossy(&self.file_identifier).into_owned()
        };

        let name = match decoded.rfind(';') {
            Some(idx) => &decoded[..idx],
            None => &decoded[..],
        };

        if self.is_directory() {
            name.trim_end_matches('.').to_string()
        } else {
            name.to_string()
        }
    }
}

/// Read the little-endian half of an 8-byte ISO 9660 "both-endian" `u32`.
fn both_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// True if `system_use` holds any recognizable SUSP / Rock Ridge entry. Decides
/// whether the primary tree is browsed in preference to a Joliet one.
fn detect_rock_ridge(system_use: &[u8]) -> bool {
    let mut p = 0usize;
    while p + 4 <= system_use.len() {
        let sig = &system_use[p..p + 2];
        let len = system_use[p + 2] as usize;
        if len < 4 || p + len > system_use.len() {
            break;
        }
        if matches!(
            sig,
            b"SP" | b"ER" | b"PX" | b"NM" | b"SL" | b"TF" | b"RR" | b"CE"
        ) {
            return true;
        }
        p += len;
    }
    false
}

/// Detect Rock Ridge by inspecting the root directory's `.` record, whose
/// System Use area carries the SUSP indicator when the disc uses the extension.
fn detect_rock_ridge_root(
    reader: &mut dyn SectorReader,
    root_lba: u32,
    root_size: u32,
    high_sierra: bool,
) -> bool {
    if root_size == 0 {
        return false;
    }
    let want = (root_size as usize).min(SECTOR_SIZE as usize);
    let data = match reader.read_bytes(root_lba as u64 * SECTOR_SIZE, want) {
        Ok(data) => data,
        Err(_) => return false,
    };
    if data.is_empty() {
        return false;
    }
    let record_length = data[0] as usize;
    if record_length == 0 || record_length > data.len() {
        return false;
    }
    match DirectoryRecord::parse(&data[..record_length], high_sierra) {
        Some(record) => detect_rock_ridge(&record.system_use),
        None => false,
    }
}

/// The Rock Ridge `NM` (alternate name) for a record, following `CE`
/// continuation areas. `None` when the record carries no `NM` entry.
fn rock_ridge_name(system_use: &[u8], reader: &mut dyn SectorReader) -> Option<String> {
    let mut name = String::new();
    let mut have_name = false;
    // Areas to scan: the record's System Use, plus any CE continuation areas
    let mut areas: Vec<Vec<u8>> = vec![system_use.to_vec()];
    let mut ce_reads = 0u32;
    let mut idx = 0usize;

    while idx < areas.len() {
        let area = areas[idx].clone();
        idx += 1;
        let mut p = 0usize;
        while p + 4 <= area.len() {
            let sig = [area[p], area[p + 1]];
            let len = area[p + 2] as usize;
            if len < 4 || p + len > area.len() {
                break;
            }
            let data = &area[p + 4..p + len];
            match &sig {
                b"ST" => break, // System use terminator for this area
                b"NM" => {
                    if !data.is_empty() {
                        // bit1 = CURRENT ("."), bit2 = PARENT (".."), no content
                        if data[0] & 0x06 == 0 {
                            name.push_str(&String::from_utf8_lossy(&data[1..]));
                        }
                        have_name = true;
                    }
                }
                // block(both,8) offset(both,8) length(both,8)
                b"CE" if data.len() >= 24 && ce_reads < 16 => {
                    let block = both_u32(&data[0..8]);
                    let ce_offset = both_u32(&data[8..16]);
                    let ce_len = both_u32(&data[16..24]);
                    if ce_len > 0 {
                        ce_reads += 1;
                        let byte_offset = block as u64 * SECTOR_SIZE + ce_offset as u64;
                        if let Ok(continuation) = reader.read_bytes(byte_offset, ce_len as usize) {
                            areas.push(continuation);
                        }
                    }
                }
                // SP / ER / PD / RR / PL / CL / RE, PX / SL / TF: skip
                _ => {}
            }
            p += len;
        }
    }

    if have_name && !name.is_empty() {
        Some(name)
    } else {
        None
    }
}

/// ISO 9660 filesystem browser.
pub struct Iso9660Filesystem {
    reader: Box<dyn SectorReader>,
    root_location: u32,
    root_size: u32,
    /// The tree being browsed is a Joliet one, so identifiers are UTF-16BE.
    joliet: bool,
    high_sierra: bool,
}

impl Iso9660Filesystem {
    /// Read the volume descriptors and pick a directory tree to browse.
    ///
    /// The primary tree is preferred when it carries **Rock Ridge**, so that
    /// POSIX metadata and long names are available; otherwise a **Joliet** tree
    /// is used when present, for its Unicode names. A plain ISO 9660 disc falls
    /// back to the primary tree and its 8.3 style names.
    pub fn new(mut reader: Box<dyn SectorReader>) -> Result<Self> {
        let pvd = PrimaryVolumeDescriptor::read_from(&mut *reader)?;
        let high_sierra = pvd.high_sierra;

        // High Sierra predates both extensions, so neither probe matches and it
        // browses its primary tree
        let rock_ridge = detect_rock_ridge_root(
            reader.as_mut(),
            pvd.root_directory_lba,
            pvd.root_directory_size,
            high_sierra,
        );
        let joliet_svd = if rock_ridge {
            None
        } else {
            JolietVolumeDescriptor::find(reader.as_mut())
        };

        let (root_location, root_size, joliet) = match joliet_svd {
            Some(svd) => (svd.root_directory_lba, svd.root_directory_size, true),
            None => (pvd.root_directory_lba, pvd.root_directory_size, false),
        };

        Ok(Self {
            reader,
            root_location,
            root_size,
            joliet,
            high_sierra,
        })
    }

    /// The root directory entry.
    pub fn root(&self) -> FileEntry {
        FileEntry {
            name: String::from("/"),
            path: String::from("/"),
            size: self.root_size as u64,
            location: self.root_location as u64,
            directory: true,
        }
    }

    /// List the direct children of a directory entry.
    pub fn list_directory(&mut self, entry: &FileEntry) -> Result<Vec<FileEntry>> {
        if !entry.is_directory() {
            bail!("not a directory: {}", entry.path);
        }

        let data = if entry.size == 0 {
            Vec::new()
        } else {
            self.reader
                .read_bytes(entry.location * SECTOR_SIZE, entry.size as usize)?
        };
        Ok(self.parse_directory(&data, &entry.path))
    }

    /// Parse every directory record in a directory extent, skipping `.` and `..`.
    fn parse_directory(&mut self, data: &[u8], parent_path: &str) -> Vec<FileEntry> {
        let mut entries = Vec::new();
        let mut offset = 0usize;

        while offset < data.len() {
            let record_length = data[offset] as usize;

            // A record length of 0 ends the current sector: skip to the next
            if record_length == 0 {
                let next_sector = (offset / SECTOR_SIZE as usize + 1) * SECTOR_SIZE as usize;
                if next_sector >= data.len() {
                    break;
                }
                offset = next_sector;
                continue;
            }

            if offset + record_length > data.len() {
                break;
            }

            if let Some(record) =
                DirectoryRecord::parse(&data[offset..offset + record_length], self.high_sierra)
                && !record.is_self()
                && !record.is_parent()
            {
                let name = rock_ridge_name(&record.system_use, self.reader.as_mut())
                    .unwrap_or_else(|| record.clean_name(self.joliet));
                let path = if parent_path == "/" {
                    format!("/{}", name)
                } else {
                    format!("{}/{}", parent_path, name)
                };
                entries.push(FileEntry {
                    name,
                    path,
                    size: record.data_length as u64,
                    location: record.extent_location as u64,
                    directory: record.is_directory(),
                });
            }

            offset += record_length;
        }

        // Directories first, then files, each sorted by lowercase name. The sort
        // is stable, so the several records of a multi-extent file keep their
        // on-disc order and the first of them is still the first extent.
        entries.sort_by(|a, b| match (a.directory, b.directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        entries
    }
}
