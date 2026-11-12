# ore-rs

ORE, oxyROMon's ROM enclosure, for GameCube and Wii discs. Part of
[oxyROMon](https://github.com/alucryd/oxyromon), as a library and the
`orers` CLI. An ORE gives back the disc that went in, byte for byte, and
records the disc's hashes, so it checks against a DAT without being decoded.

```rust
use std::path::Path;
use ore_rs::Options;

let (iso, ore) = (Path::new("game.iso"), Path::new("game.ore"));

// A Wii disc needs the Wii common key, which ships with nothing here
let keys = ore_rs::load_keys(Path::new("keys.bin")).unwrap();
ore_rs::compress(iso, ore, &Options::default(), &keys, &mut |_| {}).unwrap();
ore_rs::decompress(ore, Path::new("out.iso"), &mut |_| {}).unwrap();
```

## The format

The specification is oxyROMon's
[`formats/design/01-format-spec.md`](../../formats/design/01-format-spec.md),
and the GameCube and Wii profile this crate writes is
[`07-gamecube-wii-profile.md`](../../formats/design/07-gamecube-wii-profile.md),
where every choice below is measured. In short:

- **What RVZ does**: Wii partitions stored decrypted and without their
  hashes, those recomputing would not give back kept as exceptions; padding
  stored as the 68-byte seed that generates it; runs of zeros as their length.
- **Then content-defined chunks**: what is left is cut with MinCDC into
  chunks of 8 to 24 KiB, 16 KiB on average, wherever the content says, so a
  file repeated anywhere on the disc is stored once.
- **Compressed in groups**: the chunks stored are gathered, in the order the
  disc first holds them, into groups of 1 MiB (`stream`) or 4 MiB
  (`archive`), each compressed on its own with Zstandard 19 or LZMA2. A read
  decodes one group.
- **Checked as it is written**: padding is only stored as a seed where the
  seed gives it back, and every Wii group is encrypted again and compared
  with the disc before it is stored decrypted. Decompressing checks the disc
  against the BLAKE3 it was written with; every group carries a CRC-32C.

## A store

A library's discs share much: regional versions, second discs. A store,
`.ore-store` in a system's directory, holds the chunks of every disc
written to it once, each game a thin `.ore` of half a megabyte naming its
chunks there; two discs of one game take a third less than as two OREs.

```rust
use std::path::Path;
use ore_rs::{Options, StoreOptions};

let store = Path::new("GameCube/.ore-store");
ore_rs::create_store(store, &StoreOptions::default()).unwrap();
let (iso, ore) = (Path::new("disc1.iso"), Path::new("GameCube/disc1.ore"));
ore_rs::compress_to_store(iso, ore, store, &Options::default(), &[], &mut |_| {}).unwrap();
```

- **A thin ORE finds its store** as the nearest `.ore-store` up from its
  folder, so games in folders beneath the system's directory find it too.
  Without it, it says which store it needs, and [`Reader::inspect`] still
  gives its identity.
- **Segments** of the store stay under 4 GiB, for FAT32, unless
  `StoreOptions` says otherwise, and chunk ids are 32 bits unless it says 64.
- **[`attach`] and [`detach`]** move a game into a store and out of it,
  its disc, hashes and metadata as they were.
- **[`purge`]** counts again which chunks the games beneath the store use,
  and copies what is left of mostly dead segments into a new one.
- **[`Reader::sizes`]** gives what a game weighs: its own chunks, its share
  of those it shares, and about what it would weigh detached.
- One writer at a time: a write waits for another to be done.

## Keys

Writing an ORE from a Wii disc decrypts its partitions, which takes the Wii
common key. None ships with this crate: [`load_keys`] reads one from a text
file of hex keys, a BootMii `keys.bin`, or an OTP dump. An ORE keeps each
partition's title key, so reading one needs no key, nor does a GameCube disc.

## Scope

- **GameCube and Wii discs, from an ISO.** Anything else is refused.
- **Not yet in the file**: the Merkle tree, parity, the tail index and the
  `deep` profile of the spec; in a store, a segment's fixity and parity.

## CLI

```
cargo build --release -p ore-rs
orers game.iso                          # to game.ore, next to it
orers --keys keys.bin -o out/ *.iso     # Wii discs, to out/
orers -p archive -c lzma2 game.iso      # smaller, slower to read
orers game.ore                          # back to game.iso
orers --store GameCube/.ore-store -o GameCube/ *.iso   # thin OREs, chunks in the store
orers --detach -o out/ GameCube/game.ore               # a standalone ORE again
orers GameCube/.ore-store               # purge the store of what no game uses
```

| Flag | Meaning |
| --- | --- |
| `-o DIR` | output directory (default: next to each input) |
| `-p PROFILE` | `stream` (default): 1 MiB groups; `archive`: 4 MiB groups |
| `-c CODEC` | `zstd` (default) or `lzma2` |
| `--store DIR` | write ISOs as thin OREs, their chunks in the store `DIR`, made if missing; attach OREs to it |
| `--detach` | write OREs as standalone OREs |
| `--keys FILE` | where the Wii common key is |

An ORE written from an ORE needs `-o`, so it lands beside its input rather
than over it. A purge copies what is left of segments half dead or more.

Like every oxyROMon tool, `orers` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed.

## Verification

`tests/roundtrip.rs` round-trips a synthetic GameCube disc and a synthetic
Wii disc, a hash exception included, through every profile and codec, reads
at offsets across areas, and flips bytes of a group, the index and the
metadata to check each is caught. `tests/store.rs` writes discs sharing a
file to a store, checks it is stored once, attaches, detaches and purges,
and damages the index and a segment. The chunker is checked against
[mothcdc](https://crates.io/crates/mothcdc)'s MinCDC, which it is a port of.

```sh
cargo test -p ore-rs
```

## Source layout

The GameCube and Wii disc specifics live in oxyROMon's unpublished `gcwii`
crate, shared with rvz-rs and wbfs-rs. `src/gcwii` is a symlink to its
source, and so is `tests/common` to its synthetic discs; publishing copies
them in, so this crate stands alone.

## Credits

The padding generator and its seed recovery are Dolphin's, by the
[Dolphin Emulator Project](https://github.com/dolphin-emu/dolphin), through
gcwii. MinCDC is Orson Peters's [algorithm](https://github.com/orlp/mincdc).

## License

GPL-3.0-or-later, same as oxyROMon. See [LICENSE](LICENSE).
