//! Reads that run sequential bring the disc in ahead of the playhead:
//! the worker has the groups past it decoded before it asks for them,
//! a read-ahead that is asked to keep out decodes nothing unread, and
//! none of it bends a byte.

// They write the ORE they read
#![cfg(feature = "write")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use ore_rs::{Codec, Options, Profile, READ_AHEAD_OFF, Reader};
use tempfile::TempDir;

#[path = "../../gcwii/tests/common/mod.rs"]
mod common;
use common::{gamecube_disc, noise, write};

/// A 5.5 MiB GameCube disc, incompressible past its header: six 1 MiB
/// groups, none of them a repeated zero.
fn six_group_ore(dir: &Path) -> (PathBuf, Vec<u8>) {
    let (iso, ore) = (dir.join("disc.iso"), dir.join("disc.ore"));
    let mut disc = gamecube_disc(0x580000);
    disc[0x10000..].copy_from_slice(&noise(0x570000, 7));
    write(&iso, &disc);
    let options = Options {
        profile: Profile::Stream,
        codec: Codec::Zstd,
        platform: None,
    };
    ore_rs::compress(&iso, &ore, &options, &[], &mut |_| {}).unwrap();
    (ore, disc)
}

/// The count once the worker has had its moment: polling so the test
/// waits for the prefetch rather than for luck.
fn settled(reader: &Reader, want: u64) -> u64 {
    for _ in 0..40 {
        if reader.group_decodes() >= want {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    reader.group_decodes()
}

/// Reads `from..to` in `step` steps, checking every byte.
fn check(reader: &mut Reader, disc: &[u8], from: usize, to: usize, step: usize) {
    let mut buf = vec![0; step];
    let mut offset = from;
    while offset < to {
        let length = step.min(to - offset);
        reader.read_at(offset as u64, &mut buf[..length]).unwrap();
        assert_eq!(
            &buf[..length],
            &disc[offset..offset + length],
            "at {offset:#x}"
        );
        offset += length;
    }
}

#[test]
fn sequential_reads_leave_groups_decoded_ahead() {
    let dir = TempDir::new().unwrap();
    let (ore, disc) = six_group_ore(dir.path());
    let mut reader = Reader::open(&ore).unwrap();
    // Four reads in a row: the third says playhead, the fourth guesses.
    for i in 0..4u64 {
        reader.read_at(i * 2048, &mut [0; 2048]).unwrap();
    }
    // The reader has asked for one group; the window says four more are
    // on their way, and here they are.
    assert_eq!(
        settled(&reader, 5),
        5,
        "one group for the reads, the window of four ahead of them"
    );
    // Through them and on: the disc comes back byte for byte, and no
    // group is paid for twice on the way.
    check(&mut reader, &disc, 4 << 20, 0x580000, 64 << 10);
    assert_eq!(
        settled(&reader, 6),
        6,
        "every group once, guessed or read, and no group twice"
    );
}

#[test]
fn read_ahead_off_decodes_nobody_elses_groups() {
    let dir = TempDir::new().unwrap();
    let (ore, disc) = six_group_ore(dir.path());
    let mut reader = Reader::open_ahead(&ore, 0, READ_AHEAD_OFF).unwrap();
    for i in 0..4u64 {
        reader.read_at(i * 2048, &mut [0; 2048]).unwrap();
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        reader.group_decodes(),
        1,
        "sequential reads with no worker are just reads"
    );
    // And the disc is still the disc, one thread serving all of it.
    check(&mut reader, &disc, 2048 * 4, 0x580000, 64 << 10);
    assert_eq!(reader.group_decodes(), 6);
}
