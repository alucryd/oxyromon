# wbfs-rs

WBFS creation and extraction in Rust, for Wii and GameCube discs. Part of
[oxyROMon](https://github.com/alucryd/oxyromon), as a library and the
`wbfsrs` CLI. Its WBFSs are those of [wit](https://wit.wiimm.de)'s
`wit copy --wbfs`, which it is a port of (see [Credits](#credits)), byte for
byte but for two things below: a leftover no reader looks at, and discs
without a boot DOL, which wit breaks.

```rust
use std::path::Path;

// A Wii disc needs the Wii common key, which ships with nothing here
let keys = wbfs_rs::load_keys(Path::new("keys.bin")).unwrap();
wbfs_rs::create(Path::new("game.iso"), Path::new("game.wbfs"), &keys, &mut |_| {}).unwrap();
wbfs_rs::extract(Path::new("game.wbfs"), Path::new("out.iso"), &mut |_| {}).unwrap();
```

## The format

WBFS is the file system the Wii's USB loaders read games from, and a `.wbfs`
file is that file system holding one disc. The disc is cut into 2 MiB blocks,
and only the blocks holding something are stored, behind a table mapping each
block of the disc to where it is stored.

A block holds something when any of its 32 KiB clusters is used: the disc's
header and boot files, its file system and files, and on a Wii disc each
partition's ticket, TMD, certificates and hashes. wit zeroes the clusters
nothing points to in the blocks it stores, so a WBFS is scrubbed: it plays
the same, but is no longer the disc it was, and no longer matches its dump's
hashes. Which clusters are used is found by a port of Dolphin's scrubber,
which keeps exactly the clusters wit does.

What wit writes holds one more thing: wit lays a new WBFS out for the largest
disc, then trims it to what it holds, and up to 512 bytes of the first table
of free blocks it wrote stay in the file, ahead of the table it reads. Neither
wit nor libwbfs reads them, and `wwt check` passes without them, so wbfs-rs
leaves them zeroed.

A disc without a boot DOL, as Datel's discs and homebrew loaders such as Swiss
are, boots an apploader of its own that loads code from where no file
points. Nothing on it can be told unused, so wbfs-rs keeps it whole, where
wit zeroes all but what its file system lists and the disc no longer boots.

## Keys

Finding what a Wii disc uses decrypts its partitions, which takes the Wii
common key. No key ships with this crate: [`load_keys`] reads one from a text
file of hex keys, a BootMii `keys.bin`, or a Wii's or a Wii U's OTP dump.
Every key given is tried. A GameCube disc needs none, nor does extracting a
WBFS.

`create_from` reads the disc through a `Source`, which can hand over each
partition's title key itself: an RVZ keeps them, so oxyROMon writes a WBFS
from one through rvz-rs's reader, with no key and no ISO decoded on the way.

## Scope

- **One disc a file, 2 MiB blocks, unsplit**, as `wit copy --wbfs` writes
  them. WBFS partitions holding several discs, and the `.wbf1` files wit
  splits a WBFS into for FAT32, are not written or read.
- **GameCube and Wii discs**, single- or dual-layer. Anything else is refused.
- **Extraction pads as wit does**: a Wii disc to the size of a single-layer
  one at least. A GameCube disc comes back as long as a GameCube disc, where
  wit pads it as it pads a Wii one; the bytes they share are the same.

## CLI

`wbfsrs` writes ISOs to WBFS and extracts WBFSs, telling which from each
file, and writes next to each one unless told otherwise:

```
cargo build --release -p wbfs-rs
wbfsrs game.iso                         # a GameCube disc, to game.wbfs
wbfsrs --keys keys.bin -o out/ *.iso    # Wii discs, to out/
wbfsrs game.wbfs                        # back to game.iso
```

| Flag | Meaning |
| --- | --- |
| `-o DIR` | output directory (default: next to each input) |
| `--keys FILE` | where the Wii common key is |

Like every oxyROMon tool, `wbfsrs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another input, or over an earlier
input's output.

## Verification

`tests/interop.rs` checks the crate against wit:

- Our WBFS of a synthetic GameCube disc is wit's byte for byte but for the
  leftover table, and we extract it as wit does.
- An ignored test takes a real RVZ from `$WBFS_RS_REAL`, read through
  rvz-rs so that no key is needed: our WBFS of it is
  wit's of the ISO it holds, byte for byte but for the leftover table, and
  extracts as wit extracts it. It passed on five Wii discs, one of them
  dual-layer, and a GameCube disc.

They are skipped unless wit is reachable through `$WIT` or `$PATH`. wit's
first block is also pinned in unit tests, as it wrote it for two real discs.
`tests/roundtrip.rs` needs nothing: what a disc uses comes back out of its
WBFS, the rest as zeros, a disc without a boot DOL comes back whole, a Wii
disc's common key is refused without it, an
RVZ gives the same WBFS without one, and what is not a disc or a WBFS is
refused. The tests check against the unpublished `gcwii` crate and read RVZs
through rvz-rs, both dev-dependencies, so they are not packaged.

```sh
cargo test -p wbfs-rs
WBFS_RS_REAL=game.rvz cargo test --release -p wbfs-rs -- --ignored
```

## Source layout

The GameCube and Wii disc specifics, partitions, encryption, the file system,
keys and what a disc uses, live in oxyROMon's unpublished `gcwii` crate,
shared with rvz-rs. `src/gcwii` is a symlink to its source, and so is
`tests/common` to its synthetic discs; publishing copies them in, so this
crate stands alone.

## Credits

wbfs-rs is a port of how [wit](https://wit.wiimm.de) writes and reads a WBFS
file, and of the libwbfs it builds on: libwbfs by Kwiirk, and wit and its
libwbfs by Dirk Clemens (Wiimm).

## License

GPL-2.0-or-later, same as wit. See [LICENSE](LICENSE).
