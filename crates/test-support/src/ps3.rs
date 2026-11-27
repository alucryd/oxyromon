//! A small synthetic PlayStation 3 disc, laid out as Redump dumps one and
//! encrypted with a made-up key, and the IRD that rebuilds it: no real disc
//! or key is needed, nor shipped.
//!
//! Sectors 0-39 are plain, 40-99 encrypted, 100-109 plain again. The header
//! an IRD keeps is sectors 0-31, its footer the last four. Between the files
//! of the encrypted region lie zeros, which encrypt like any other sector.
//! One file takes two extents, as a file past 4 GiB does.

use std::io::Write;

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use flate2::Compression;
use flate2::write::GzEncoder;
use md5::{Digest, Md5};

pub const SECTOR: usize = 2048;
pub const DISC_KEY: [u8; 16] = [0x44; 16];
pub const GAME_ID: &str = "BLUS00001";
pub const GAME_NAME: &str = "Test Game";
pub const UPDATE_VERSION: &str = "4.90";
pub const UPDATE_PATH: &str = "PS3_UPDATE/PS3UPDAT.PUP";
const SECTORS: u32 = 110;
const HEADER_SECTORS: usize = 32;
const FOOTER_SECTORS: usize = 4;
const ENCRYPTED: (u32, u32) = (40, 99);
const DATA1_KEY: [u8; 16] = [
    0x38, 0x0b, 0xcf, 0x0b, 0x53, 0x45, 0x5b, 0x3c, 0x78, 0x17, 0xab, 0x4f, 0xa3, 0xba, 0x90, 0xed,
];
const DATA1_IV: [u8; 16] = [
    0x69, 0x47, 0x47, 0x72, 0xaf, 0x6f, 0xda, 0xb3, 0x42, 0x74, 0x3a, 0xef, 0xaa, 0x18, 0x62, 0x87,
];

/// The disc, its IRD and its files.
pub struct Disc {
    /// The encrypted image, as Redump dumps it.
    pub iso: Vec<u8>,
    /// Its IRD, gzipped as they are shared.
    pub ird: Vec<u8>,
    /// Its files, decrypted, by their Joliet paths: its JB folder.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Disc {
    /// Write its JB folder into `dir`, but for the update, as most go.
    pub fn write_jb(&self, dir: &std::path::Path) {
        for (path, bytes) in &self.files {
            if path == UPDATE_PATH {
                continue;
            }
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }

    pub fn file(&self, path: &str) -> &[u8] {
        &self.files.iter().find(|(p, _)| p == path).unwrap().1
    }
}

/// Bytes that don't repeat, so a misplaced read can't go unnoticed.
pub fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// A fake PS3UPDAT.PUP: its header, and the version.txt entry 0x100 points to.
pub fn pup(version: &str, len: usize) -> Vec<u8> {
    let mut pup = noise(len, 99);
    pup[..0x50].fill(0);
    pup[..8].copy_from_slice(b"SCEUF\0\0\0");
    pup[0x18..0x20].copy_from_slice(&1u64.to_be_bytes());
    pup[0x30..0x38].copy_from_slice(&0x100u64.to_be_bytes());
    pup[0x38..0x40].copy_from_slice(&0x60u64.to_be_bytes());
    let text = format!("{version}\n");
    pup[0x40..0x48].copy_from_slice(&(text.len() as u64).to_be_bytes());
    pup[0x60..0x60 + text.len()].copy_from_slice(text.as_bytes());
    pup
}

enum Node {
    Dir(&'static str, Vec<Node>),
    /// A file, by its Joliet name, and the extents it takes.
    File(&'static str, Vec<(u32, u32)>),
}

fn tree() -> Node {
    use Node::*;
    Dir(
        "",
        vec![
            File("PS3_DISC.SFB", vec![(32, 1536)]),
            Dir(
                "PS3_GAME",
                vec![
                    File("PARAM.SFO", vec![(33, 700)]),
                    Dir(
                        "USRDIR",
                        vec![
                            File("EBOOT.BIN", vec![(48, 5000)]),
                            Dir("Data", vec![File("Big.dat", vec![(60, 4096), (70, 3000)])]),
                        ],
                    ),
                ],
            ),
            Dir("PS3_UPDATE", vec![File("PS3UPDAT.PUP", vec![(100, 6000)])]),
        ],
    )
}

fn record(name: &[u8], lba: u32, size: u32, flags: u8) -> Vec<u8> {
    let len = 33 + name.len() + (name.len() + 1) % 2;
    let mut r = vec![0u8; len];
    r[0] = len as u8;
    r[2..6].copy_from_slice(&lba.to_le_bytes());
    r[6..10].copy_from_slice(&lba.to_be_bytes());
    r[10..14].copy_from_slice(&size.to_le_bytes());
    r[14..18].copy_from_slice(&size.to_be_bytes());
    r[25] = flags;
    r[28] = 1;
    r[31] = 1;
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

fn encode(name: &str, joliet: bool, file: bool) -> Vec<u8> {
    let name = if file {
        format!("{name};1")
    } else {
        name.to_string()
    };
    if joliet {
        name.encode_utf16().flat_map(u16::to_be_bytes).collect()
    } else {
        name.to_uppercase().into_bytes()
    }
}

/// Lay out one tree's directories from sector `next` on, each in a sector of
/// its own, and return the root's.
fn write_tree(image: &mut [u8], node: &Node, joliet: bool, next: &mut u32, parent: u32) -> u32 {
    let Node::Dir(_, children) = node else {
        unreachable!()
    };
    let lba = *next;
    *next += 1;
    let mut sector = Vec::new();
    sector.extend(record(&[0], lba, SECTOR as u32, 2));
    sector.extend(record(
        &[1],
        if parent == 0 { lba } else { parent },
        SECTOR as u32,
        2,
    ));
    for child in children {
        match child {
            Node::Dir(name, _) => {
                let child_lba = write_tree(image, child, joliet, next, lba);
                sector.extend(record(
                    &encode(name, joliet, false),
                    child_lba,
                    SECTOR as u32,
                    2,
                ));
            }
            Node::File(name, extents) => {
                for (i, &(extent, size)) in extents.iter().enumerate() {
                    let flags = if i + 1 < extents.len() { 0x80 } else { 0 };
                    sector.extend(record(&encode(name, joliet, true), extent, size, flags));
                }
            }
        }
    }
    let at = lba as usize * SECTOR;
    image[at..at + sector.len()].copy_from_slice(&sector);
    lba
}

fn files(node: &Node, path: &str, out: &mut Vec<(String, Vec<(u32, u32)>)>) {
    match node {
        Node::Dir(name, children) => {
            let path = if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}/{name}")
            };
            for child in children {
                files(child, &path, out);
            }
        }
        Node::File(name, extents) => {
            let path = if path.is_empty() {
                name.to_string()
            } else {
                format!("{path}/{name}")
            };
            out.push((path, extents.clone()));
        }
    }
}

fn descriptor(kind: u8, root: u32) -> Vec<u8> {
    let mut d = vec![0u8; SECTOR];
    d[0] = kind;
    d[1..6].copy_from_slice(b"CD001");
    d[6] = 1;
    if kind != 255 {
        d[80..84].copy_from_slice(&SECTORS.to_le_bytes());
        d[84..88].copy_from_slice(&SECTORS.to_be_bytes());
        d[128..130].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        d[130..132].copy_from_slice(&(SECTOR as u16).to_be_bytes());
        d[156..190].copy_from_slice(&record(&[0], root, SECTOR as u32, 2));
    }
    if kind == 2 {
        d[88..91].copy_from_slice(b"%/E");
    }
    d
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// The disc, built once per call.
pub fn disc() -> Disc {
    let mut plain = vec![0u8; SECTORS as usize * SECTOR];
    // sector 0: the two plain regions
    plain[3] = 2;
    plain[12..16].copy_from_slice(&(ENCRYPTED.0 - 1).to_be_bytes());
    plain[16..20].copy_from_slice(&(ENCRYPTED.1 + 1).to_be_bytes());
    plain[20..24].copy_from_slice(&(SECTORS - 1).to_be_bytes());
    plain[SECTOR..SECTOR + 12].copy_from_slice(b"PlayStation3");

    let tree = tree();
    let mut next = 20;
    let primary = write_tree(&mut plain, &tree, false, &mut next, 0);
    let joliet = write_tree(&mut plain, &tree, true, &mut next, 0);
    assert!(next as usize <= HEADER_SECTORS);
    plain[16 * SECTOR..17 * SECTOR].copy_from_slice(&descriptor(1, primary));
    plain[17 * SECTOR..18 * SECTOR].copy_from_slice(&descriptor(2, joliet));
    plain[18 * SECTOR..19 * SECTOR].copy_from_slice(&descriptor(255, 0));

    let mut listed = Vec::new();
    files(&tree, "", &mut listed);
    let mut contents = Vec::new();
    for (i, (path, extents)) in listed.iter().enumerate() {
        let size: u32 = extents.iter().map(|&(_, size)| size).sum();
        let mut bytes = if path == UPDATE_PATH {
            pup(UPDATE_VERSION, size as usize)
        } else {
            noise(size as usize, i as u32 + 1)
        };
        // an executable starts as a SELF does, which tells a disc key right
        if path.ends_with("EBOOT.BIN") {
            bytes[..4].copy_from_slice(b"SCE\0");
        }
        // game data that compresses once decrypted, as most does
        if path.ends_with("Big.dat") {
            bytes = (0..bytes.len()).map(|i| b"game data "[i % 10]).collect();
        }
        let mut offset = 0;
        for &(lba, len) in extents {
            let at = lba as usize * SECTOR;
            plain[at..at + len as usize].copy_from_slice(&bytes[offset..offset + len as usize]);
            offset += len as usize;
        }
        contents.push((path.clone(), bytes));
    }
    let footer_at = (SECTORS as usize - FOOTER_SECTORS) * SECTOR;
    plain[footer_at..].copy_from_slice(&noise(FOOTER_SECTORS * SECTOR, 77));

    let mut iso = plain.clone();
    let cipher = Aes128::new(&DISC_KEY.into());
    for lba in ENCRYPTED.0..=ENCRYPTED.1 {
        let mut chain = (lba as u128).to_be_bytes();
        let at = lba as usize * SECTOR;
        for block in iso[at..at + SECTOR].as_chunks_mut::<16>().0 {
            for (b, c) in block.iter_mut().zip(chain) {
                *b ^= c;
            }
            let mut array = Array::from(*block);
            cipher.encrypt_block(&mut array);
            block.copy_from_slice(&array);
            chain = *block;
        }
    }

    // data1, which encrypts into the disc key
    let mut data1 = Array::from(DISC_KEY);
    Aes128::new(&DATA1_KEY.into()).decrypt_block(&mut data1);
    let data1: Vec<u8> = data1.iter().zip(DATA1_IV).map(|(b, iv)| b ^ iv).collect();

    let mut ird = Vec::new();
    ird.extend(b"3IRD");
    ird.push(9);
    ird.extend(GAME_ID.as_bytes());
    ird.push(GAME_NAME.len() as u8);
    ird.extend(GAME_NAME.as_bytes());
    ird.extend(UPDATE_VERSION.as_bytes());
    ird.extend(b"01.00");
    ird.extend(b"01.00");
    for part in [&plain[..HEADER_SECTORS * SECTOR], &plain[footer_at..]] {
        let gz = gzip(part);
        ird.extend((gz.len() as u32).to_le_bytes());
        ird.extend(gz);
    }
    let bounds = [
        (0, ENCRYPTED.0 - 1),
        ENCRYPTED,
        (ENCRYPTED.1 + 1, SECTORS - 1),
    ];
    ird.push(bounds.len() as u8);
    for (start, end) in bounds {
        ird.extend(Md5::digest(
            &iso[start as usize * SECTOR..(end as usize + 1) * SECTOR],
        ));
    }
    ird.extend((contents.len() as u32).to_le_bytes());
    for ((_, extents), (_, bytes)) in listed.iter().zip(&contents) {
        ird.extend((extents[0].0 as u64).to_le_bytes());
        ird.extend(Md5::digest(bytes));
    }
    ird.extend([0; 4]);
    ird.extend(noise(115, 5));
    ird.extend(data1);
    ird.extend(noise(16, 6));
    ird.extend(0x1234_5678u32.to_le_bytes());
    let crc = crc32fast::hash(&ird);
    ird.extend(crc.to_le_bytes());

    Disc {
        iso,
        ird: gzip(&ird),
        files: contents,
    }
}
