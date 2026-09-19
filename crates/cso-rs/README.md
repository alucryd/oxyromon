# cso-rs

CSO and ZSO compression and decompression in Rust — a port of
[maxcso](https://github.com/mattlewis92/maxcso)'s core, without the GUI and
without the formats nobody uses. Built to replace the `maxcso` subprocess in
[oxyromon](https://github.com/alucryd/oxyromon).

```rust
use std::path::Path;
use cso_rs::{CompressOptions, DecompressOptions, Format};

let (iso, cso, zso) = (Path::new("game.iso"), Path::new("game.cso"), Path::new("game.zso"));

cso_rs::compress(iso, cso, &CompressOptions::new(Format::Cso), &mut |_| {}).unwrap();
cso_rs::compress(iso, zso, &CompressOptions::new(Format::Zso), &mut |_| {}).unwrap();
cso_rs::decompress(cso, Path::new("out.iso"), &DecompressOptions::default(), &mut |_| {}).unwrap();
```

## The format

CSO and ZSO share one container and differ in two bytes: the magic, and which
codec a compressed block holds.

```
offset  size   field
0       4      magic: "CISO" (cso) or "ZISO" (zso)
4       4      header size (24)
8       8      uncompressed size
16      4      block size
20      1      version
21      1      index shift
22      2      unused
24      4n     index: one u32 per block, plus a terminating entry
        ...    block data
```

An index entry is `(offset >> shift) | flag`. The high bit
(`0x8000_0000`) means the block is stored uncompressed; the low 31 bits
hold the offset, scaled down by `index_shift`. A CSO block is raw DEFLATE
(windowBits 15, no zlib wrapper); a ZSO block is a raw LZ4 block.

## Scope

Deliberately narrow, so it is worth trusting:

- **CSO v1 and ZSO only.** CSO v2 and DAX are not supported in either
  direction and are rejected on read rather than misread.
- **Compression takes a raw ISO.** There is no direct cso→zso transcode;
  decompress, then compress.
- **No checksums.** maxcso's optional CRC of the decompressed stream is not
  ported.

## Compressors

maxcso compresses every block several ways and keeps the winner under a cost
allowance, because paying CPU on every read to save a handful of bytes is a
bad trade. This crate reproduces that selection, including the rule that
reverts a block to stored when the alignment padding would swallow the
savings, and the tie-break that favours LZ4 because it decodes faster.

| Method | Backend | Default |
| --- | --- | --- |
| zlib, level 9, all four strategies | `libz-sys` | cso |
| libdeflate, level 12 | `libdeflater` | off |
| Zopfli | `zopfli` | off |
| LZ4 fast | `lz4-sys` | zso |
| LZ4 HC, level 16 | `lz4-sys` | zso |

Two notes on the backends, both forced by what the crates actually expose:

- **`libz-sys` rather than `flate2`.** CSO's quality comes from trying
  zlib's FILTERED, HUFFMAN_ONLY and RLE strategies per block, and `flate2`
  exposes no strategy control at all. Going to the raw FFI also keeps the
  output byte-comparable with maxcso's zlib trials.
- **A hand-written bounded LZ4 decoder** (`src/lz4.rs`) rather than
  `lz4_flex`. When `index_shift > 0` a block's stored span is
  `next_offset - offset`, which includes the alignment padding after it.
  `LZ4_decompress_safe_partial` handles that, but `lz4-sys` does not
  export it and `lz4_flex`'s loop is input-driven, so trailing padding makes
  it fail outright. The decoder here stops when the output buffer is full
  and ignores the rest, which is the same contract.

## CLI

`csors` mirrors the maxcso flags that matter, including the
`--use-`/`--no-` toggles, which apply on top of the format's default set.

```
csors --format=cso --block=2048 game.iso -o game.cso
csors --format=zso game.iso -o game.zso
csors --decompress game.cso -o game.iso
csors --format=cso --use-zopfli game.iso -o game.cso   # slower, smaller
```

The format is inferred from the output extension when `--format` is omitted.
Block size defaults to 2048, or 16384 for inputs of 2 GiB or more, as
maxcso does.

## Verification

The tests that matter are the ones against the reference binary: a CSO is
only worth producing if the tools that consume it can read it back.

`tests/interop.rs` runs maxcso in both directions — our output through
`maxcso --decompress`, and maxcso's output through us — and requires a
byte-identical image. It is skipped unless a maxcso binary is reachable
through `$MAXCSO` or at `../maxcso/maxcso`:

```sh
MAXCSO=/path/to/maxcso cargo test
```

Verified against maxcso 1.13.0 on images of 1, 5 and 20 MiB of mixed
content, at block sizes 2048, 4096, 16384, 65536 and 262144, with every
method combination, and on 2.5 GiB and 4.5 GiB images that exercise
`index_shift` of 1 and 2 — the padding cases where a naive LZ4 decoder
breaks. ZSO output is byte-identical to maxcso at the default settings.

CSO output lands about 0.1–0.3% larger than maxcso's. maxcso also trials
7-zip's deflate, which has no Rust port; everything else matches. ZSO is
equal or slightly smaller at larger block sizes.

## License

ISC, same as maxcso. See [LICENSE](LICENSE).
