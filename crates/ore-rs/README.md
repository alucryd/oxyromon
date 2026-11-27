# ore-rs

ORE, oxyROMon's ROM enclosure, for GameCube, Wii, PlayStation 2, PSP, Xbox and Xbox 360
discs, CDs, PlayStation's first, Dreamcast GD-ROMs, and cartridge ROMs,
from the Atari 2600 to the DS. Part of
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

The format is ORE 1.0, specified in [SPEC.md](SPEC.md), with conformance
vectors in [`spec/vectors`](spec/vectors) that `tests/vectors.rs` holds this
crate to. In short:

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

A PlayStation 2 DVD is cut as a disc image is, its runs of zeros stored as
their length, with no file system read: its games keep their data in a few
large files, or outside any, and cutting at files finds nothing more. On
the maintainer's 52 DVDs, 4-9% smaller than their CHDs a disc and 16-20% in
a store, where an International version or an undub adds a twentieth to a
half of its original. Its audio, found by its `VAGp`, `SShd` and `RIFF
WAVE` headers, is stored apart: SPU ADPCM through `vag`, each 4-bit sample
coded as `xa` codes XA's, 12-14% under Zstandard, and 16-bit PCM through
`pcm`, de-interleaved into FLAC, which turns *Armored Core 2*, 14% over its
CHD, to 8.5% under. A PlayStation 2 CD is a PlayStation CD, 5-9% under
its CHD.

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
- **[`replace_cue`]** puts another cue sheet in a CD ORE, as when Redump
  revises one, writing its metadata alone again: the new one must give the
  same tracks, audio and data as they were, at the same frames, and names
  the cue sheet and its tracks.
- **Parity**: each segment is sealed with Reed-Solomon parity, RS(255,239)
  by default, 6.9% with its CRCs. [`check_store`] checks every segment
  and rebuilds what is damaged from it, a burst of 16 shards or bad
  sectors anywhere, up to 16 in a row of 4 KiB blocks across the segment;
  then decodes every group and hashes every chunk, and names the games
  using what is past repair. `StoreOptions` takes 8 parity shards, 3.2%,
  or none.
- **[`compact`]** counts again which chunks the games beneath the store use,
  and copies what is left of mostly dead segments into a new one, and of
  the small segments a write that adds little leaves; groups less than half
  full, as writes of a game each leave, are decoded and packed together
  again, each chunk in its group's codec. With `recompress`, every group
  is: the smallest the store gets, for a read and a write of all of it.
- **[`Reader::sizes`]** gives what a game weighs: its own chunks, its share
  of those it shares, and about what it would weigh detached.
- **A write folds the store's last segment into its own** when that one
  is 4 MiB or less, copying its groups as they are, each checked against
  its CRC first, and drops it once the index no longer lists it: a ROM
  added alone costs its chunks, not a segment and 64 KiB of parity of its
  own. Each write still compresses its chunks in groups of its own, so
  games added one at a time compress apart until a compaction packs them
  together. A last segment that fails is left where its parity can repair it.
- **[`StoreBatch`]** writes many games to a store as one write, their
  chunks sharing its groups too, which a write closes at its end, and
  committed once, when every thin ORE lands: how to add a library.
  `orers --store` writes all its inputs so.
- One writer at a time: a write waits for another to be done.

## Keys

Writing an ORE from a Wii disc decrypts its partitions, which takes the Wii
common key. None ships with this crate: [`load_keys`] reads one from a text
file of hex keys, a BootMii `keys.bin`, or an OTP dump. An ORE keeps each
partition's title key, so reading one needs no key, nor does a GameCube disc.

## Scope

- **GameCube, Wii, PlayStation 2, PSP, Xbox and Xbox 360 discs, from an ISO; CDs, from a cue
  sheet whose tracks are a file each**, as Redump lists them; a GD-ROM's
  tracks after its `REM HIGH-DENSITY AREA` addressed from sector 45,000. An ISO is a
  PlayStation 2 DVD when its ISO 9660 `SYSTEM.CNF` boots a `BOOT2`
  executable, a PSP UMD when it holds `UMD_DATA.BIN` or
  `PSP_GAME/PARAM.SFO`, and an Xbox or Xbox 360 disc as Redump dumps one,
  XGD1, XGD2 or XGD3, by its game partition's `MICROSOFT*XBOX*MEDIA` volume
  descriptor, where its kind starts it; an Xbox disc's filler
  regenerated when an early layout tool's 32-bit seed made it, found by
  trying every seed, and otherwise kept as it is: RC4 noise, which no codec
  shrinks; Game Boy, Game Boy Color and Game Boy Advance
  ROMs, by the Nintendo logo or the header checksum their console checks,
  DS ROMs by their logo CRC and header CRC-16, N64 ROMs by their first
  word, in any byte order, and Mega Drive ROMs by their `SEGA`, each kept
  with its file name, its padding, a run of one byte, stored as that byte
  and its length. A ROM named a cartridge's, by `Options::platform` or
  `--platform`, is taken whole, its header not checked: a NES, SNES, PC
  Engine, Master System or Atari ROM, which none says, or a BIOS, the
  caller vouching for it, as a ROM manager does from its DAT. A floppy's
  image named one of `FLOPPY_PLATFORMS`, `amiga`, `fds`, `n64dd` or `st`, an FDS
  or a 64DD disk's, or an IPF, is taken whole the same way, as media class 16.
  A folder named one of those platforms is a game's several ROMs, its disks
  or its chips, one ORE: each file an area of its own, by name, with its own
  hashes, written back as that folder, which `Reader::files` lists.
  Anything else is refused.
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
orers "Game (USA).gba"                  # a ROM, back as a .gba
orers --platform snes "Game (USA).sfc"  # a ROM with no header ORE knows
orers -o out/ "Game (Europe).ore"       # back to its cue sheet, tracks and SBI
orers --store GameCube/.ore-store -o GameCube/ *.iso   # thin OREs, chunks in the store
orers --detach -o out/ GameCube/game.ore               # a standalone ORE again
orers GameCube/.ore-store               # check the store, repairing what parity covers
orers --compact GameCube/.ore-store       # and compact it, dropping what no game uses
orers --compact --recompress GB/.ore-store  # and pack every group again, the smallest it gets
```

| Flag | Meaning |
| --- | --- |
| `-o DIR` | output directory (default: next to each input) |
| `-p PROFILE` | `stream` (default): 1 MiB groups, quick enough to play from; `archive`: 4 MiB groups, smaller, to keep rather than play, as a read away from the last decodes a whole group |
| `-c CODEC` | `zstd` or `lzma2`; by default `lzma2` for a cartridge's ROM or a floppy's image, which Zstandard loses to 7z on, and `zstd` otherwise, a 3DS card's included |
| `--platform NAME` | the cartridge the inputs are ROMs of, or the floppy they are images of (`amiga`, `fds`, `n64dd`, `st`), taken whole, their headers not checked: `32x`, `3ds`, `a2600`, `a5200`, `a7800`, `colecovision`, `dsi`, `gb`, `gba`, `gbc`, `gg`, `jaguar`, `lynx`, `md`, `n64`, `nds`, `nes`, `ngp`, `ngpc`, `odyssey2`, `pce`, `pokemini`, `satellaview`, `sg1000`, `sgx`, `sms`, `snes`, `sufami`, `vb`, `ws`, `wsc` (default: recognised by their headers) |
| `--store DIR` | write ISOs as thin OREs, their chunks in the store `DIR`, made if missing; attach OREs to it; every input as one write, committed after the last |
| `--detach` | write OREs as standalone OREs |
| `--parity N` | parity shards a store's segment gets, 0 to 16 against its 239 data shards: a store made with it, or an existing one set to it, its segments sealed otherwise written again by `--compact` (default: 16 for a store made) |
| `--compact` | once a store is checked, compact it, dropping what no game uses |
| `--recompress` | with `--compact`, decode and pack every group again: the smallest store, for a read and a write of all of it |
| `--keys FILE` | where the Wii common key is |

An ORE written from an ORE needs `-o`, so it lands beside its input rather
than over it. A store given as input is checked and repaired, and fails if anything is
past repair, naming the games it harms. A compaction copies what is left of
segments half dead or more.

Like every oxyROMon tool, `orers` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed.

## Verification

`tests/roundtrip.rs` round-trips a synthetic GameCube disc and a synthetic
Wii disc, a hash exception included, through every profile and codec, reads
at offsets across areas, and flips bytes of a group, the index and the
metadata to check each is caught. `tests/store.rs` writes discs sharing a
file to a store, checks it is stored once, attaches, detaches and compacts,
damages the index and segments, and repairs them; the parity is checked
against scattered damage, a burst as long as the parity, a row past it,
and a segment cut short. `tests/cd.rs` builds a CD of every kind of
sector PlayStation discs hold, subheader copies that differ and a sector
nothing generates among them, with two audio tracks and an SBI, and
round-trips it as its files, alone and through a store, detached,
attached and compacted, its XA sectors through the `xa` codec, which
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
crate, shared with chd-rs. This crate is never published, so it
declares their sources, and gcwii's synthetic discs for its tests, where they
are, rather than holding copies.

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

## Features

- **`write`** (default): compressing, attaching, detaching and compacting,
  and the GameCube and Wii disc model they need. Without it, ore-rs reads
  OREs and stores, and checks and repairs a store, and nothing else: the
  build smelt, the reader library emulators link, takes.
- **`cli`** (default, implies `write`): `orers`.

## License

Each file says its license in an SPDX header. What ore-rs builds without
`write`, its reader, is MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)), with the CD sector model it embeds,
whose ECC is ported from MAME, under BSD-3-Clause
([LICENSE-BSD-3-Clause](LICENSE-BSD-3-Clause)): all permissive, so an
emulator of any license can link it, as smelt does, keeping those notices; the writer, the CLI, and the GameCube and Wii disc
model ported from Dolphin for them, are GPL-3.0-or-later and
GPL-2.0-or-later, so the default build is GPL-3.0-or-later as a whole
([LICENSE](LICENSE)).

The specification, [SPEC.md](SPEC.md), is CC BY 4.0, and its conformance
vectors, [`spec/vectors`](spec/vectors), are CC0 1.0: implement ORE, and
copy the vectors into your tests, under any license.
