//! The container: header, chunk envelope, index records and metadata, as
//! `formats/design/01-format-spec.md` lays them out and
//! `formats/design/07-gamecube-wii-profile.md` specialises them.
//!
//! Integers are big endian; `uvarint` is unsigned LEB128.

use serde::{Deserialize, Serialize};

use super::error::{Error, Result};

pub const MAGIC: [u8; 4] = *b"ORE1";
pub const SPEC_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 192;
pub const ENVELOPE_MAGIC: [u8; 2] = *b"OC";

pub const REQ_VARIABLE_CHUNKS: u32 = 1 << 0;
/// The object's chunks lie in a store, which its metadata names.
pub const REQ_LIBRARY: u32 = 1 << 1;
pub const REQ_SOLID_GROUPS: u32 = 1 << 5;
/// Every required flag this reader understands.
const REQ_KNOWN: u32 = REQ_VARIABLE_CHUNKS | REQ_LIBRARY | REQ_SOLID_GROUPS;

pub const INDEX_FLAT: u8 = 0;
pub const INDEX_LIBRARY: u8 = 2;

pub const MEDIA_DVD_2048: u16 = 64;

/// Index record kinds.
pub const KIND_ZERO: u8 = 2;
pub const KIND_GENERATED: u8 = 4;
pub const KIND_DEDUP: u8 = 5;
pub const KIND_SOLID: u8 = 7;
/// A chunk of the store, by its id.
pub const KIND_STORE: u8 = 9;

/// The GameCube and Wii padding generator.
pub const GEN_GC_WII_FILLER: u8 = 4;

/// Bounds a hostile file cannot push a reader past.
pub const GROUP_MAX: u64 = 16 << 20;
pub const META_MAX: u64 = 16 << 20;

/// The codec a group is compressed with, by its spec id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    /// Zstandard, level 19, its window the group's size.
    #[default]
    Zstd,
    /// LZMA2, preset 9, its dictionary the group's size.
    Lzma2,
}

impl Codec {
    pub fn id(self) -> u8 {
        match self {
            Self::Zstd => 2,
            Self::Lzma2 => 8,
        }
    }

    pub fn from_id(id: u8) -> Result<Self> {
        match id {
            2 => Ok(Self::Zstd),
            8 => Ok(Self::Lzma2),
            _ => Err(Error::Unsupported(format!("codec {id}"))),
        }
    }
}

/// What an encode is for, which sets the group size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// 1 MiB groups: a read decodes little.
    #[default]
    Stream,
    /// 4 MiB groups: smaller.
    Archive,
}

impl Profile {
    pub fn id(self) -> u8 {
        match self {
            Self::Stream => 0,
            Self::Archive => 1,
        }
    }

    pub fn from_id(id: u8) -> Result<Self> {
        match id {
            0 => Ok(Self::Stream),
            1 => Ok(Self::Archive),
            _ => Err(Error::Unsupported(format!("profile {id}"))),
        }
    }

    pub fn group_size(self) -> usize {
        match self {
            Self::Stream => 1 << 20,
            Self::Archive => 4 << 20,
        }
    }
}

/// The fixed header, at offset 0.
#[derive(Debug, Default, Clone)]
pub struct Header {
    pub flags_required: u32,
    pub flags_optional: u32,
    pub profile: u8,
    pub fidelity: u8,
    pub media_class: u16,
    pub index_kind: u8,
    pub logical_size: u64,
    pub chunk_nominal: u32,
    pub chunk_max: u32,
    pub chunk_count: u32,
    pub object_count: u32,
    pub index_offset: u64,
    pub index_len: u64,
    pub index_crc: u32,
    pub meta_offset: u64,
    pub meta_len: u32,
    pub meta_crc: u32,
    pub logical_hash: [u8; 32],
    pub merkle_root: [u8; 32],
    pub params_digest: [u8; 8],
}

impl Header {
    pub fn write(&self) -> [u8; HEADER_LEN] {
        let mut out = [0; HEADER_LEN];
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&SPEC_VERSION.to_be_bytes());
        out[6..8].copy_from_slice(&(HEADER_LEN as u16).to_be_bytes());
        out[8..12].copy_from_slice(&self.flags_required.to_be_bytes());
        out[12..16].copy_from_slice(&self.flags_optional.to_be_bytes());
        out[16] = self.profile;
        out[17] = self.fidelity;
        out[18..20].copy_from_slice(&self.media_class.to_be_bytes());
        out[20] = self.index_kind;
        out[24..32].copy_from_slice(&self.logical_size.to_be_bytes());
        out[32..36].copy_from_slice(&self.chunk_nominal.to_be_bytes());
        out[36..40].copy_from_slice(&self.chunk_max.to_be_bytes());
        out[40..44].copy_from_slice(&self.chunk_count.to_be_bytes());
        out[44..48].copy_from_slice(&self.object_count.to_be_bytes());
        out[48..56].copy_from_slice(&self.index_offset.to_be_bytes());
        out[56..64].copy_from_slice(&self.index_len.to_be_bytes());
        out[64..68].copy_from_slice(&self.index_crc.to_be_bytes());
        out[72..80].copy_from_slice(&self.meta_offset.to_be_bytes());
        out[80..84].copy_from_slice(&self.meta_len.to_be_bytes());
        out[84..88].copy_from_slice(&self.meta_crc.to_be_bytes());
        out[88..120].copy_from_slice(&self.logical_hash);
        out[120..152].copy_from_slice(&self.merkle_root);
        // Fixity and AUX regions: absent
        out[184..192].copy_from_slice(&self.params_digest);
        out
    }

    pub fn read(bytes: &[u8; HEADER_LEN]) -> Result<Self> {
        if bytes[0..4] != MAGIC {
            return Err(Error::BadMagic {
                expected: "ORE1".into(),
                found: String::from_utf8_lossy(&bytes[0..4]).into_owned(),
            });
        }
        let u16_at = |at: usize| u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap());
        let u32_at = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let u64_at = |at: usize| u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap());
        if u16_at(6) as usize != HEADER_LEN {
            return Err(Error::Unsupported(format!("header of {} bytes", u16_at(6))));
        }
        let header = Self {
            flags_required: u32_at(8),
            flags_optional: u32_at(12),
            profile: bytes[16],
            fidelity: bytes[17],
            media_class: u16_at(18),
            index_kind: bytes[20],
            logical_size: u64_at(24),
            chunk_nominal: u32_at(32),
            chunk_max: u32_at(36),
            chunk_count: u32_at(40),
            object_count: u32_at(44),
            index_offset: u64_at(48),
            index_len: u64_at(56),
            index_crc: u32_at(64),
            meta_offset: u64_at(72),
            meta_len: u32_at(80),
            meta_crc: u32_at(84),
            logical_hash: bytes[88..120].try_into().unwrap(),
            merkle_root: bytes[120..152].try_into().unwrap(),
            params_digest: bytes[184..192].try_into().unwrap(),
        };
        let unknown = header.flags_required & !REQ_KNOWN;
        if unknown != 0 {
            return Err(Error::Unsupported(format!("required flags {unknown:#x}")));
        }
        if header.media_class != MEDIA_DVD_2048 {
            return Err(Error::Unsupported(format!(
                "media class {}",
                header.media_class
            )));
        }
        let thin = header.flags_required & REQ_LIBRARY != 0;
        if header.index_kind != if thin { INDEX_LIBRARY } else { INDEX_FLAT }
            || header.object_count != 1
        {
            return Err(Error::Unsupported("files of several objects".into()));
        }
        Ok(header)
    }
}

pub fn put_uvarint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// A signed difference as an unsigned integer small for small differences.
pub fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

pub fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

/// Bytes being read, each read failing past the end as corrupt.
pub struct Cursor<'a> {
    bytes: &'a [u8],
    pub at: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self
            .bytes
            .get(self.at..self.at.checked_add(n).ok_or_else(truncated)?)
            .ok_or_else(truncated)?;
        self.at += n;
        Ok(bytes)
    }

    /// `N` bytes, as a big-endian integer's.
    pub fn be<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn uvarint(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            value |= ((byte & 0x7F) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::Corrupt("uvarint longer than 64 bits".into()))
    }
}

fn truncated() -> Error {
    Error::Corrupt("truncated structure".into())
}

/// The envelope before a stored group: its kind, codec, where it starts
/// among the stored chunks, its stored length, and the CRC-32C of what it
/// decodes to.
pub fn envelope(codec: Codec, logical_offset: u64, stored_len: u64, crc: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(24);
    out.extend_from_slice(&ENVELOPE_MAGIC);
    out.push(KIND_SOLID);
    out.push(codec.id() << 4);
    put_uvarint(&mut out, logical_offset);
    put_uvarint(&mut out, stored_len);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

pub fn envelope_len(logical_offset: u64, stored_len: u64) -> u64 {
    let mut scratch = Vec::new();
    put_uvarint(&mut scratch, logical_offset);
    put_uvarint(&mut scratch, stored_len);
    (8 + scratch.len()) as u64
}

/// A stored group, as the index's group table lists it.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub codec: Codec,
    pub chunks: u64,
    pub logical_len: u64,
    pub stored_len: u64,
}

// The metadata, as canonical CBOR: every map's keys are declared shortest
// first, then in byte order, as that is the order they are written in.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub media: Media,
    /// The store a thin object's chunks lie in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreRef>,
    pub ledger: Vec<Step>,
    pub identity: Vec<Identity>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreRef {
    #[serde(with = "serde_bytes")]
    pub uuid: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub size: u64,
    pub areas: Vec<Area>,
    pub class: u16,
    pub platform: String,
}

/// A stretch of the disc, in disc order, the areas tiling it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    /// Bytes of disc it covers.
    pub len: u64,
    pub kind: AreaKind,
    pub start: u64,
    /// Index records it takes, in order after the previous area's.
    pub records: u64,
    /// A partition's title key.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "serde_bytes")]
    pub title_key: Option<Vec<u8>>,
    /// A partition's hash blocks recomputing does not give back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<Exception>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AreaKind {
    /// The disc's bytes as they are.
    Raw,
    /// A Wii partition's data: stored decrypted and without its hashes,
    /// 31 KiB a 32 KiB block.
    WiiPartition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exception {
    /// The block, counted from the partition's data.
    pub block: u64,
    /// Its 1 KiB of hashes, decrypted, as the disc has them.
    #[serde(with = "serde_bytes")]
    pub hashes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub algo: String,
    pub view: String,
    #[serde(with = "serde_bytes")]
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub tool: String,
    pub params: Params,
}

/// What an encode was asked for, which `params_digest` digests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    pub codec: Codec,
    pub group: u64,
    pub level: u32,
    pub chunker: Chunker,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunker {
    pub max: u64,
    pub min: u64,
    pub name: String,
}

pub fn to_cbor<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).expect("writing to memory does not fail");
    out
}

pub fn from_cbor<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    ciborium::from_reader(bytes).map_err(|e| Error::Corrupt(format!("metadata: {e}")))
}
