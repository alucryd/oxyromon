//! A small synthetic Wii U disc, laid out as Cemu reads one, encrypted with
//! made-up keys: no real disc or key is needed, nor shipped.

#![allow(dead_code)]

use aes::Aes128;
use aes::cipher::{Array, BlockCipherEncrypt, KeyInit};
use sha1::{Digest, Sha1};

pub const SECTOR: usize = 0x8000;
pub const DISC_KEY: [u8; 16] = [0x11; 16];
pub const COMMON_KEY: [u8; 16] = [0x22; 16];
pub const TITLE_KEY: [u8; 16] = [0x33; 16];
pub const TITLE_ID: u64 = 0x0005_0000_1010_1010;
/// The WUA folder the disc converts to: its title ID, and `title_version`
/// 0x0020.
pub const FOLDER: &str = "0005000010101010_v32";
const OFFSET_FACTOR: usize = 0x20;

pub fn cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], buf: &mut [u8]) {
    let cipher = Aes128::new(key.into());
    let mut chain = *iv;
    for block in buf.as_chunks_mut::<16>().0 {
        for (b, c) in block.iter_mut().zip(chain) {
            *b ^= c;
        }
        let mut array = Array::from(*block);
        cipher.encrypt_block(&mut array);
        block.copy_from_slice(&array);
        chain = *block;
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

pub enum Node {
    Dir(&'static str, Vec<Node>),
    /// A file, the cluster it's in, its data, and whether it's a link.
    File(&'static str, u16, Vec<u8>, bool),
}

use Node::{Dir, File};

/// The files the game partition holds, and those its WUA should: everything
/// but the link.
pub fn game_files() -> (Vec<Node>, Vec<(String, Vec<u8>)>) {
    let app =
        b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<app type=\"complex\" access=\"777\">\n  \
                <version type=\"unsignedInt\" length=\"4\">16</version>\n  \
                <title_version type=\"hexBinary\" length=\"2\">0020</title_version>\n  \
                <title_id type=\"hexBinary\" length=\"8\">0005000010101010</title_id>\n</app>\n"
            .to_vec();
    let rpx = noise(70_000, 1);
    let meta = b"<?xml version=\"1.0\"?><menu/>".to_vec();
    // Spans three hashed blocks, the next file starts inside the third.
    let data = noise(0xFC00 * 2 + 1234, 2);
    let small = b"a small file in a hashed cluster".to_vec();
    let tree = vec![
        Dir(
            "code",
            vec![
                File("app.xml", 1, app.clone(), false),
                File("game.rpx", 1, rpx.clone(), false),
                File("hidden.rpx", 1, noise(100, 3), true),
            ],
        ),
        Dir(
            "content",
            vec![
                File("data.bin", 2, data.clone(), false),
                File("empty.bin", 2, Vec::new(), false),
                // Data no WUA keeps, which revert data must.
                File("hidden.bin", 2, noise(77, 4), true),
                Dir("sub", vec![File("small.txt", 2, small.clone(), false)]),
            ],
        ),
        Dir("meta", vec![File("meta.xml", 1, meta.clone(), false)]),
    ];
    let expected = [
        ("code/app.xml", app),
        ("code/game.rpx", rpx),
        ("content/data.bin", data),
        ("content/empty.bin", Vec::new()),
        ("content/sub/small.txt", small),
        ("meta/meta.xml", meta),
    ]
    .into_iter()
    .map(|(path, data)| (format!("{FOLDER}/{path}"), data))
    .collect();
    (tree, expected)
}

/// An FST and its clusters, encrypted: `hashed` says which clusters are
/// hashed, or in an unhashed partition, if `blocks`, in 64 KiB blocks each
/// encrypted on its own; cluster 0 is the FST's own. Cluster offsets are laid out as in a
/// partition, which a NUS title ignores.
struct Built {
    fst: Vec<u8>,
    fst_len: usize,
    fst_plain: Vec<u8>,
    clusters: Vec<Vec<u8>>,
    /// Each raw cluster's plaintext, as long as the cluster; empty if hashed.
    plain: Vec<Vec<u8>>,
    hashed: Vec<bool>,
}

fn build(tree: &[Node], key: &[u8; 16], hashed: &[bool], blocks: bool) -> Built {
    // Lay the files out in their clusters.
    let mut streams: Vec<Vec<u8>> = vec![Vec::new(); hashed.len()];
    let mut entries: Vec<[u8; 16]> = Vec::new();
    let mut names = vec![0u8];
    fn walk(
        nodes: &[Node],
        parent: usize,
        streams: &mut [Vec<u8>],
        entries: &mut Vec<[u8; 16]>,
        names: &mut Vec<u8>,
    ) {
        for node in nodes {
            let name_offset = names.len() as u32;
            let index = entries.len();
            let mut entry = [0; 16];
            match node {
                Dir(name, children) => {
                    names.extend(name.as_bytes());
                    names.push(0);
                    entries.push(entry);
                    walk(children, index, streams, entries, names);
                    entry[..4].copy_from_slice(&(0x0100_0000 | name_offset).to_be_bytes());
                    entry[4..8].copy_from_slice(&(parent as u32).to_be_bytes());
                    entry[8..12].copy_from_slice(&(entries.len() as u32).to_be_bytes());
                    entries[index] = entry;
                }
                File(name, cluster, data, link) => {
                    names.extend(name.as_bytes());
                    names.push(0);
                    let stream = &mut streams[*cluster as usize];
                    stream.resize(stream.len().next_multiple_of(OFFSET_FACTOR), 0);
                    let offset = stream.len() / OFFSET_FACTOR;
                    stream.extend(data);
                    let flags: u32 = if *link { 0x80 } else { 0 };
                    entry[..4].copy_from_slice(&((flags << 24) | name_offset).to_be_bytes());
                    entry[4..8].copy_from_slice(&(offset as u32).to_be_bytes());
                    entry[8..12].copy_from_slice(&(data.len() as u32).to_be_bytes());
                    entry[14..16].copy_from_slice(&cluster.to_be_bytes());
                    entries.push(entry);
                }
            }
        }
    }
    let mut root = [0; 16];
    entries.push(root);
    walk(tree, 0, &mut streams, &mut entries, &mut names);
    root[0] = 1;
    root[8..12].copy_from_slice(&(entries.len() as u32).to_be_bytes());
    entries[0] = root;

    // Encrypt each cluster.
    let mut plain = Vec::new();
    let clusters: Vec<Vec<u8>> = streams
        .iter()
        .enumerate()
        .map(|(index, stream)| {
            if hashed[index] && blocks {
                let mut data = stream.clone();
                data.resize(data.len().next_multiple_of(SECTOR).max(SECTOR), 0);
                plain.push(data.clone());
                for (block, chunk) in data.chunks_mut(0x10000).enumerate() {
                    let mut iv = [0; 16];
                    match block {
                        0 => iv[..2].copy_from_slice(&(index as u16).to_be_bytes()),
                        _ => iv[8..].copy_from_slice(&(block as u64).to_be_bytes()),
                    }
                    cbc_encrypt(key, &iv, chunk);
                }
                data
            } else if hashed[index] {
                plain.push(Vec::new());
                hashed_cluster(stream, key)
            } else {
                let mut data = stream.clone();
                data.resize(data.len().next_multiple_of(SECTOR).max(SECTOR), 0);
                plain.push(data.clone());
                let mut iv = [0; 16];
                iv[..2].copy_from_slice(&(index as u16).to_be_bytes());
                cbc_encrypt(key, &iv, &mut data);
                data
            }
        })
        .collect();

    // The FST, sized before the cluster offsets it holds are known.
    let fst_len = 0x20 + 0x20 * clusters.len() + 0x10 * entries.len() + names.len();
    let fst_sectors = fst_len.div_ceil(SECTOR);
    let mut fst = Vec::with_capacity(fst_len);
    fst.extend(b"FST\0");
    fst.extend((OFFSET_FACTOR as u32).to_be_bytes());
    fst.extend((clusters.len() as u32).to_be_bytes());
    fst.resize(0x20, 0);
    let mut sector = 1 + fst_sectors;
    for (index, cluster) in clusters.iter().enumerate() {
        let mut entry = [0u8; 0x20];
        let offset = if index == 0 { 1 } else { sector };
        entry[..4].copy_from_slice(&(offset as u32).to_be_bytes());
        if index > 0 {
            entry[4..8].copy_from_slice(&((cluster.len() / SECTOR) as u32).to_be_bytes());
        }
        entry[0x14] = if hashed[index] { 2 } else { 0 };
        fst.extend(entry);
        if index > 0 {
            sector += cluster.len() / SECTOR;
        }
    }
    for entry in &entries {
        fst.extend(entry);
    }
    fst.extend(&names);
    let mut encrypted = fst.clone();
    encrypted.resize(fst_len.next_multiple_of(16), 0);
    cbc_encrypt(key, &[0; 16], &mut encrypted);
    Built {
        fst: encrypted,
        fst_len,
        fst_plain: fst,
        clusters,
        plain,
        hashed: hashed.to_vec(),
    }
}

/// A content as its TMD lists it: its size, whether it is hashed, and its
/// SHA-1, of its plaintext or, if hashed, of its H3 hashes.
pub struct Content {
    size: u64,
    hashed: bool,
    hash: [u8; 20],
}

/// A partition and the contents a TMD would list for it.
struct Partition {
    bytes: Vec<u8>,
    contents: Vec<Content>,
    /// Where its first hashed cluster starts, if it has one.
    hashed_at: Option<usize>,
}

impl Partition {
    /// Pad it to `sectors`, each free sector repeating the one as many
    /// sectors back as its last region is long, as a disc's are.
    fn fill(mut self, sectors: usize) -> Vec<u8> {
        let last = self
            .contents
            .last()
            .map_or(1, |c| (c.size as usize).div_ceil(SECTOR));
        assert!(
            self.bytes.len() <= sectors * SECTOR,
            "the partition is too small"
        );
        while self.bytes.len() < sectors * SECTOR {
            let at = self.bytes.len() - last * SECTOR;
            let sector = self.bytes[at..at + SECTOR].to_vec();
            self.bytes.extend(sector);
        }
        self.bytes
    }
}

/// What encrypts a partition: a title key, the game partitions', or the
/// disc key, the others', whose clusters are not hashed.
#[derive(Clone, Copy, PartialEq)]
enum Encryption {
    TitleKey,
    DiscKey,
}

/// The H3 hashes of a hashed cluster: one per group of 4096 blocks, of the
/// H2 hashes its first block holds, which this builder leaves zero.
pub fn h3(cluster_len: usize) -> Vec<u8> {
    let groups = (cluster_len / 0x10000).div_ceil(4096);
    (0..groups)
        .flat_map(|_| Sha1::digest([0u8; 0x140]))
        .collect()
}

/// A TMD listing `contents`, their IDs `first_id` on: their indexes, as on a
/// disc, or past them, as on the CDN.
fn tmd(title_id: u64, version: u16, first_id: u32, contents: &[Content]) -> Vec<u8> {
    let mut tmd = vec![0u8; 0xB04 + contents.len() * 0x30];
    tmd[..4].copy_from_slice(&0x0001_0004u32.to_be_bytes());
    tmd[0x18C..0x194].copy_from_slice(&title_id.to_be_bytes());
    tmd[0x1DC..0x1DE].copy_from_slice(&version.to_be_bytes());
    tmd[0x1DE..0x1E0].copy_from_slice(&(contents.len() as u16).to_be_bytes());
    for (index, content) in contents.iter().enumerate() {
        let record = &mut tmd[0xB04 + index * 0x30..];
        record[..4].copy_from_slice(&(first_id + index as u32).to_be_bytes());
        record[4..6].copy_from_slice(&(index as u16).to_be_bytes());
        let flags: u16 = 0x2001 | if content.hashed { 2 } else { 0 };
        record[6..8].copy_from_slice(&flags.to_be_bytes());
        record[8..16].copy_from_slice(&content.size.to_be_bytes());
        record[16..36].copy_from_slice(&content.hash);
    }
    tmd
}

/// A partition: plaintext header, FST in its second sector, then clusters.
fn partition(tree: &[Node], key: &[u8; 16], hashed: &[bool], encryption: Encryption) -> Partition {
    let built = build(tree, key, hashed, encryption == Encryption::DiscKey);
    let (fst_len, clusters) = (built.fst_len, &built.clusters);
    let fst_sectors = fst_len.div_ceil(SECTOR);
    // The FST's sectors, encrypted whole, as content 0 is.
    let mut fst_region = built.fst_plain.clone();
    fst_region.resize(fst_sectors * SECTOR, 0);
    let mut contents = vec![Content {
        size: fst_region.len() as u64,
        hashed: false,
        hash: Sha1::digest(&fst_region).into(),
    }];
    let mut encrypted = fst_region;
    cbc_encrypt(key, &[0; 16], &mut encrypted);
    for (index, cluster) in clusters.iter().enumerate().skip(1) {
        let hash = match built.hashed[index] {
            true => Sha1::digest(h3(cluster.len())).into(),
            false => Sha1::digest(&built.plain[index]).into(),
        };
        contents.push(Content {
            size: cluster.len() as u64,
            hashed: built.hashed[index],
            hash,
        });
    }
    let mut header = vec![0u8; SECTOR];
    header[..4].copy_from_slice(&0xCC93_A4F5u32.to_be_bytes());
    header[4..8].copy_from_slice(&(SECTOR as u32).to_be_bytes());
    header[0x14..0x18].copy_from_slice(&(fst_len as u32).to_be_bytes());
    header[0x18..0x1C].copy_from_slice(&1u32.to_be_bytes());
    (header[0x24], header[0x25]) = match encryption {
        Encryption::TitleKey => (1, 2),
        Encryption::DiscKey => (0, 1),
    };

    let mut out = header;
    out.extend(encrypted);
    let mut hashed_at = None;
    for (index, cluster) in clusters.iter().enumerate().skip(1) {
        if built.hashed[index] && hashed_at.is_none() {
            hashed_at = Some(out.len());
        }
        out.extend(cluster);
    }
    Partition {
        bytes: out,
        contents,
        hashed_at,
    }
}

/// Hashed blocks: 0x400 bytes of hashes holding the SHA-1 of the data of
/// each block in this one's group of 16, then 0xFC00 of data, encrypted with
/// its own hash as IV.
fn hashed_cluster(stream: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let mut data = stream.to_vec();
    data.resize(data.len().next_multiple_of(0xFC00).max(0xFC00), 0);
    let chunks: Vec<&[u8]> = data.chunks(0xFC00).collect();
    let hashes: Vec<[u8; 20]> = chunks.iter().map(|c| Sha1::digest(c).into()).collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let mut area = vec![0u8; 0x400];
        let group = i / 16 * 16;
        for (j, h) in hashes[group..].iter().take(16).enumerate() {
            area[j * 20..j * 20 + 20].copy_from_slice(h);
        }
        let mut iv = [0; 16];
        iv.copy_from_slice(&hashes[i][..16]);
        cbc_encrypt(key, &[0; 16], &mut area);
        let mut block = chunk.to_vec();
        cbc_encrypt(key, &iv, &mut block);
        out.extend(area);
        out.extend(block);
    }
    out.resize(out.len().next_multiple_of(SECTOR), 0);
    out
}

/// A ticket for a title, its title key encrypted with the common key.
fn ticket(title_id: u64, key: &[u8; 16]) -> Vec<u8> {
    let mut ticket = vec![0u8; 0x2A4];
    ticket[..4].copy_from_slice(&0x0001_0004u32.to_be_bytes());
    let mut title_key = *key;
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&title_id.to_be_bytes());
    cbc_encrypt(&COMMON_KEY, &iv, &mut title_key);
    ticket[0x1BF..0x1CF].copy_from_slice(&title_key);
    ticket[0x1DC..0x1E4].copy_from_slice(&title_id.to_be_bytes());
    ticket
}

/// The certificates a disc keeps with its TMD; made up, as nothing reads them.
pub const CERT: &[u8] = b"a certificate chain";

pub const UPDATE_ID: u64 = 0x0005_000E_1010_1010;
const UPDATE_KEY: [u8; 16] = [0x44; 16];
/// The WUA folder of the update: its title ID and TMD version 48.
pub const UPDATE_FOLDER: &str = "0005000e10101010_v48";

/// The game's update as Nintendo's servers deliver it, written to `dir`: a
/// TMD, a ticket, and one `.app` per content, content IDs differing from
/// indexes as they do. Returns the files its WUA folder should hold.
pub fn nus(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let app =
        b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<app type=\"complex\" access=\"777\">\n  \
                <title_version type=\"hexBinary\" length=\"2\">0030</title_version>\n  \
                <title_id type=\"hexBinary\" length=\"8\">0005000E10101010</title_id>\n</app>\n"
            .to_vec();
    let rpx = noise(50_000, 7);
    let data = noise(0xFC00 + 99, 8);
    let tree = vec![
        Dir(
            "code",
            vec![
                File("app.xml", 1, app.clone(), false),
                File("game.rpx", 1, rpx.clone(), false),
            ],
        ),
        Dir("content", vec![File("data.bin", 2, data.clone(), false)]),
    ];
    let built = build(&tree, &UPDATE_KEY, &[false, false, true], false);
    // Content 0, the FST, padded to the AES block size, as the CDN's are.
    let mut fst_plain = built.fst_plain.clone();
    fst_plain.resize(fst_plain.len().next_multiple_of(16), 0);
    let mut contents = vec![Content {
        size: built.fst.len() as u64,
        hashed: false,
        hash: Sha1::digest(&fst_plain).into(),
    }];
    let mut files = vec![built.fst.clone()];
    for (index, cluster) in built.clusters.iter().enumerate().skip(1) {
        let hash = match built.hashed[index] {
            true => Sha1::digest(h3(cluster.len())).into(),
            false => Sha1::digest(&built.plain[index]).into(),
        };
        contents.push(Content {
            size: cluster.len() as u64,
            hashed: built.hashed[index],
            hash,
        });
        files.push(cluster.clone());
    }
    for (index, file) in files.iter().enumerate() {
        std::fs::write(dir.join(format!("{:08x}.app", 0x10 + index)), file).unwrap();
        if contents[index].hashed {
            std::fs::write(dir.join(format!("{:08x}.h3", 0x10 + index)), h3(file.len())).unwrap();
        }
    }
    let tmd = tmd(UPDATE_ID, 48, 0x10, &contents);
    std::fs::write(dir.join("title.tmd"), tmd).unwrap();
    std::fs::write(dir.join("title.tik"), ticket(UPDATE_ID, &UPDATE_KEY)).unwrap();
    [
        ("code/app.xml", app),
        ("code/game.rpx", rpx),
        ("content/data.bin", data),
    ]
    .into_iter()
    .map(|(path, data)| (format!("{UPDATE_FOLDER}/{path}"), data))
    .collect()
}

/// The system title the update partition carries, and its key.
pub const SYSTEM_ID: u64 = 0x0005_0010_1000_1234;
const SYSTEM_KEY: [u8; 16] = [0x55; 16];
/// What a system title's contents hold, so a test can tell they were
/// stored decrypted.
pub const SYSTEM_PLAINTEXT: &[u8] = b"the plaintext of a system title";
/// A made-up vWii common key, and the vWii system title it opens, which an
/// update partition carries too.
pub const VWII_KEY: [u8; 16] = [0x99; 16];
const VWII_ID: u64 = 0x0000_0007_0000_0002;
const VWII_TITLE_KEY: [u8; 16] = [0xAA; 16];
pub const VWII_PLAINTEXT: &[u8] = b"the plaintext of a vWii title";
/// A second disc key, for another disc with the same update.
pub const DISC_KEY_2: [u8; 16] = [0x66; 16];
const SMALL_ID: u64 = 0x0005_0010_1006_0000;
const SMALL_KEY: [u8; 16] = [0x77; 16];
/// The partitions' sizes, in sectors, past what they use.
const FREE: usize = 6;

/// The update partition: a system update, its titles as NUS folders, one
/// with a raw and a hashed content.
fn update_partition(disc_key: &[u8; 16]) -> Vec<u8> {
    let mut raw = SYSTEM_PLAINTEXT.to_vec();
    raw.extend(noise(0x8000 - SYSTEM_PLAINTEXT.len(), 20));
    let mut raw_encrypted = raw.clone();
    cbc_encrypt(&SYSTEM_KEY, &[0; 16], &mut raw_encrypted);
    let mut data = SYSTEM_PLAINTEXT.to_vec();
    data.extend(noise(0xFC00 * 3, 21));
    let hashed = hashed_cluster(&data, &SYSTEM_KEY);
    let mut hashed_iv = [0; 16];
    hashed_iv[..2].copy_from_slice(&1u16.to_be_bytes());
    let contents = [
        Content {
            size: raw.len() as u64,
            hashed: false,
            hash: Sha1::digest(&raw).into(),
        },
        Content {
            size: hashed.len() as u64,
            hashed: true,
            hash: Sha1::digest(h3(hashed.len())).into(),
        },
    ];
    let title = Dir(
        "0005001010001234",
        vec![
            File("00000000.app", 1, raw_encrypted, false),
            File("00000001.app", 1, hashed, false),
            File("title.cert", 1, CERT.to_vec(), false),
            File("title.tik", 1, ticket(SYSTEM_ID, &SYSTEM_KEY), false),
            File("title.tmd", 1, tmd(SYSTEM_ID, 1, 0, &contents), false),
        ],
    );
    // A vWii title: a Wii TMD and a ticket naming the vWii common key, its
    // content as long as the TMD says, the file padded to a whole block.
    let mut vwii = VWII_PLAINTEXT.to_vec();
    vwii.extend(noise(0x1000 + 5 - VWII_PLAINTEXT.len(), 22));
    let mut vwii_encrypted = vwii.clone();
    vwii_encrypted.resize(vwii.len().next_multiple_of(16), 0);
    cbc_encrypt(&VWII_TITLE_KEY, &[0; 16], &mut vwii_encrypted);
    let mut wii_tmd = vec![0u8; 0x1E4 + 36];
    wii_tmd[..4].copy_from_slice(&0x0001_0001u32.to_be_bytes());
    wii_tmd[0x18C..0x194].copy_from_slice(&VWII_ID.to_be_bytes());
    wii_tmd[0x1DE..0x1E0].copy_from_slice(&1u16.to_be_bytes());
    wii_tmd[0x1E4 + 6..0x1E4 + 8].copy_from_slice(&1u16.to_be_bytes());
    wii_tmd[0x1E4 + 8..0x1E4 + 16].copy_from_slice(&(vwii.len() as u64).to_be_bytes());
    wii_tmd[0x1E4 + 16..0x1E4 + 36].copy_from_slice(&Sha1::digest(&vwii));
    let mut vwii_ticket = vec![0u8; 0x2A4];
    let mut encrypted_key = VWII_TITLE_KEY;
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&VWII_ID.to_be_bytes());
    cbc_encrypt(&VWII_KEY, &iv, &mut encrypted_key);
    vwii_ticket[0x1BF..0x1CF].copy_from_slice(&encrypted_key);
    vwii_ticket[0x1DC..0x1E4].copy_from_slice(&VWII_ID.to_be_bytes());
    vwii_ticket[0x1F1] = 2;
    let vwii_title = Dir(
        "0000000700000002",
        vec![
            File("00000000.app", 1, vwii_encrypted, false),
            File("title.tik", 1, vwii_ticket, false),
            File("title.tmd", 1, wii_tmd, false),
        ],
    );
    let tree = vec![Dir(
        "update",
        vec![
            Dir("000000", vec![title, vwii_title]),
            File(
                "app.xml",
                1,
                b"<?xml version=\"1.0\"?><app/>".to_vec(),
                false,
            ),
        ],
    )];
    let up = partition(&tree, disc_key, &[false, true], Encryption::DiscKey);
    let sectors = up.bytes.len() / SECTOR + FREE;
    up.fill(sectors)
}

/// The whole disc: a WUD.
pub fn wud() -> Vec<u8> {
    wud_with(&DISC_KEY)
}

/// The whole disc, encrypted with `disc_key`: its SI partition, an update
/// partition, the game partition, and a second, small game partition, as
/// real discs have, each holding more sectors than it uses.
pub fn wud_with(disc_key: &[u8; 16]) -> Vec<u8> {
    wud_and_hashed(disc_key).0
}

/// The whole disc, and where the game's hashed cluster starts in it.
pub fn wud_and_hashed(disc_key: &[u8; 16]) -> (Vec<u8>, usize) {
    // The game partitions are third and fourth, so the SI partition holds
    // their TMDs, tickets and certificates under 02/ and 03/.
    let gm = partition(
        &game_files().0,
        &TITLE_KEY,
        &[false, false, true],
        Encryption::TitleKey,
    );
    let small = partition(
        &[Dir(
            "code",
            vec![File("app.xml", 1, b"<app/>".to_vec(), false)],
        )],
        &SMALL_KEY,
        &[false, false],
        Encryption::TitleKey,
    );
    let si = partition(
        &[
            Dir(
                "02",
                vec![
                    File("title.cert", 1, CERT.to_vec(), false),
                    File("title.tik", 1, ticket(TITLE_ID, &TITLE_KEY), false),
                    File("title.tmd", 1, tmd(TITLE_ID, 32, 0, &gm.contents), false),
                ],
            ),
            Dir(
                "03",
                vec![
                    File("title.cert", 1, CERT.to_vec(), false),
                    File("title.tik", 1, ticket(SMALL_ID, &SMALL_KEY), false),
                    File("title.tmd", 1, tmd(SMALL_ID, 0, 0, &small.contents), false),
                ],
            ),
        ],
        disc_key,
        &[false, false],
        Encryption::DiscKey,
    );
    let fill = |partition: Partition| {
        let sectors = partition.bytes.len() / SECTOR + FREE;
        partition.fill(sectors)
    };
    let gm_hashed = gm.hashed_at.unwrap();
    let partitions = [
        ("SI", fill(si)),
        ("UP0005001010046200000000001000", update_partition(disc_key)),
        ("GM0005000010101010", fill(gm)),
        ("GM0005001010060000", fill(small)),
    ];

    let mut disc = vec![0u8; 4 * SECTOR];
    disc[..10].copy_from_slice(b"WUP-P-TEST");
    disc[2 * SECTOR..2 * SECTOR + 4].copy_from_slice(&0xCC54_9EB9u32.to_be_bytes());
    let mut table = vec![0u8; SECTOR];
    table[..4].copy_from_slice(&0xCCA6_E67Bu32.to_be_bytes());
    table[4..8].copy_from_slice(&(SECTOR as u32).to_be_bytes());
    table[0x1C..0x20].copy_from_slice(&(partitions.len() as u32).to_be_bytes());
    let mut sector = 4;
    let mut hashed = 0;
    for (i, (name, bytes)) in partitions.iter().enumerate() {
        if name.starts_with("GM0005000010101010") {
            hashed = sector * SECTOR + gm_hashed;
        }
        let entry = &mut table[0x800 + i * 0x80..];
        entry[..name.len()].copy_from_slice(name.as_bytes());
        entry[31] = 1;
        entry[0x20..0x24].copy_from_slice(&(sector as u32).to_be_bytes());
        sector += bytes.len() / SECTOR;
    }
    cbc_encrypt(disc_key, &[0; 16], &mut table);
    disc[3 * SECTOR..].copy_from_slice(&table);
    for (_, bytes) in partitions {
        disc.extend(bytes);
    }
    (disc, hashed)
}

/// Every file in the ZArchive at `path`, with its contents, read with an
/// independent reader, sorted by path.
pub fn archive_files(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    use zarust::{ArchiveReader, ROOT_NODE};
    let mut reader = ArchiveReader::open(path).unwrap();
    assert!(reader.verify_integrity().unwrap());
    let mut files = Vec::new();
    let mut dirs = vec![(String::new(), ROOT_NODE)];
    while let Some((prefix, handle)) = dirs.pop() {
        let entries: Vec<_> = reader
            .directory_entries(handle)
            .unwrap()
            .iter()
            .map(|e| {
                (
                    String::from_utf8(e.name.to_vec()).unwrap(),
                    e.is_file(),
                    e.handle,
                )
            })
            .collect();
        for (name, is_file, handle) in entries {
            let path = format!("{prefix}{name}");
            if is_file {
                files.push((path, reader.read_file_to_end(handle).unwrap()));
            } else {
                dirs.push((format!("{path}/"), handle));
            }
        }
    }
    files.sort();
    files
}
