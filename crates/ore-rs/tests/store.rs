//! Games in a store: thin OREs give back their discs byte for byte, the
//! chunks they share stored once, and a store compacted once a game leaves keeps what
//! the others use.

use std::path::{Path, PathBuf};

use ore_rs::{Options, Reader, StoreOptions};
use tempfile::TempDir;

mod common;
use common::{COMMON_KEY, gamecube_disc, noise, wii_disc, write};

/// Two GameCube discs sharing a file, as regional versions do, each with
/// one of its own.
fn discs() -> [Vec<u8>; 2] {
    let shared = noise(0x100000, 7);
    [1, 2].map(|seed| {
        let mut disc = gamecube_disc(0x580000);
        disc[0x200000..0x300000].copy_from_slice(&shared);
        disc[0x400000..0x480000].copy_from_slice(&noise(0x80000, seed));
        disc
    })
}

/// A system's directory: its store, and the games beneath it.
struct Library {
    _dir: TempDir,
    root: PathBuf,
    store: PathBuf,
}

impl Library {
    fn new(options: &StoreOptions) -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let store = root.join(ore_rs::STORE_DIR);
        ore_rs::create_store(&store, options).unwrap();
        Self {
            _dir: dir,
            root,
            store,
        }
    }

    /// Compress `disc` into a thin ORE at `name`, beneath the root.
    fn add(&self, name: &str, disc: &[u8], keys: &[[u8; 16]]) -> PathBuf {
        let iso = self.root.join("in.iso");
        write(&iso, disc);
        let ore = self.root.join(name);
        std::fs::create_dir_all(ore.parent().unwrap()).unwrap();
        ore_rs::compress_to_store(
            &iso,
            &ore,
            &self.store,
            &Options::default(),
            keys,
            &mut |_| {},
        )
        .unwrap();
        std::fs::remove_file(iso).unwrap();
        ore
    }

    fn store_size(&self) -> u64 {
        segments(&self.store).iter().map(|path| len(path)).sum()
    }
}

fn segments(store: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(store)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ores"))
        .collect();
    paths.sort();
    paths
}

fn len(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

/// Decompress `ore` and check it gives back `disc`.
fn check(ore: &Path, disc: &[u8]) {
    let out = ore.with_extension("iso");
    ore_rs::decompress(ore, &out, &mut |_| {}).unwrap();
    assert!(std::fs::read(&out).unwrap() == disc, "{}", ore.display());
    std::fs::remove_file(out).unwrap();
}

#[test]
fn games_share_their_chunks_and_round_trip() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    // One beside the store, one in a folder beneath, as 1G1R/ is
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);
    check(&a, &discs[0]);
    check(&b, &discs[1]);

    // The shared file is stored once: the store holds three files' noise,
    // and what else the discs hold, once, and their parity, its shards
    // rounded up to whole blocks, which small segments feel
    let store = library.store_size();
    assert!(
        store < (0x200000 + 0x70000) * 112 / 100,
        "store of {store} bytes"
    );
    assert!(len(&a) < 0x10000, "a thin ORE of {} bytes", len(&a));

    // Each game owns its own file, shares the other, and the shares add up
    let sizes = [&a, &b].map(|ore| Reader::open(ore).unwrap().sizes());
    for size in &sizes {
        assert_eq!(size.uncompressed, 0x580000);
        assert!(size.own > 0x80000 && size.own < size.share && size.share < size.standalone);
    }
    let shares: u64 = sizes.iter().map(|size| size.share).sum();
    let files = len(&a) + len(&b);
    // Less the segments' headers and the groups' envelopes
    assert!(
        shares.abs_diff(store + files) < 0x400,
        "{shares} against {store} + {files}"
    );
}

#[test]
fn a_wii_disc_round_trips_through_a_store() {
    let library = Library::new(&StoreOptions::default());
    let disc = wii_disc();
    let ore = library.add("wii.ore", &disc, &[COMMON_KEY]);
    check(&ore, &disc);
    let mut reader = Reader::open(&ore).unwrap();
    let mut buf = vec![0; 0x9000];
    reader.read_at(0x123456, &mut buf).unwrap();
    assert!(buf == disc[0x123456..0x123456 + 0x9000]);
}

#[test]
fn a_thin_ore_says_which_store_it_needs() {
    let library = Library::new(&StoreOptions::default());
    let ore = library.add("a.ore", &discs()[0], &[]);
    let elsewhere = TempDir::new().unwrap();
    let moved = elsewhere.path().join("a.ore");
    std::fs::copy(&ore, &moved).unwrap();
    let error = Reader::open(&moved).err().unwrap();
    assert!(error.to_string().contains("needs the ORE store"), "{error}");
    // Its identity is its own, store or not
    let reader = Reader::inspect(&moved).unwrap();
    assert!(reader.store_uuid().is_some());
    assert!(reader.identity().any(|(_, algo, _)| algo == "sha-1"));

    // Beside another store, it says so too
    ore_rs::create_store(
        &elsewhere.path().join(ore_rs::STORE_DIR),
        &StoreOptions::default(),
    )
    .unwrap();
    assert!(Reader::open(&moved).is_err());
}

#[test]
fn attaching_and_detaching_keep_the_disc() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    library.add("a.ore", &discs[0], &[]);
    let iso = library.root.join("b.iso");
    write(&iso, &discs[1]);
    let standalone = library.root.join("out/b.ore");
    std::fs::create_dir_all(standalone.parent().unwrap()).unwrap();
    ore_rs::compress(&iso, &standalone, &Options::default(), &[], &mut |_| {}).unwrap();

    let before = library.store_size();
    let thin = library.root.join("b.ore");
    let mut consumed = 0;
    ore_rs::attach(
        &standalone,
        &thin,
        &library.store,
        &Options::default(),
        &mut |n| consumed += n,
    )
    .unwrap();
    assert_eq!(consumed, len(&standalone), "progress adds up to the input");
    check(&thin, &discs[1]);
    // Only what a.ore does not share went in
    assert!(library.store_size() - before < 0xA0000);

    let detached = library.root.join("out/b2.ore");
    ore_rs::detach(&thin, &detached, &Options::default(), &mut |_| {}).unwrap();
    assert!(Reader::open(&detached).unwrap().store_uuid().is_none());
    check(&detached, &discs[1]);
    let identity = |ore: &Path| -> Vec<(String, String, Vec<u8>)> {
        Reader::inspect(ore)
            .unwrap()
            .identity()
            .map(|(view, algo, value)| (view.to_string(), algo.to_string(), value.to_vec()))
            .collect()
    };
    assert_eq!(identity(&standalone), identity(&thin));
    assert_eq!(identity(&thin), identity(&detached));
}

#[test]
fn segments_stay_under_their_size() {
    let library = Library::new(&StoreOptions {
        segment_size: 0x40000,
        id_bits: 64,
        parity: 16,
    });
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("b.ore", &discs[1], &[]);
    let segments = segments(&library.store);
    assert!(segments.len() > 2, "{} segments", segments.len());
    // A segment holds a group at least, 1 MiB at most, and its parity
    assert!(
        segments
            .iter()
            .all(|path| len(path) <= (0x40000 + (1 << 20)) * 110 / 100)
    );
    check(&a, &discs[0]);
    check(&b, &discs[1]);
}

#[test]
fn purging_a_game_keeps_the_others() {
    let library = Library::new(&StoreOptions {
        segment_size: 0x40000,
        id_bits: 32,
        parity: 8,
    });
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("b.ore", &discs[1], &[]);

    // Nothing to compact while both are there, once the small segments
    // each commit leaves are merged
    let compacted = ore_rs::compact(&library.store, &Options::default(), 0.0).unwrap();
    assert_eq!(compacted.games, 2);
    assert!(compacted.after <= compacted.before * 101 / 100);
    let full = compacted.after;

    std::fs::remove_file(&a).unwrap();
    let compacted = ore_rs::compact(&library.store, &Options::default(), 0.0).unwrap();
    assert_eq!(compacted.games, 1);
    assert_eq!(compacted.after, library.store_size());
    assert!(
        compacted.after < full - 0x60000,
        "{} of {full}",
        compacted.after
    );
    check(&b, &discs[1]);

    // Added again, a.ore stores its own file anew and both round-trip
    let a = library.add("a.ore", &discs[0], &[]);
    check(&a, &discs[0]);
    check(&b, &discs[1]);
}

#[test]
fn a_game_that_cannot_be_read_stops_a_compaction() {
    let library = Library::new(&StoreOptions::default());
    let ore = library.add("a.ore", &discs()[0], &[]);
    let mut bytes = std::fs::read(&ore).unwrap();
    let meta = u64::from_be_bytes(bytes[72..80].try_into().unwrap()) as usize;
    bytes[meta + 3] ^= 0x40;
    write(&ore, &bytes);
    let before = library.store_size();
    assert!(ore_rs::compact(&library.store, &Options::default(), 0.0).is_err());
    assert_eq!(library.store_size(), before);
}

#[test]
fn the_index_copy_stands_in_for_a_damaged_index() {
    let library = Library::new(&StoreOptions::default());
    let disc = &discs()[0];
    let ore = library.add("a.ore", disc, &[]);
    let index = library.store.join("store.index");
    let mut bytes = std::fs::read(&index).unwrap();
    bytes[60] ^= 1;
    write(&index, &bytes);
    check(&ore, disc);
    std::fs::remove_file(library.store.join("store.index.copy")).unwrap();
    assert!(Reader::open(&ore).is_err());
}

#[test]
fn a_damaged_segment_fails() {
    let library = Library::new(&StoreOptions::default());
    let ore = library.add("a.ore", &discs()[0], &[]);
    let segment = &segments(&library.store)[0];
    let mut bytes = std::fs::read(segment).unwrap();
    bytes[100] ^= 0x40;
    write(segment, &bytes);
    let out = library.root.join("a.iso");
    assert!(ore_rs::decompress(&ore, &out, &mut |_| {}).is_err());
    // A check finds it, and names the game
    let checked = ore_rs::check_store(&library.store, false).unwrap();
    assert_eq!(
        (checked.damaged, checked.repaired, checked.lost_groups),
        (1, 0, 1)
    );
    assert_eq!(checked.lost_games, std::slice::from_ref(&ore));
    assert!(
        ore_rs::detach(
            &ore,
            &library.root.join("x.ore"),
            &Options::default(),
            &mut |_| {}
        )
        .is_err()
    );
    assert!(!out.exists());
}

#[test]
fn a_check_repairs_what_parity_covers() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("b.ore", &discs[1], &[]);
    let segment = &segments(&library.store)[0];
    let whole = std::fs::read(segment).unwrap();
    // Bad sectors, and a burst of 40 KiB
    let mut bytes = whole.clone();
    for at in (100..bytes.len()).step_by(150_001) {
        bytes[at] ^= 0x40;
    }
    for byte in &mut bytes[300_000..340_000] {
        *byte ^= 0x40;
    }
    write(segment, &bytes);
    let checked = ore_rs::check_store(&library.store, true).unwrap();
    assert!(checked.damaged > 10 && checked.repaired == checked.damaged);
    assert_eq!((checked.lost_groups, checked.lost_games.len()), (0, 0));
    assert!(std::fs::read(segment).unwrap() == whole);
    check(&a, &discs[0]);
    check(&b, &discs[1]);
    let checked = ore_rs::check_store(&library.store, true).unwrap();
    // b's write folded a's segment, small, into its own
    assert_eq!((checked.damaged, checked.segments), (0, 1));
}

#[test]
fn without_parity_a_check_only_finds_damage() {
    let library = Library::new(&StoreOptions {
        parity: 0,
        ..StoreOptions::default()
    });
    let ore = library.add("a.ore", &discs()[0], &[]);
    let segment = &segments(&library.store)[0];
    let mut bytes = std::fs::read(segment).unwrap();
    bytes[1000] ^= 0x40;
    write(segment, &bytes);
    let checked = ore_rs::check_store(&library.store, true).unwrap();
    assert_eq!((checked.damaged, checked.lost_groups), (0, 1));
    assert_eq!(checked.lost_games, [ore]);
}

#[test]
fn a_damaged_last_segment_is_not_folded_but_left_to_repair() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let segment = segments(&library.store)[0].clone();
    let whole = std::fs::read(&segment).unwrap();
    let mut bytes = whole.clone();
    bytes[100] ^= 0x40;
    write(&segment, &bytes);
    // b's write finds a's segment failing its CRC, and writes its own
    let b = library.add("b.ore", &discs[1], &[]);
    assert_eq!(segments(&library.store).len(), 2);
    let checked = ore_rs::check_store(&library.store, true).unwrap();
    assert_eq!((checked.repaired, checked.lost_groups), (1, 0));
    assert!(std::fs::read(&segment).unwrap() == whole);
    check(&a, &discs[0]);
    check(&b, &discs[1]);
}
