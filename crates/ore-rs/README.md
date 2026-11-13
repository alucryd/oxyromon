# ore-rs

ORE, oxyROMon's ROM enclosure, for GameCube and Wii discs and CDs,
PlayStation's first. Part of
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

The specification, and the GameCube and Wii profile this crate writes,
where every choice below is measured, are unpublished while the format is a
draft. In short:

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

## CDs

A CD is given as its cue sheet, a BIN a track as Redump lists them, and
comes back as those files, the cue sheet included, with the SBI of a
LibCrypt disc when one lies beside the cue sheet under its name. Its
PlayStation profile is unpublished too.

```rust
use std::path::Path;
use ore_rs::Options;

let (cue, ore) = (Path::new("Game (Europe).cue"), Path::new("Game (Europe).ore"));
ore_rs::compress(cue, ore, &Options::default(), &[], &mut |_| {}).unwrap();
// The cue sheet, its tracks beside it under their names, and the SBI
ore_rs::decompress(ore, Path::new("out/Game (Europe).cue"), &mut |_| {}).unwrap();
```

- **Sectors split**: each data sector's sync, address, EDC and ECC are
  generated rather than stored, a sector that does not fit the standard
  exactly kept whole; what is left, its subheader and 2048 or 2324 bytes
  of data, is cut and deduped as a disc image's bytes are.
- **XA audio coded as XA**: each 4-bit sample of a Form 2 XA ADPCM sector
  range coded against what a predictor of the decoded channel expects of
  it, 12.7% under Zstandard, losslessly; a group Zstandard does better on,
  data a disc flags as audio, is stored with Zstandard.
- **STR video coded as MDEC codes**: each frame of a video sector's BS
  bitstream, v1, v2 or v3, Form 1 and *Final Fantasy IX*'s Form 2,
  parsed into its codes and range coded against the frame before, the
  block where it lies and the level at each position, 23.2% under
  Zstandard; a frame is only coded so when its codes
  write back to it byte for byte, and every frame of the maintainer's
  library does.
- **Audio in FLAC**, in frames of 8 sectors, each chunk of 8 sectors stored
  once: a track another disc holds costs nothing in a store.
- **Each file's hashes**: every track, the cue sheet and the SBI are
  recorded under their names, as a DAT lists them.
- On the maintainer's PlayStation library, 10-15% smaller than its CHDs a
  disc, 26-29% in a store, before XA's coder took about 3.4 points more and
  video's about 6.3 more.

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
- **Parity**: each segment is sealed with Reed-Solomon parity, RS(255,239)
  by default, 6.9% with its CRCs. [`check_store`] checks every segment
  and rebuilds what is damaged from it, a burst of 16 shards or bad
  sectors anywhere, up to 16 in a row of 4 KiB blocks across the segment;
  then decodes every group and hashes every chunk, and names the games
  using what is past repair. `StoreOptions` takes 8 parity shards, 3.2%,
  or none.
- **[`purge`]** counts again which chunks the games beneath the store use,
  and copies what is left of mostly dead segments into a new one, and of
  the small segments a write that adds little leaves.
- **[`Reader::sizes`]** gives what a game weighs: its own chunks, its share
  of those it shares, and about what it would weigh detached.
- One writer at a time: a write waits for another to be done.

## Keys

Writing an ORE from a Wii disc decrypts its partitions, which takes the Wii
common key. None ships with this crate: [`load_keys`] reads one from a text
file of hex keys, a BootMii `keys.bin`, or an OTP dump. An ORE keeps each
partition's title key, so reading one needs no key, nor does a GameCube disc.

## Scope

- **GameCube and Wii discs, from an ISO; CDs, from a cue sheet whose
  tracks are a file each**, as Redump lists them. Anything else is refused.
- **Not yet in the file**: the Merkle tree, parity, the tail index and the
  `deep` profile of the spec, beyond a store's segment parity.

## CLI

```
cargo build --release -p ore-rs
orers game.iso                          # to game.ore, next to it
orers --keys keys.bin -o out/ *.iso     # Wii discs, to out/
orers -p archive -c lzma2 game.iso      # smaller, slower to read
orers game.ore                          # back to game.iso
orers "Game (Europe).cue"               # a CD, its SBI with it if beside it
orers -o out/ "Game (Europe).ore"       # back to its cue sheet, tracks and SBI
orers --store GameCube/.ore-store -o GameCube/ *.iso   # thin OREs, chunks in the store
orers --detach -o out/ GameCube/game.ore               # a standalone ORE again
orers GameCube/.ore-store               # check the store, repairing what parity covers
orers --purge GameCube/.ore-store       # and purge it of what no game uses
```

| Flag | Meaning |
| --- | --- |
| `-o DIR` | output directory (default: next to each input) |
| `-p PROFILE` | `stream` (default): 1 MiB groups; `archive`: 4 MiB groups |
| `-c CODEC` | `zstd` (default) or `lzma2` |
| `--store DIR` | write ISOs as thin OREs, their chunks in the store `DIR`, made if missing; attach OREs to it |
| `--detach` | write OREs as standalone OREs |
| `--purge` | once a store is checked, purge it of what no game uses |
| `--keys FILE` | where the Wii common key is |

An ORE written from an ORE needs `-o`, so it lands beside its input rather
than over it. A store given as input is checked and repaired, and fails if anything is
past repair, naming the games it harms. A purge copies what is left of
segments half dead or more.

Like every oxyROMon tool, `orers` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed.

## Verification

`tests/roundtrip.rs` round-trips a synthetic GameCube disc and a synthetic
Wii disc, a hash exception included, through every profile and codec, reads
at offsets across areas, and flips bytes of a group, the index and the
metadata to check each is caught. `tests/store.rs` writes discs sharing a
file to a store, checks it is stored once, attaches, detaches and purges,
damages the index and segments, and repairs them; the parity is checked
against scattered damage, a burst as long as the parity, a row past it,
and a segment cut short. `tests/cd.rs` builds a CD of every kind of
sector PlayStation discs hold, subheader copies that differ and a sector
nothing generates among them, with two audio tracks and an SBI, and
round-trips it as its files, alone and through a store, detached,
attached and purged, its XA sectors through the `xa` codec, which
`src/xa.rs` also round-trips on arbitrary bytes, and a still picture's
video sectors through the `str` codec, which `src/video.rs` also
round-trips on frames of BS v2 and v3, Final Fantasy IX's among them,
frames cut short and noise. On real discs, 11 of the maintainer's PlayStation
library come back with every track and cue sheet matching the Redump DAT. The
chunker is checked against
[mothcdc](https://crates.io/crates/mothcdc)'s MinCDC, which it is a port of.

```sh
cargo test -p ore-rs
```

## Source layout

The GameCube and Wii disc specifics live in oxyROMon's unpublished `gcwii`
crate, shared with rvz-rs and wbfs-rs, and the CD-ROM ones in its `cd`
crate, shared with chd-rs. `src/gcwii` and `src/cd` are symlinks to their
sources, and so is `tests/common` to gcwii's synthetic discs; publishing
copies them in, so this crate stands alone.

## Credits

The padding generator and its seed recovery are Dolphin's, by the
[Dolphin Emulator Project](https://github.com/dolphin-emu/dolphin), through
gcwii. MinCDC is Orson Peters's [algorithm](https://github.com/orlp/mincdc).
The CD ECC is MAME's, through oxyROMon's `cd` crate, under BSD-3-Clause.
FLAC is [rusty_flac](https://crates.io/crates/rusty_flac)'s.
The XA ADPCM decoder `xa` models is as Martin Korth's
[psx-spx](https://problemkaputt.de/psxspx-cdrom-xa-audio-adpcm-compression.htm)
documents it; its range coder is LZMA's. The BS bitstream `str` parses
is as psx-spx and [jPSXdec](https://github.com/m35/jpsxdec) document it,
its codes MPEG-1's.
Reed-Solomon coding is
[reed-solomon-erasure](https://crates.io/crates/reed-solomon-erasure)'s.

## License

GPL-3.0-or-later, same as oxyROMon. See [LICENSE](LICENSE).
