// SPDX-License-Identifier: MIT OR Apache-2.0
//! A PlayStation 3 disc written as an ORE. Each region sector 0 lists is an
//! area: a plain one stored as it is, an encrypted one stored decrypted with
//! the disc key, which the ORE keeps to encrypt it back, so that what is
//! under the encryption compresses and dedups. Without the disc key, an
//! encrypted region is stored as it is, still bit-exact.

use super::*;
use crate::ps3::disc::{self as ps3, Crypt, Probe, Region};

const _: () = assert!(ps3::SECTOR as u64 == SECTOR_2048);

/// A disc's sector, as `ps3` counts them.
const SECTOR_2048: u64 = 2048;

/// What a PS3 disc's sector 1 starts with: its serial follows.
const MAGIC: &[u8; 16] = b"PlayStation3\0\0\0\0";

/// How much of a region is read at a time.
const REGION_WINDOW: u64 = 4 << 20;

/// The disc model's error: a disc that is not one is corrupt.
fn disc_error(error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::InvalidData => Error::Corrupt(error.to_string()),
        _ => Error::Io(error),
    }
}

/// Whether `input` is a PS3 disc, by its sector 1.
pub(super) fn is_ps3(input: &Path) -> bool {
    let mut sector = [0; 16];
    Iso::open(input).is_ok_and(|mut iso| {
        iso.size() > 2 * SECTOR_2048
            && iso.read_at(SECTOR_2048, &mut sector).is_ok()
            && &sector == MAGIC
    })
}

/// The key of `keys` that opens the disc `iso`, told by what its encrypted
/// EBOOT.BIN decrypts to.
fn disc_key(iso: &mut Iso, keys: &[Key]) -> Result<Option<Key>> {
    if keys.is_empty() {
        return Ok(None);
    }
    let mut read = |lba: u32, sector: &mut [u8; ps3::SECTOR]| {
        iso.read_at(lba as u64 * SECTOR_2048, sector)
            .map_err(|error| std::io::Error::other(error.to_string()))
    };
    Ok(Probe::find(&mut read)
        .map_err(disc_error)?
        .and_then(|probe| keys.iter().find(|key| probe.opens(key)).copied()))
}

/// Write the PS3 disc at `input`, a Redump ISO, to `part`, decrypting what
/// one of `keys` opens.
pub(super) fn encode(
    input: &Path,
    part: &Path,
    options: &Options,
    keys: &[Key],
    store: Option<&mut StoreWriter>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let mut iso = Iso::open(input)?;
    let size = iso.size();
    if !size.is_multiple_of(SECTOR_2048) {
        return Err(Error::Corrupt(format!(
            "a PS3 disc of {size} bytes, not whole sectors"
        )));
    }
    let mut sector0 = [0; ps3::SECTOR];
    iso.read_at(0, &mut sector0)?;
    let regions = ps3::regions(&sector0).map_err(disc_error)?;
    if regions
        .last()
        .map(|region| (region.end as u64 + 1) * SECTOR_2048)
        != Some(size)
    {
        return Err(Error::Corrupt(
            "a PS3 disc whose regions do not end with it".into(),
        ));
    }
    let key = disc_key(&mut iso, keys)?;

    (|| {
        let mut encoder = Encoder::new(part, options, store)?;
        encoder.set_codec(options.codec)?;
        let mut hashes = Hashes::default();
        let mut areas = Vec::new();
        let mut decrypted = 0;
        let mut disc = vec![0; REGION_WINDOW as usize];
        for region in &regions {
            let start = region.start as u64 * SECTOR_2048;
            let len = (region.end - region.start + 1) as u64 * SECTOR_2048;
            let records = encoder.records;
            let crypt = match (region.encrypted, key) {
                (true, Some(key)) => Some(Crypt::new(
                    &key,
                    vec![Region {
                        encrypted: true,
                        ..*region
                    }],
                )),
                _ => None,
            };
            let mut segmenter = Segmenter::without_padding(start);
            let mut at = 0;
            while at < len {
                let length = (len - at).min(REGION_WINDOW) as usize;
                let disc = &mut disc[..length];
                iso.read_at(start + at, disc)?;
                hashes.update(disc);
                match &crypt {
                    Some(crypt) => {
                        let lba = ((start + at) / SECTOR_2048) as u32;
                        let mut plain = disc.to_vec();
                        crypt.decrypt(lba, &mut plain);
                        let mut back = plain.clone();
                        crypt.encrypt(lba, &mut back);
                        if back != *disc {
                            return Err(Error::Corrupt(format!(
                                "the PS3 region at sector {lba} does not encrypt back"
                            )));
                        }
                        segmenter.feed(&plain, at, None, &mut |run| encoder.push(run));
                    }
                    None => segmenter.feed(disc, at, None, &mut |run| encoder.push(run)),
                }
                encoder.flush(false)?;
                progress(length as u64);
                at += length as u64;
            }
            segmenter.finish(&mut |run| encoder.push(run));
            if crypt.is_some() {
                decrypted += len;
            }
            areas.push(Area {
                len,
                file: None,
                kind: match crypt {
                    Some(_) => AreaKind::Ps3,
                    None => AreaKind::Raw,
                },
                frame: None,
                start,
                records: encoder.records - records,
                sectors: None,
                title_key: crypt.as_ref().and(key).map(|key| key.to_vec()),
                codec: None,
                exceptions: Vec::new(),
                wiiu: None,
                period: None,
            });
        }

        let Hashes {
            crc32,
            md5,
            sha1,
            blake3: logical,
        } = hashes;
        let logical_hash: [u8; 32] = logical.finalize().into();
        let identity = |algo: &str, value: Vec<u8>| Identity {
            algo: algo.into(),
            view: "logical".into(),
            value,
        };
        let meta = Meta {
            media: Media {
                cue: None,
                sbi: None,
                ticket: None,
                size,
                areas,
                class: MEDIA_DVD_2048,
                platform: "ps3".into(),
            },
            store: None,
            ledger: vec![
                step("ps3-decrypt", Some(decrypted)),
                step("zero", Some(encoder.zeros)),
                step("mincdc-dedup", Some(encoder.deduped)),
            ],
            identity: vec![
                identity("blake3-256", logical_hash.to_vec()),
                identity("sha-1", sha1.finalize().to_vec()),
                identity("md5", md5.finalize().to_vec()),
                identity("crc-32", crc32.finalize().to_be_bytes().to_vec()),
            ],
            provenance: placeholder_provenance(options.codec),
        };
        Ok(Stats {
            input_size: size,
            output_size: encoder.finish(meta, options, logical_hash)?,
        })
    })()
}
