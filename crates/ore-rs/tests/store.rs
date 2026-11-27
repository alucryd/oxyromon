//! Games in a store: thin OREs give back their discs byte for byte, the
//! chunks they share stored once, and a store compacted once a game leaves keeps what
//! the others use.

// They write the OREs they read
#![cfg(feature = "write")]

use std::path::{Path, PathBuf};

use ore_rs::{Options, Reader, StoreOptions};
use tempfile::TempDir;

use common::{COMMON_KEY, gamecube_disc, noise, wii_disc, write};
use test_support::gcwii as common;

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
fn a_loaded_index_answers_for_its_store() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);

    // the index read once answers, alone, what the store answers to
    // whoever loads it afresh: the same stats, the same shares, the same
    // full sizes
    let index = ore_rs::Index::load(&library.store).unwrap();
    assert_eq!(index.stats(), ore_rs::store_stats(&library.store).unwrap());
    let paths = [a.as_path(), b.as_path()];
    assert_eq!(
        index.shares(&paths),
        ore_rs::shares(&library.store, &paths).unwrap()
    );
    let sizes = index.game_sizes(&paths);
    assert_eq!(sizes, ore_rs::game_sizes(&library.store, &paths).unwrap());
    for size in sizes.into_iter().map(Option::unwrap) {
        assert_eq!(size.uncompressed, 0x580000);
        assert!(size.own > 0 && size.own < size.share && size.share < size.standalone);
    }

    // A standalone ORE, weighed beside the thin ones: it classes on its
    // own, no index serves it, and its file is its whole weight
    let alone_iso = library.root.join("alone.iso");
    let alone = library.root.join("alone.ore");
    write(&alone_iso, &discs[0]);
    ore_rs::compress(
        &alone_iso,
        &alone,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();
    let mixed = [a.as_path(), b.as_path(), alone.as_path()];
    let weights = ore_rs::ore_weights(&mixed);
    assert!(matches!(weights[2], ore_rs::OreWeight::Standalone));
    let empty = ore_rs::Index::weighed_sizes(&mixed, &weights, &[]);
    assert!(empty[0].is_none() && empty[1].is_none());
    let alone_sizes = empty[2].unwrap();
    assert_eq!(alone_sizes.own, len(&alone));
    assert_eq!(alone_sizes.share, len(&alone));
    assert_eq!(alone_sizes.standalone, len(&alone));
    // the same pass, with the index in hand, answers every file at once
    let served = ore_rs::Index::weighed_sizes(&mixed, &weights, &[&index]);
    assert_eq!(
        served[..2],
        index.game_sizes(&[a.as_path(), b.as_path()])[..]
    );
    assert_eq!(served[2], Some(alone_sizes));
}

#[test]
fn store_stats_tell_the_dedup_from_the_parity() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let one = ore_rs::store_stats(&library.store).unwrap();
    let b = library.add("1G1R/b.ore", &discs[1], &[]);
    let both = ore_rs::store_stats(&library.store).unwrap();

    // The second game adds chunks, its own file's among them, yet less
    // than half again what the store weighed: the shared file is stored
    // once and the discs' zeros still more so
    assert!(both.chunks > one.chunks);
    let added = both.stored - one.stored;
    assert!(added < both.stored / 2, "added {added} to {}", both.stored);

    // The shared 1 MiB weighs about what it lies, halved by the two games
    // using it: the saving of dedup is at least that half, and never more
    // than what the store holds
    assert!(
        both.dedup_saved > 0x100000 / 2,
        "dedup saved only {}",
        both.dedup_saved
    );
    assert!(both.dedup_saved < both.stored, "saved more than stored");

    // Sixteen shards over two hundred thirty-nine, whole blocks: about
    // a sixteenth of what lies, felt more by small segments
    assert!(
        both.parity * 25 > both.stored && both.parity * 8 < both.stored,
        "parity {} against {} stored",
        both.parity,
        both.stored
    );

    // The same ledger the games read: their shares, the OREs' own files
    // subtracted, are the store's chunks
    let shares: u64 = [&a, &b]
        .map(|ore| Reader::open(ore).unwrap().sizes().share)
        .iter()
        .sum();
    let files = len(&a) + len(&b);
    assert!(
        (shares - files).abs_diff(both.stored) < 0x400,
        "{shares} less {files} against {}",
        both.stored
    );
}

#[test]
fn the_store_stats_add_up_over_the_games() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("first-system/a.ore", &discs[0], &[]);
    let b = library.add("second-system/b.ore", &discs[1], &[]);
    // A copy of a, as an update shared between systems names: its chunks
    // count for three games where b's private ones count for one
    let c = library.add("second-system/c.ore", &discs[0], &[]);
    let index = ore_rs::Index::load(&library.store).unwrap();
    let stats = index.stats();

    let savings = index.savings(&[a.as_path(), b.as_path(), c.as_path()]);
    let (a, b, c) = (
        savings[0].unwrap(),
        savings[1].unwrap(),
        savings[2].unwrap(),
    );

    // The dedup saved over the games is what the store says it saved, to
    // the rounding of the shares: every game past the first owes a copy
    let saved = a.dedup_saved + b.dedup_saved + c.dedup_saved;
    assert!(
        saved.abs_diff(stats.dedup_saved) < 0x400,
        "{saved} against {}",
        stats.dedup_saved
    );
    // And the parity they paid totals the footers, give or take the
    // envelope headers and the last segment's unused room: nobody's game
    // lies in those, so the games' sum falls just short of the store's
    let paid = a.parity + b.parity + c.parity;
    assert!(paid > 0, "the store has parity and the games pay it");
    assert!(
        paid.abs_diff(stats.parity) < stats.parity / 8,
        "{paid} against {}",
        stats.parity
    );
    // Two copies of one disc pay exactly the same
    assert_eq!(a, c);
    // And a different disc pays differently: a's private chunks are shared
    // with c and save it, b's are its own alone — the two systems' tiles
    // are their own numbers, not the store's shown twice
    assert_ne!(a, b);
    assert!(a.dedup_saved > b.dedup_saved);
    // A standalone ORE saves nothing and pays nothing of this store's
    let iso = library.root.join("in.iso");
    write(&iso, &discs[1]);
    let alone = library.root.join("standalone.ore");
    ore_rs::compress(&iso, &alone, &Options::default(), &[], &mut |_| {}).unwrap();
    assert_eq!(
        index.savings(&[alone.as_path()])[0],
        Some(ore_rs::Savings {
            dedup_saved: 0,
            parity: 0
        })
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
    let compacted =
        ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).unwrap();
    assert_eq!(compacted.games, 2);
    assert!(compacted.after <= compacted.before * 101 / 100);
    let full = compacted.after;

    std::fs::remove_file(&a).unwrap();
    let compacted =
        ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).unwrap();
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
fn rewriting_a_game_keeps_its_share_true() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);
    let before = ore_rs::store_stats(&library.store).unwrap();
    // Written again over itself, as recompressing a game does: the old
    // thin ORE's references go with the copy it replaces
    let a = library.add("a.ore", &discs[0], &[]);
    check(&a, &discs[0]);
    check(&b, &discs[1]);

    // The shares still add up to the store: the chunks a names twice are
    // still only its game's once
    let sizes = [&a, &b].map(|ore| Reader::open(ore).unwrap().sizes());
    let shares: u64 = sizes.iter().map(|size| size.share).sum();
    let store = library.store_size();
    let files = len(&a) + len(&b);
    assert!(
        shares.abs_diff(store + files) < 0x400,
        "{shares} against {store} + {files}"
    );
    // and the rewrite left no references behind: the same chunks, shared
    // by no more games than they were
    let after = ore_rs::store_stats(&library.store).unwrap();
    assert_eq!(after.chunks, before.chunks);
    assert_eq!(after.stored, before.stored);
    assert_eq!(after.dedup_saved, before.dedup_saved);
}

#[test]
fn recount_gives_back_the_refs_a_removal_leaves() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);

    // A game leaves and nothing tells the store: the chunks it shared keep
    // counting it, and b's share says half the truth
    std::fs::remove_file(&a).unwrap();
    let lied = Reader::open(&b).unwrap().sizes();

    // A recount, segments untouched, finds the one game there is and b's
    // share stops lying: the chunks they shared count for b whole
    assert_eq!(
        ore_rs::recount(&library.store).unwrap(),
        ore_rs::Recounted {
            games: 1,
            unreadable: 0
        }
    );
    let truth = Reader::open(&b).unwrap().sizes();
    assert!(
        truth.share > lied.share,
        "{} against {}",
        truth.share,
        lied.share
    );
    // The recount also knows what is dead, so a compaction takes a's
    // private file out and b's share is the store's whole
    let compacted =
        ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).unwrap();
    assert_eq!(compacted.games, 1);
    let store = library.store_size();
    let sizes = Reader::open(&b).unwrap().sizes();
    assert!(
        sizes.share.abs_diff(store + len(&b)) < 0x400,
        "{} against {} + {}",
        sizes.share,
        store,
        len(&b)
    );
    check(&b, &discs[1]);
}

#[test]
fn a_clone_still_on_the_old_index_never_feeds_the_reloaded_one() {
    let library = Library::new(&StoreOptions {
        segment_size: 0x40000,
        id_bits: 32,
        parity: 8,
    });
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("b.ore", &discs[1], &[]);
    let read_all = |reader: &mut Reader| {
        let mut out = vec![0; reader.size() as usize];
        reader.read_at(0, &mut out).unwrap();
        out
    };

    // The clone reads first, so it holds every old segment open; the
    // reader holds none, and meets the compaction as a segment gone.
    let mut reader = Reader::open_ahead(&b, 0, ore_rs::READ_AHEAD_OFF).unwrap();
    let mut clone = reader.try_clone().unwrap();
    assert!(read_all(&mut clone) == discs[1]);
    std::fs::remove_file(&a).unwrap();
    ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).unwrap();

    assert!(read_all(&mut reader) == discs[1], "the reader reloads");
    assert!(
        read_all(&mut clone) == discs[1],
        "the clone, on its old handles"
    );
    assert!(
        read_all(&mut reader) == discs[1],
        "the reader, after the clone"
    );
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
    assert!(ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).is_err());
    assert_eq!(library.store_size(), before);
}

#[test]
fn a_corrupt_game_in_the_trash_does_not_stop_a_recount() {
    let library = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);

    // The shape a failed check leaves behind: a corrupt thin ORE, magic
    // intact and body unreadable, moved to the Trash beneath the walk
    // root of the recount that follows it.
    let mut bytes = std::fs::read(&a).unwrap();
    let meta = u64::from_be_bytes(bytes[72..80].try_into().unwrap()) as usize;
    bytes[meta + 3] ^= 0x40;
    let trash = library.store.parent().unwrap().join("Trash");
    std::fs::create_dir(&trash).unwrap();
    std::fs::write(trash.join("a.ore"), &bytes).unwrap();
    std::fs::remove_file(&a).unwrap();

    // A game the store still counts for two, but no recount will ever see
    let lied = Reader::open(&b).unwrap().sizes();
    // The recount skips the corrupt file, counts the one game the store
    // really has, and b's share stops being halved for a game in the Trash
    assert_eq!(
        ore_rs::recount(&library.store).unwrap(),
        ore_rs::Recounted {
            games: 1,
            unreadable: 1
        }
    );
    let truth = Reader::open(&b).unwrap().sizes();
    assert!(
        truth.share > lied.share,
        "{} against {}",
        truth.share,
        lied.share
    );
    check(&b, &discs[1]);
    // A compaction still refuses to rewrite the store on counts that
    // skipped a file: the corrupt ORE must be purged first.
    assert!(ore_rs::compact(&library.store, &Options::default(), 0.0, false, &mut |_| {}).is_err());
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
    let checked = ore_rs::check_store(&library.store, false, &mut |_| {}).unwrap();
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
    let checked = ore_rs::check_store(&library.store, true, &mut |_| {}).unwrap();
    assert!(checked.damaged > 10 && checked.repaired == checked.damaged);
    assert_eq!((checked.lost_groups, checked.lost_games.len()), (0, 0));
    assert!(std::fs::read(segment).unwrap() == whole);
    check(&a, &discs[0]);
    check(&b, &discs[1]);
    let checked = ore_rs::check_store(&library.store, true, &mut |_| {}).unwrap();
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
    let checked = ore_rs::check_store(&library.store, true, &mut |_| {}).unwrap();
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
    let checked = ore_rs::check_store(&library.store, true, &mut |_| {}).unwrap();
    assert_eq!((checked.repaired, checked.lost_groups), (1, 0));
    assert!(std::fs::read(&segment).unwrap() == whole);
    check(&a, &discs[0]);
    check(&b, &discs[1]);
}

#[test]
fn thin_ores_open_through_an_index_loaded_once() {
    let library = Library::new(&StoreOptions::default());
    let other = Library::new(&StoreOptions::default());
    let discs = discs();
    let a = library.add("a.ore", &discs[0], &[]);
    let b = library.add("1G1R/b.ore", &discs[1], &[]);
    let foreign = other.add("c.ore", &discs[0], &[]);
    let index = std::sync::Arc::new(ore_rs::Index::load(&library.store).unwrap());
    for (ore, disc) in [(&a, &discs[0]), (&b, &discs[1])] {
        let mut reader = index.open(ore).unwrap();
        let mut out = vec![0; reader.size() as usize];
        reader.read_at(0, &mut out).unwrap();
        assert!(&out == disc);
    }
    // Another store's thin ORE is refused, as Reader::open refuses it
    // beneath the wrong store
    assert!(index.open(&foreign).is_err());
}
