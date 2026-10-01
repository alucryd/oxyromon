# chd-rs

CHD compression, extraction and inspection in Rust. Part of
[oxyROMon](https://github.com/alucryd/oxyromon), as a library and the `chdrs`
CLI, and interchangeable with the files of
[`chdman`](https://github.com/mamedev/mame/blob/master/src/tools/chdman.cpp),
which it began as a port of (see [Credits](#credits)).

A CHD holds the logical contents of a hard disk, a DVD, a CD or a LaserDisc,
split into hunks, each compressed with one of several codecs and covered by
hashes, with metadata describing the media.

```rust
use std::path::Path;
use chd_rs::{Chd, create};

let (img, chd) = (Path::new("game.bin"), Path::new("game.chd"));

// 512-byte units, 4 KiB hunks, hunk by hunk the zlib encoding wins.
create(img, chd, 512, 4096, [u32::from_be_bytes(*b"zlib"), 0, 0, 0], None, &[], &mut |_| {}).unwrap();

let mut chd = Chd::open(chd).unwrap();
chd.verify().unwrap();
chd.extract(Path::new("game.iso"), &mut |_| {}).unwrap();
```

## Scope

The port lands in steps, and this section says how far it has come:

- **Reading** covers CHD v3, v4 and v5, the versions `chdman` still writes;
  v1 and v2 are not planned. Every codec below decodes. Hard disks and DVDs
  extract to raw images with `Chd::extract`, CDs to a CUE, a GDI or a
  cdrdao TOC and their BINs with `extract_cd`, the way `chdman extractcd`
  names and lays them out, and LaserDiscs to an AVI with `extract_ld`, as
  `chdman extractld` writes it.
- **Writing** emits CHD v5, like `chdman`: hard disks (`create_hd`) and DVDs
  (`create_dvd`) from a raw image, CDs (`create_cd`) from a CUE sheet, a GDI
  or an ISO, LaserDiscs (`create_ld`) from an AVI of YUY2, UYVY, VYUY or
  left-predicted HuffYUV video and PCM audio, all with clone CHDs that store
  only what changed against a parent.
- **Compatibility** is with `chdman` 0.289: CUE and GDI parsing, the CD
  frame layout and metadata, and extraction follow it rather than later
  MAME, which has since changed GDI pregaps and session metadata and added
  CD+G tracks.
- **Codecs** are tried per hunk, the shortest result winning: `none`, `flac`,
  `huff`, `lzma`, `zlib` and `zstd` for DVDs and hard disks, their
  sector-interleaved siblings `none`, `cdfl`, `cdlz`, `cdzl` and `cdzs` for
  CDs, and `avhu`, audio as FLAC and video as Huffman-coded deltas, for
  LaserDiscs.
- Parent/clone CHDs are read and written through the `--parent` option and
  the `parentsha1` header field.
- `chdman`'s `copy`, `addmeta`, `delmeta`, `dumpmeta`, `listtemplates` and
  CHS geometry options are out of scope.

## Encoders

Per hunk, every codec named on the command line is tried in turn and the
shortest output kept, storing the hunk raw when none of them saves space, as
`chdman` does.

| Codec                        | Reading             | Writing              |
| ---------------------------- | ------------------- | -------------------- |
| `none`                       | built-in            | built-in             |
| `zlib` / `cdzl`              | `flate2`            | `flate2` (`zlib-rs`), level 9 |
| `zstd` / `cdzs`              | `zstd`              | `zstd`, level 22     |
| `lzma` / `cdlz`              | `lzma-rs`           | `lzma-sdk-rs`        |
| `huff`                       | built-in            | built-in             |
| `flac` / `cdfl`              | `libflac-sys`       | `libflac-sys`        |
| `avhu`                       | built-in, `libflac-sys` | built-in, `libflac-sys` |

The encoders are those `chdman` uses, or byte-exact ports of them, so the
bitstreams match `chdman`'s by construction: LZMA goes through
`lzma-sdk-rs`, a port of the LZMA SDK 23.01 encoder MAME bundles (liblzma,
given the same parameters, makes slightly different choices). The one
caveat is deflate: `flate2`'s `zlib-rs` backend is a port of zlib-ng, so its
output is that of a `chdman` linking zlib-ng, not of one linking classic
zlib. Either way the data, and the hashes in the header that cover it, are
the same.

`libflac-sys` builds its vendored libFLAC with CMake, so a CMake toolchain is
needed to build chd-rs.

## CLI

`chdrs` compresses images to CHDs and extracts CHDs back, telling which from
each file's extension, and writes next to each one unless told otherwise. A
`.cue` or `.gdi` becomes a CD, an `.iso` a DVD, an `.avi` a LaserDisc,
anything else a hard disk; a CD CHD extracts to a CUE and its BIN, a BIN per
track for a GD-ROM, a LaserDisc to an AVI, and any other CHD to a raw image:

```
cargo build --release -p chd-rs
chdrs game.cue                  # to game.chd, next to it
chdrs -c zlib,lzma -o out/ *.bin
chdrs game.chd                  # back to game.cue and game.bin, or game.iso
chdrs -p base.chd clone.bin     # a clone, storing only what changed
```

| Flag        | Meaning                                                        |
| ----------- | -------------------------------------------------------------- |
| `-o DIR`    | output directory (default: next to each input)                 |
| `-b N`      | hunk size in bytes, 16 to 1048576 (default: 19584, eight frames, for CDs, 4096 otherwise) |
| `-c CODECS` | comma-separated codecs to try per hunk, best wins (default: `cdlz,cdzl,cdfl` for CDs, `zlib` otherwise) |
| `-p CHD`    | parent CHD, to read a clone from or to write one against       |

Two commands read a CHD directly: `chdrs info <CHD>` prints its contents and
`chdrs verify <CHD>` checks its checksums. Both report to stdout only, without
`chdman`'s banner or carriage-return progress, and on a mismatch `verify`
prints the two hashes to stderr and exits non-zero, where `chdman` exits
successfully.

Like every oxyROMon tool, `chdrs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another input, or over an earlier
input's output.

## Verification

`tests/cli.rs`, `tests/write.rs`, `tests/cd.rs` and `tests/ld.rs` check the crate against
`chdman` itself, which is what matters: a CHD is only worth producing if MAME
and `chdman` can read it, and oxyROMon has to keep reading the CHDs `chdman`
wrote. Each tool verifies and extracts the other's files back to the
original image, for plain CHDs and for clones against a parent, and the CHDs
chd-rs writes — hard disks, DVDs, CDs from CUE, GDI and ISO inputs, and
LaserDiscs from AVIs of every layout `chdman` reads, made with `ffmpeg` — are
compared with `chdman`'s byte for byte, as are the CUEs, GDIs, TOCs, BINs and
AVIs it extracts, down to the bytes `chdman` leaves past the end of a partly
filled last hunk, which come from its never-cleared work buffer. Deflate,
whose bytes depend on the zlib `chdman` links (see [Encoders](#encoders)),
is compared by the header's hashes instead, and byte for byte too with
`CHDRS_STRICT_PARITY=1`, against a `chdman` linking zlib-ng. The tests skip rather than fail when no `chdman` binary is
around (`$CHDMAN`, then `$PATH`).

## Credits

chd-rs began as a port of `chdman` and the CHD support code of
[MAME](https://github.com/mamedev/mame), by Aaron Giles and the MAME
contributors. The container, the hunk maps, the codec set and the CD sector
model all come from their work.

## License

BSD-3-Clause, same as `chdman`. See [LICENSE](LICENSE).
