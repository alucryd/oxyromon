# rvz-rs

RVZ compression and decompression in Rust, for GameCube and Wii discs,
scrubbing included. Part of [oxyROMon](https://github.com/alucryd/oxyromon),
as a library and the `rvzrs` CLI. Its files are interchangeable with those of
[Dolphin](https://dolphin-emu.org), which it is a port of (see
[Credits](#credits)), and the same byte for byte with Zstandard, bzip2 or no
compression.

```rust
use std::path::Path;
use rvz_rs::Options;

let (iso, rvz) = (Path::new("game.iso"), Path::new("game.rvz"));

// A Wii disc needs the Wii common key, which ships with nothing here
let keys = rvz_rs::load_keys(Path::new("keys.bin")).unwrap();
rvz_rs::compress(iso, rvz, &Options::default(), &keys, &mut |_| {}).unwrap();
rvz_rs::decompress(rvz, Path::new("out.iso"), &mut |_| {}).unwrap();
```

## The format

RVZ is Dolphin's successor to wit's WIA, and is described in Dolphin's
[`docs/WiaAndRvz.md`](https://github.com/dolphin-emu/dolphin/blob/master/docs/WiaAndRvz.md).
The disc is cut into chunks, each compressed on its own, so any part of it can
be read without the rest. Three things make it smaller than a plain
compressed image:

- **Wii partitions are stored decrypted and without their hashes.** A Wii
  disc's data is encrypted, which compresses to nothing, and every 32 KiB
  block carries a KiB of hashes. RVZ keeps 31 KiB of decrypted data a block,
  recomputes the hashes when reading, and keeps only those that recomputing
  would not give back, as hash exceptions. Each partition's title key is
  kept to encrypt it again.
- **Padding is stored as its seed.** Discs are padded with the output of a
  lagged Fibonacci generator, which compresses no better than encrypted
  data. RVZ finds the 68-byte seed that generates each run of it, and stores
  that instead.
- **Identical chunks are stored once**, the zeros of a scrubbed disc among
  them.

## Keys

Writing an RVZ from a Wii disc decrypts its partitions, which takes the Wii
common key to decrypt each partition's title key. No key ships with this
crate: [`load_keys`] reads one from a text file of hex keys, a BootMii
`keys.bin`, or a Wii's or a Wii U's OTP dump. Every key given is tried, and
the one that decrypts the partition's data is used.

Nothing else needs a key. An RVZ keeps each partition's title key, so reading
one, hashing one through `Reader`, or rewriting one, to recompress or scrub
it, needs none. Nor does a GameCube disc.

## Scrubbing

Scrubbing zeroes every 32 KiB cluster that nothing on the disc points to:
the padding between files, and what follows the last one. The game plays the
same, and the RVZ is smaller, though by little, since RVZ already stores
padding as a seed. A scrubbed disc is no longer the disc it was, and no
longer matches its dump's hashes.

## Scope

- **RVZ only.** WIA, RVZ's predecessor, is refused rather than misread.
- **GameCube and Wii discs.** Anything else is stored as raw data, as Dolphin
  stores it, and cannot be scrubbed.
- **Compression levels as Dolphin's converter takes them**: 1 to 22 for
  Zstandard, 1 to 9 for the others. Zstandard's negative levels are left out.

## Codecs

| Method | Backend | Same bytes as Dolphin |
| --- | --- | --- |
| none | | yes |
| Zstandard | `zstd` | yes |
| bzip2 | `bzip2`, the pure Rust port of libbzip2 | yes |
| LZMA, LZMA2 | `lzma-rust2` | no, but each reads the other's |

Every chunk is stored uncompressed when compressing it saves nothing, as
Dolphin does. LZMA and LZMA2 are set up from the same presets as Dolphin's
liblzma, but are a different encoder, and come out a little smaller.

## CLI

`rvzrs` compresses ISOs and decompresses RVZs, telling which from each file,
and writes next to each one unless told otherwise:

```
cargo build --release -p rvz-rs
rvzrs game.iso                          # to game.rvz, next to it
rvzrs --keys keys.bin -o out/ *.iso     # Wii discs, to out/
rvzrs game.rvz                          # back to game.iso
rvzrs -f rvz --scrub -o out/ game.rvz   # scrubbed, to out/game.rvz
```

| Flag | Meaning |
| --- | --- |
| `-o DIR` | output directory (default: next to each input) |
| `-f FORMAT` | `iso` or `rvz` (default: RVZ for an ISO, ISO for an RVZ) |
| `-c METHOD` | `none`, `bzip2`, `lzma`, `lzma2` or `zstd` (default) |
| `-l LEVEL` | compression level (default: 5) |
| `-b SIZE` | chunk size in bytes (default: 131072, Dolphin's) |
| `--scrub` | scrub as it compresses |
| `--keys FILE` | where the Wii common key is |

Like every oxyROMon tool, `rvzrs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another input, or over an earlier
input's output.

## Verification

`tests/interop.rs` checks the crate against dolphin-tool:

- Our RVZ of a synthetic GameCube disc is dolphin-tool's byte for byte, with
  Zstandard at several levels, bzip2 and no compression, chunks from 32 KiB to
  4 MiB, scrubbed or not.
- dolphin-tool reads our RVZs of a synthetic Wii disc, encrypted with a
  made-up common key, back to the disc, and each tool reads the other's LZMA
  and LZMA2.
- An ignored test takes a real RVZ from `$RVZ_RS_REAL`: we read it as
  dolphin-tool does, and rewrite it, scrubbed or not, as dolphin-tool does,
  byte for byte. It passed on a GameCube and a Wii disc, as did the same
  comparisons at every setting above.

They are skipped unless dolphin-tool is reachable through `$DOLPHIN_TOOL` or
`$PATH`. `tests/roundtrip.rs` needs nothing: every disc comes back out of every
method and chunk size, a Wii disc's common key is found among others or
refused without it, and an RVZ is rewritten without one.

```sh
cargo test -p rvz-rs
RVZ_RS_REAL=game.rvz cargo test --release -p rvz-rs -- --ignored
```

## Credits

rvz-rs is a port of Dolphin's RVZ code, by the
[Dolphin Emulator Project](https://github.com/dolphin-emu/dolphin): the format,
its writer and reader, the scrubber, and the lagged Fibonacci generator,
whose seed recovery is Dolphin's own.

## License

GPL-2.0-or-later, same as Dolphin. See [LICENSE](LICENSE).
