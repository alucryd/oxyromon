# cso-rs

CSO and ZSO compression and decompression in Rust — a port of
[maxcso](https://github.com/unknownbrackets/maxcso)'s core, without the GUI and
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

## Encoders

Each format has one encoder:

| Format | Encoder | Backend |
| --- | --- | --- |
| CSO | zlib, level 9, best of its four strategies | `libz-sys` |
| ZSO | LZ4 HC, level 16 | `lz4-sys` |

Every block is stored raw unless compressing it saves space even after the
alignment padding, as maxcso does: otherwise every read would pay for
decompression, for nothing.

**CSO: zlib, because the rest don't load.** libdeflate and Zopfli both
compress better (on Patapon, 0.3% and 0.6% smaller than zlib, Zopfli even
beating maxcso), but a PSP running ARK-5 could not load into a level from
either, while the zlib CSO played fine. maxcso's own default adds 7-Zip's
deflate, which has no Rust port; without it, CSOs come out 0.4% (Patapon) to
1.7% (a sample of system libraries) larger than maxcso's. The CSO output is
byte-identical to `maxcso --only-zlib`.

**ZSO: LZ4 HC 16, because nothing beats it.** On its own it matched every LZ4
setting combined, fast LZ4 and HC at levels 4 to 13 included, and it decodes
as fast as plain LZ4: on an ARK-5 PSP, both HC and fast LZ4 ZSOs played fine.
It is 2.5% (Patapon) to 3.7% smaller than maxcso's default ZSO, which is fast
LZ4 alone: maxcso only runs HC in brute-force mode.

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

`csors` takes the maxcso flags oxyromon used, so it can stand in for maxcso
there.

```
cargo build --release -p cso-rs
csors --format=cso --block=2048 game.iso -o game.cso
csors --format=zso game.iso -o game.zso
csors --decompress game.cso -o game.iso
```

The format is inferred from the output extension when `--format` is omitted.

## Block sizes

Larger blocks compress better but make every read decompress more, and not
every reader takes them, so the defaults follow the readers:

- **CSO: 8 KiB**, 3.8% smaller than 2 KiB on Patapon; 16 KiB saves only 0.7
  points more. An ARK-5 PSP plays 8 KiB CSOs, PPSSPP reads larger blocks, and
  PCSX2 reads any power of two. From 2 GiB, where only PS2 DVDs are, it is
  16 KiB, as in maxcso.
- **ZSO: 2 KiB**, whatever the size. Open PS2 Loader hard-codes 2 KiB blocks
  and silently misreads anything else. (An ARK-5 PSP and PCSX2 do read larger
  ZSO blocks, if you ask for them with `--block`.)

## Verification

`tests/interop.rs` checks the crate against the reference binary, which is
what matters: a CSO is only worth producing if the tools that consume it can
read it back.

- Each tool decompresses the other's CSO and ZSO back to the original image.
- Our CSO is byte-identical to `maxcso --only-zlib`. The fixture is
  pseudo-text, on which the zlib strategies often tie, so a change in trial
  order fails the test.
- An ignored test round-trips a sparse 2 GiB image, where `index_shift` is 1
  and every block is padded, through both decoders.

They are skipped unless a maxcso binary is reachable through `$MAXCSO` or
`$PATH`:

```sh
cargo test -p cso-rs
cargo test --release -p cso-rs -- --ignored   # the 2 GiB image, ~35 s
```

## License

ISC, same as maxcso. See [LICENSE](LICENSE).
