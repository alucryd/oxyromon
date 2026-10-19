//! The Title MetaData (TMD): the ES record that describes a title's contents.
//!
//! A CTR TMD is the v1 format: a signature, a 100-byte header, a 2336-byte
//! extension that lists up to 64 groups with a SHA-256 each, and the content
//! records the groups point into, 48 bytes apiece. The whole thing carries no
//! magic; it is addressed by the CIA header's ticket/TMD sizes, and this module
//! trusts the signature type's declared size the way [ctrtool] does.
//!
//! [ctrtool]: https://github.com/3DSGuy/Project_CTR

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Width of the fixed part below the signature: the v0 header plus the v1
/// extension. With RSA-2048's 0x180-byte signature this is 2820.
const FIXED_PART: usize = 100 + 2336;
/// One content record.
pub(crate) const RECORD_SIZE: usize = 48;
/// The v1 extension's group array: 64 entries of 36 bytes, all present.
const GROUPS: usize = 64;
const GROUP_SIZE: usize = 36;

/// Total size of an ES signature from its type, the way
/// `getSignatureSizeFromSigType` sizes it.
pub(crate) fn signature_size(sig_type: u32) -> Result<usize> {
    match sig_type {
        0x0001_0000 | 0x0001_0003 => Ok(640), // RSA-4096, SHA-1 or SHA-256
        0x0001_0001 | 0x0001_0004 => Ok(384), // RSA-2048, SHA-1 or SHA-256
        0x0001_0002 | 0x0001_0005 => Ok(192), // ECDSA, SHA-1 or SHA-256
        other => Err(Error::Unsupported(format!(
            "TMD signature type 0x{other:08x}"
        ))),
    }
}

/// The signature type in the first four bytes of a TMD, big-endian like
/// every ES field.
pub(crate) fn sig_type(data: &[u8]) -> u32 {
    be32(data, 0)
}

/// Size of the fixed part of a TMD with `sig_type`'s signature: enough to
/// hold the header, the v1 extension and its group array.
pub(crate) fn fixed_len(sig_type: u32) -> Result<usize> {
    Ok(signature_size(sig_type)? + FIXED_PART)
}

/// How many content records a TMD's groups point at, counted from the group
/// counts the way the reference counts them. `fixed` must be [`fixed_len`]
/// bytes of a TMD whose signature type was already accepted.
pub(crate) fn record_count(fixed: &[u8]) -> Result<usize> {
    let groups = signature_size(be32(fixed, 0))? + 100 + 0x20;
    let mut total = 0usize;
    for group in 0..GROUPS {
        let at = groups + group * GROUP_SIZE;
        let count = be16(fixed, at + 2) as usize;
        if count == 0 {
            continue;
        }
        total = total
            .checked_add(count)
            .ok_or_else(|| Error::Corrupt("TMD content group is too large".into()))?;
    }
    Ok(total)
}

/// One content as the TMD's records describe it. The bytes themselves live in
/// the CIA; see [`crate::Content`] for where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Record {
    pub id: u32,
    pub index: u16,
    pub encrypted: bool,
    pub optional: bool,
    pub size: u64,
    pub hash: [u8; 32],
}

/// The handful of TMD fields that matter: which title this is, how far it is
/// updated, and what contents it expects.
#[derive(Debug, Clone)]
pub(crate) struct Tmd {
    pub title_id: u64,
    pub title_version: u16,
    pub records: Vec<Record>,
}

/// Parse a TMD from `data`, the bytes of a CIA's TMD section.
///
/// ctrtool verifies the group array's SHA-256, and every group's, before it
/// trusts a single record; this does too, so a corrupt TMD is rejected here
/// rather than unpacked as garbage. It does not verify the signature, and does
/// not insist on the RSA-2048 type or the CTR title type that ctrtool's TMD
/// reader hard-codes: the layout is the same for every family, and the content
/// hashes say whether the bytes are right.
pub(crate) fn parse(data: &[u8]) -> Result<Tmd> {
    if data.len() < 4 {
        return Err(Error::Corrupt("TMD is too small".into()));
    }
    let sig_size = signature_size(be32(data, 0))?;
    let fixed_end = sig_size + FIXED_PART;
    // Even the reference refuses a TMD without room for one record.
    if data.len() < fixed_end + RECORD_SIZE {
        return Err(Error::Corrupt("TMD is too small".into()));
    }

    if data[sig_size] != 1 {
        return Err(Error::Unsupported(format!(
            "TMD format version {}",
            data[sig_size]
        )));
    }
    // The header is C packed to 4 bytes, so the three leading bytes pad out
    // to four before the 64-bit fields: sysVersion sits at +4, titleId at
    // +0xC, and the title version at +0x5C behind accessRights at +0x58.
    let title_id = be64(data, sig_size + 0x0C);
    let title_version = be16(data, sig_size + 0x5C);

    // The v1 extension: the hash over the group array, then the array.
    let v1 = sig_size + 100;
    let groups = v1 + 0x20;
    let groups_end = groups + GROUPS * GROUP_SIZE;
    if Sha256::digest(&data[groups..groups_end]).as_slice() != &data[v1..v1 + 0x20] {
        return Err(Error::Corrupt("TMD had an invalid CMD group hash".into()));
    }

    // The records are counted from the groups, not from the header's count,
    // exactly as the reference counts them.
    let mut total = 0usize;
    for group in 0..GROUPS {
        let at = groups + group * GROUP_SIZE;
        let count = be16(data, at + 2) as usize;
        if count == 0 {
            continue;
        }
        total += count;
        let offset = be16(data, at) as usize;
        let end = offset
            .checked_add(count)
            .and_then(|end| RECORD_SIZE.checked_mul(end))
            .ok_or_else(|| Error::Corrupt("TMD content group is too large".into()))?;
        let slice = data
            .get(fixed_end + offset * RECORD_SIZE..fixed_end + end)
            .ok_or_else(|| Error::Corrupt("TMD content group is out of bounds".into()))?;
        if Sha256::digest(slice).as_slice() != &data[at + 4..at + GROUP_SIZE] {
            return Err(Error::Corrupt("TMD had an invalid CMD group hash".into()));
        }
    }
    if total == 0 {
        return Err(Error::Corrupt("TMD is too small".into()));
    }
    let records_end = RECORD_SIZE
        .checked_mul(total)
        .ok_or_else(|| Error::Corrupt("TMD content group is too large".into()))?;
    if data.len() < fixed_end + records_end {
        return Err(Error::Corrupt("TMD is too small".into()));
    }
    let records = total;

    let records = (0..records)
        .map(|record| {
            let at = fixed_end + record * RECORD_SIZE;
            let kind = be16(data, at + 6);
            Record {
                id: be32(data, at),
                index: be16(data, at + 4),
                encrypted: kind & 0x0001 != 0,
                optional: kind & 0x4000 != 0,
                size: be64(data, at + 8),
                hash: data[at + 16..at + 48].try_into().unwrap(),
            }
        })
        .collect();

    Ok(Tmd {
        title_id,
        title_version,
        records,
    })
}

/// The group hash check reads the array through the end of the v1 header,
/// which `data` is verified to hold before anything is read.
fn be16(data: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(data[at..at + 2].try_into().unwrap())
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(data[at..at + 4].try_into().unwrap())
}

fn be64(data: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(data[at..at + 8].try_into().unwrap())
}
