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
  v1 and v2 are not planned. Every codec below decodes, except `avhu`, for
  LaserDiscs. CDs and LaserDiscs open, list their metadata and verify; only
  their frame-accurate extraction is missing, and it fails rather than write
  something wrong.
- **Writing** emits CHD v5, like `chdman`: hard disks and DVDs from a raw
  image, with clone CHDs that store only what changed against a parent. CD
  writing — the frame layout, the CD metadata and the CD codecs on the write
  path — and LaserDisc writing are not implemented yet.
- **Codecs** are tried per hunk, the shortest result winning: `none`, `flac`,
  `huff`, `lzma`, `zlib` and `zstd` for DVDs and hard disks, and their
  sector-interleaved siblings `none`, `cdfl`, `cdlz`, `cdzl` and `cdzs` for
  CDs. `avhu`, for LaserDiscs, is not ported yet.
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
| `zlib` / `cdzl`              | `flate2`            | `flate2`, level 9    |
| `zstd` / `cdzs`              | `zstd`              | `zstd`, level 22     |
| `lzma` / `cdlz`              | `lzma-rs`           | `liblzma-sys`        |
| `huff`                       | built-in            | built-in             |
| `flac` / `cdfl`              | `libflac-sys`       | `libflac-sys`        |
| `avhu`                       | not ported          | not ported           |

Native libraries, not reimplementations, for the codecs that have one: they
are what `chdman` links, so the bitstreams match by construction, and the
byte-for-byte interop tests hold without chasing encoder minutiae.
`liblzma-sys` is built and linked statically; `libflac-sys` builds its
vendored libFLAC with CMake, so a CMake toolchain is needed to build chd-rs.

## CLI

`chdrs` compresses images to CHDs and extracts CHDs back, telling which from
each file's extension, and writes next to each one unless told otherwise:

```
cargo build --release -p chd-rs
chdrs game.bin                  # to game.chd, next to it
chdrs -c zlib,lzma -o out/ *.bin
chdrs game.chd                  # back to game.iso
chdrs -p base.chd clone.bin     # a clone, storing only what changed
```

| Flag        | Meaning                                                        |
| ----------- | -------------------------------------------------------------- |
| `-o DIR`    | output directory (default: next to each input)                 |
| `-b N`      | hunk size in bytes, 16 to 1048576 (default: 4096)              |
| `-c CODECS` | comma-separated codecs to try per hunk, best wins (default: zlib) |
| `-p CHD`    | parent CHD, to read a clone from or to write one against       |

Two commands read a CHD directly: `chdrs info <CHD>` prints its contents and
`chdrs verify <CHD>` checks its checksums. Both report to stdout only, without
`chdman`'s banner or carriage-return progress, and on a mismatch `verify`
prints the two hashes to stderr and exits non-zero, where `chdman` exits
successfully. CD inputs — `.cue` and `.gdi` — fail until CD writing lands.

Like every oxyROMon tool, `chdrs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another input, or over an earlier
input's output.

## Verification

`tests/cli.rs` and `tests/write.rs` check the crate against `chdman` itself,
which is what matters: a CHD is only worth producing if MAME and `chdman` can
read it, and oxyROMon has to keep reading the CHDs `chdman` wrote. Each tool
verifies and extracts the other's files back to the original image, for plain
CHDs and for clones against a parent. The tests skip rather than fail when no
`chdman` binary is around (`$CHDMAN`, then `$PATH`).

## Credits

chd-rs began as a port of `chdman` and the CHD support code of
[MAME](https://github.com/mamedev/mame), by Aaron Giles and the MAME
contributors. The container, the hunk maps, the codec set and the CD sector
model all come from their work.

## License

BSD-3-Clause, same as `chdman`. See [LICENSE](LICENSE).
