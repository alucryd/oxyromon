//! Synthetic discs, built here independently of the crate: their junk from
//! the generator as Dolphin's documentation describes it, and a Wii disc's
//! hashes and encryption from scratch, with a made-up common key, as no
//! real key may ship.
//! Each test crate uses a subset, so nothing here is dead on its own.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use aes::Aes128;
use aes::cipher::{Array, BlockCipherEncrypt, KeyInit};
use sha1::{Digest, Sha1};

/// The made-up Wii common key the synthetic Wii disc is encrypted with.
pub const COMMON_KEY: [u8; 16] = [0x11; 16];
const TITLE_KEY: [u8; 16] = *b"rvz-rs title key";
const TITLE_ID: [u8; 8] = [0x00, 0x01, 0x00, 0x00, b'R', b'T', b'S', b'T'];

/// Deterministic pseudo-random bytes, so a failure is reproducible.
pub fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    let mut state = seed | 1;
    while v.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        v.push((state >> 32) as u8);
    }
    v
}

/// Junk as discs are padded with: the lagged Fibonacci generator's output
/// for one 32 KiB block, as `docs/WiaAndRvz.md` describes it.
fn junk_block(seed: u32) -> Vec<u8> {
    let mut buffer = [0u32; 521];
    for (i, word) in buffer.iter_mut().take(17).enumerate() {
        *word = seed.wrapping_mul(0x9E37_79B9).wrapping_add(i as u32 * 0x0101_0101);
    }
    for i in 17..521 {
        buffer[i] = (buffer[i - 17] << 23) ^ (buffer[i - 16] >> 9) ^ buffer[i - 1];
    }
    let forward = |buffer: &mut [u32; 521]| {
        for i in 0..32 {
            buffer[i] ^= buffer[i + 521 - 32];
        }
        for i in 32..521 {
            buffer[i] ^= buffer[i - 32];
        }
    };
    for _ in 0..4 {
        forward(&mut buffer);
    }
    let mut out = Vec::with_capacity(0x8000);
    let mut index = 0;
    while out.len() < 0x8000 {
        let word = buffer[index];
        out.extend([(word >> 24) as u8, (word >> 18) as u8, (word >> 8) as u8, word as u8]);
        index += 1;
        if index == 521 {
            forward(&mut buffer);
            index = 0;
        }
    }
    out
}

/// Fill `data[start..end]` with junk, each 32 KiB block of `data` from its
/// own seed.
fn fill_junk(data: &mut [u8], start: usize, end: usize, seed: u32) {
    let mut position = start;
    while position < end {
        let block = position / 0x8000;
        let in_block = position % 0x8000;
        let length = (0x8000 - in_block).min(end - position);
        let junk = junk_block(seed.wrapping_add(block as u32));
        data[position..position + length].copy_from_slice(&junk[in_block..in_block + length]);
        position += length;
    }
}

fn put_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// What a disc's boot area and file system point to, as a GameCube disc or
/// a Wii partition lays them out: an apploader, a DOL, a file system table
/// and three files, junk everywhere else. Offsets are stored shifted right
/// by `shift`.
fn lay_out(data: &mut [u8], shift: u32, seed: u32) {
    let size = data.len();
    // Junk first, then what is on the disc over it, with the few zeros
    // that usually follow a file
    fill_junk(data, 0x440, size, seed);
    data[0x400..0x440].fill(0);
    let files: [(usize, usize); 3] = [(0x30000, 0x12345), (0x50000, 0x8000), (0x100000, 0x40000)];
    put_u32(data, 0x420, (0x10000 >> shift) as u32);
    put_u32(data, 0x424, (0x20000 >> shift) as u32);
    put_u32(data, 0x428, (0x60 >> shift) as u32);
    put_u32(data, 0x42C, (0x60 >> shift) as u32);
    // The apploader
    data[0x2440..0x3440].copy_from_slice(&noise(0x1000, 1));
    put_u32(data, 0x2440 + 0x14, 0x1000 - 0x20);
    put_u32(data, 0x2440 + 0x18, 0);
    // A DOL of one text section
    data[0x10000..0x12100].copy_from_slice(&noise(0x2100, 2));
    data[0x10000..0x10100].fill(0);
    put_u32(data, 0x10000, 0x100);
    put_u32(data, 0x10000 + 0x90, 0x2000);
    // The file system table: root, three files, their names
    let mut fst = Vec::new();
    let mut entry = |name: u32, offset: u32, size: u32| {
        fst.extend(name.to_be_bytes());
        fst.extend(offset.to_be_bytes());
        fst.extend(size.to_be_bytes());
    };
    entry(0x0100_0000, 0, 4);
    for (i, &(offset, length)) in files.iter().enumerate() {
        entry(i as u32 * 2, (offset >> shift) as u32, length as u32);
    }
    fst.extend(b"a\0b\0c\0");
    fst.resize(0x60, 0);
    data[0x20000..0x20060].copy_from_slice(&fst);
    for (i, &(offset, length)) in files.iter().enumerate() {
        let contents = if i == 1 { vec![0x5A; length] } else { noise(length, 10 + i as u64) };
        data[offset..offset + length].copy_from_slice(&contents);
        let padded = (offset + length).next_multiple_of(4) + 0x20;
        data[offset + length..padded.min(size)].fill(0);
    }
}

/// A GameCube disc of `size` bytes.
pub fn gamecube_disc(size: usize) -> Vec<u8> {
    let mut data = vec![0; size];
    lay_out(&mut data, 0, 0x1234);
    data[0..6].copy_from_slice(b"GTST01");
    put_u32(&mut data, 0x1C, 0xC2339F3D);
    data[0x20..0x30].copy_from_slice(b"rvz-rs test disc");
    data
}

fn sha1(data: &[u8]) -> [u8; 20] {
    Sha1::digest(data).into()
}

fn cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], buf: &mut [u8]) {
    let cipher = Aes128::new(key.into());
    let mut chain = Array::<u8, _>::from(*iv);
    for bytes in buf.as_chunks_mut::<16>().0 {
        for (byte, previous) in bytes.iter_mut().zip(chain.iter()) {
            *byte ^= previous;
        }
        chain.copy_from_slice(bytes);
        cipher.encrypt_block(&mut chain);
        bytes.copy_from_slice(&chain);
    }
}

/// Hash and encrypt a Wii partition's decrypted data, 31 KiB a block.
/// `tamper` changes a hash after hashing, as a hash a disc got wrong.
fn encrypt_partition(decrypted: &[u8], key: &[u8; 16], tamper: Option<(usize, usize)>) -> Vec<u8> {
    let blocks = decrypted.len() / 0x7C00;
    let mut out = vec![0; blocks * 0x8000];
    for group in 0..blocks.div_ceil(64) {
        let mut data = vec![[0u8; 0x7C00]; 64];
        for (j, block) in data.iter_mut().enumerate() {
            let index = group * 64 + j;
            if index < blocks {
                block.copy_from_slice(&decrypted[index * 0x7C00..(index + 1) * 0x7C00]);
            }
        }
        let mut hashes = vec![[0u8; 0x400]; 64];
        for (j, block) in data.iter().enumerate() {
            for k in 0..31 {
                hashes[j][k * 20..k * 20 + 20].copy_from_slice(&sha1(&block[k * 0x400..(k + 1) * 0x400]));
            }
        }
        for j in 0..64 {
            let h1 = sha1(&hashes[j][..0x26C]);
            for neighbour in j / 8 * 8..j / 8 * 8 + 8 {
                hashes[neighbour][0x280 + j % 8 * 20..][..20].copy_from_slice(&h1);
            }
        }
        for k in 0..8 {
            let h2 = sha1(&hashes[k * 8][0x280..0x320]);
            for block in hashes.iter_mut() {
                block[0x340 + k * 20..][..20].copy_from_slice(&h2);
            }
        }
        if let Some((block, at)) = tamper
            && block / 64 == group
        {
            hashes[block % 64][at] ^= 0xFF;
        }
        for j in 0..64 {
            let index = group * 64 + j;
            if index >= blocks {
                break;
            }
            let block = &mut out[index * 0x8000..(index + 1) * 0x8000];
            block[..0x400].copy_from_slice(&hashes[j]);
            cbc_encrypt(key, &[0; 16], &mut block[..0x400]);
            let iv: [u8; 16] = block[0x3D0..0x3E0].try_into().unwrap();
            block[0x400..].copy_from_slice(&data[j]);
            cbc_encrypt(key, &iv, &mut block[0x400..]);
        }
    }
    out
}

/// Where the synthetic Wii disc's partition starts, and its data.
pub const WII_PARTITION: usize = 0x50000;
pub const WII_PARTITION_DATA: usize = WII_PARTITION + 0x20000;
/// Blocks of partition data: two full groups and a partial one.
pub const WII_BLOCKS: usize = 138;

/// A Wii disc with one partition, encrypted with a title key that
/// [`COMMON_KEY`] decrypts. One hash in block 5 is wrong, as discs have.
pub fn wii_disc() -> Vec<u8> {
    let data_size = WII_BLOCKS * 0x8000;
    let size = WII_PARTITION_DATA + data_size + 0x100000;
    let mut data = vec![0; size];
    fill_junk(&mut data, 0x50000, size, 0x5678);
    data[0..6].copy_from_slice(b"RTST01");
    put_u32(&mut data, 0x18, 0x5D1C9EA3);
    data[0x20..0x30].copy_from_slice(b"rvz-rs test disc");
    data[0x60] = 0;
    data[0x61] = 0;
    // One partition in the first table
    data[0x40000..0x40020].fill(0);
    put_u32(&mut data, 0x40000, 1);
    put_u32(&mut data, 0x40004, 0x40020 >> 2);
    put_u32(&mut data, 0x40020, (WII_PARTITION >> 2) as u32);
    put_u32(&mut data, 0x40024, 0);

    // The partition header: ticket, TMD, certificates, H3
    let p = WII_PARTITION;
    data[p..p + 0x20000].copy_from_slice(&noise(0x20000, 3));
    put_u32(&mut data, p, 0x10001);
    let mut title_key = TITLE_KEY;
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&TITLE_ID);
    cbc_encrypt(&COMMON_KEY, &iv, &mut title_key);
    data[p + 0x1BF..p + 0x1CF].copy_from_slice(&title_key);
    data[p + 0x1DC..p + 0x1E4].copy_from_slice(&TITLE_ID);
    data[p + 0x1F1] = 0;
    put_u32(&mut data, p + 0x2A4, 0x208);
    put_u32(&mut data, p + 0x2A8, 0x2C0 >> 2);
    put_u32(&mut data, p + 0x2AC, 0x100);
    put_u32(&mut data, p + 0x2B0, 0x4C0 >> 2);
    put_u32(&mut data, p + 0x2B4, 0x8000 >> 2);
    put_u32(&mut data, p + 0x2B8, 0x20000 >> 2);
    put_u32(&mut data, p + 0x2BC, (data_size >> 2) as u32);

    // The partition's data, as a Wii disc of its own
    let mut decrypted = vec![0; WII_BLOCKS * 0x7C00];
    lay_out(&mut decrypted, 2, 0x9ABC);
    decrypted[0..6].copy_from_slice(b"RTST01");
    put_u32(&mut decrypted, 0x18, 0x5D1C9EA3);
    let encrypted = encrypt_partition(&decrypted, &TITLE_KEY, Some((5, 0x40)));
    data[WII_PARTITION_DATA..WII_PARTITION_DATA + data_size].copy_from_slice(&encrypted);
    data
}

/// Write `data` to `path`.
pub fn write(path: &Path, data: &[u8]) {
    std::fs::write(path, data).unwrap();
}

/// The reference dolphin-tool, through `$DOLPHIN_TOOL` then `$PATH`, or
/// `None` when it is not available.
pub fn dolphin_tool() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("DOLPHIN_TOOL").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("dolphin-tool"))
        .find(|path| path.is_file())
}
