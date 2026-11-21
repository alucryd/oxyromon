//! Precache: the whole ingot, compressed, brought into RAM behind the
//! playhead — every later read decodes from there, progress is polled,
//! a thin ORE brings its store's groups and no others', and the disc
//! comes back byte for byte either way.

// They write the ORE they read
#![cfg(feature = "write")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ore_rs::{Codec, Options, Profile, READ_AHEAD_OFF, Reader, StoreOptions};
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

/// The run's last word: polled until the thread reports itself over,
/// with a generous leave for a machine under test.
fn finished(reader: &Reader) -> ore_rs::Progress {
    let start = Instant::now();
    loop {
        let progress = reader.precache_progress().expect("opened with a precache");
        if !progress.running || start.elapsed() > Duration::from_secs(15) {
            return progress;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_precached_ingot_decodes_everything_from_ram() {
    let dir = TempDir::new().unwrap();
    let (ore, disc) = six_group_ore(dir.path());
    // Read-ahead off: the byte store is the only background subject.
    let mut reader = Reader::open_opts(&ore, 0, READ_AHEAD_OFF, true).unwrap();
    let progress = finished(&reader);
    assert_eq!(
        (progress.done, progress.total),
        (6, 6),
        "every group of the ingot came in, and the run knows it is over"
    );
    assert_eq!(
        reader.precache_hits(),
        0,
        "nothing has been read yet: nothing has been served from RAM"
    );
    // The disc, whole, in one read: six groups decoded, none paid for
    // at the file. Break the lookup in Reader::group and the hits stay
    // at zero while the bytes keep coming — the witness is the counter.
    let mut whole = vec![0; disc.len()];
    reader.read_at(0, &mut whole).unwrap();
    assert_eq!(whole, disc, "the precached disc is the disc");
    assert_eq!(reader.group_decodes(), 6, "each group decoded once");
    assert_eq!(
        reader.precache_hits(),
        6,
        "and each decoded from the bytes the thread brought, not the file"
    );
}

#[test]
fn an_ingot_without_a_precache_has_no_progress_to_poll() {
    let dir = TempDir::new().unwrap();
    let (ore, disc) = six_group_ore(dir.path());
    let mut reader = Reader::open(&ore).unwrap();
    assert!(reader.precache_progress().is_none());
    let mut buf = vec![0; 2048];
    reader.read_at(3 << 20, &mut buf).unwrap();
    assert_eq!(buf, disc[3 << 20..][..2048]);
    assert_eq!(reader.precache_hits(), 0, "no thread, no bytes, no hits");
}

/// A 64 MiB incompressible disc: eight 8 MiB batches of groups, so a
/// run cannot read the whole of it before a truncation reaches it.
fn big_ore(dir: &Path) -> PathBuf {
    let (iso, ore) = (dir.join("big.iso"), dir.join("big.ore"));
    let mut disc = gamecube_disc(64 << 20);
    disc[0x10000..].copy_from_slice(&noise((64 << 20) - 0x10000, 11));
    write(&iso, &disc);
    let options = Options {
        profile: Profile::Stream,
        codec: Codec::Zstd,
        platform: None,
    };
    ore_rs::compress(&iso, &ore, &options, &[], &mut |_| {}).unwrap();
    ore
}

/// The word a frontend waits on to stop waiting: a run that loses the
/// file mid-way reports itself over (`running` false) at whatever it
/// managed (`done` short of `total`), never `running` forever. Every
/// other test here ends `done == total`; nothing else pins the dying
/// path, and a refactor that turns it into a hang would ship green.
#[test]
fn a_run_that_dies_mid_file_reports_itself_over_undone() {
    let dir = TempDir::new().unwrap();
    let ore = big_ore(dir.path());
    let reader = Reader::open_opts(&ore, 0, READ_AHEAD_OFF, true).unwrap();
    // Cut the file under the run until the thread gives up: from the
    // second batch on, every read meets an EOF, and the first error
    // ends the run.
    let start = Instant::now();
    loop {
        let short = std::fs::OpenOptions::new().write(true).open(&ore).unwrap();
        short.set_len(1 << 20).unwrap();
        drop(short);
        let progress = reader.precache_progress().expect("opened with a precache");
        if !progress.running {
            assert!(
                progress.total > 0 && progress.done < progress.total,
                "a run that died is over at what it managed, not the whole: \
                 {} of {}",
                progress.done,
                progress.total
            );
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "the run never reported itself over: still running after the file died"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A clone of a precached ingot polls the same run — a frontend may
/// show its bar on the handle it reads on, not the one it opened on —
/// and a clone's close stops nothing: only the ingot the run was
/// opened on speaks.
#[test]
fn a_clone_polls_the_run_and_its_close_leaves_it_filling() {
    let dir = TempDir::new().unwrap();
    let ore = big_ore(dir.path());
    let reader = Reader::open_opts(&ore, 0, READ_AHEAD_OFF, true).unwrap();
    let clone = reader.try_clone().unwrap();
    let seen = clone
        .precache_progress()
        .expect("a clone of a precached ingot sees the run");
    drop(clone);
    // The clone is gone; the run it witnessed fills to its end.
    let progress = finished(&reader);
    assert!(
        seen.total > 0 && progress.done == seen.total && progress.total == seen.total,
        "closing a clone stopped nothing: {} of {} came in after a clone saw {}",
        progress.done,
        progress.total,
        seen.total
    );
}

#[test]
fn a_thin_ingot_precaches_its_store_groups_and_no_others() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let store = root.join(ore_rs::STORE_DIR);
    ore_rs::create_store(&store, &StoreOptions::default()).unwrap();
    // Two discs sharing a megabyte, as regional versions do: the second
    // one's own groups are its alone, and its precache must reach no
    // group the other holds and it does not use.
    let shared = noise(0x100000, 7);
    let mut discs = [2u64, 3].map(|seed| {
        let mut disc = gamecube_disc(0x580000);
        disc[0x200000..0x300000].copy_from_slice(&shared);
        disc[0x400000..0x480000].copy_from_slice(&noise(0x80000, seed));
        disc
    });
    let mut ores = Vec::new();
    for (seed, disc) in discs.iter_mut().enumerate() {
        let iso = root.join(format!("in{seed}.iso"));
        write(&iso, disc);
        let ore = root.join(format!("game{seed}.ore"));
        ore_rs::compress_to_store(&iso, &ore, &store, &Options::default(), &[], &mut |_| {})
            .unwrap();
        std::fs::remove_file(iso).unwrap();
        ores.push((ore, std::mem::take(disc)));
    }
    for (ore, disc) in &ores {
        let mut reader = Reader::open_opts(ore, 0, READ_AHEAD_OFF, true).unwrap();
        let progress = finished(&reader);
        assert!(
            progress.done == progress.total && progress.total > 1,
            "the ingot's group and the store groups behind its chunks, \
             each once: {} of {}",
            progress.done,
            progress.total
        );
        let mut whole = vec![0; disc.len()];
        reader.read_at(0, &mut whole).unwrap();
        assert_eq!(whole, *disc, "the thin disc is the disc");
        assert!(
            reader.precache_hits() >= 2,
            "store groups came from RAM: {} hits",
            reader.precache_hits()
        );
    }
}
