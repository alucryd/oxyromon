# nsz-rs

A Rust port of [nsz](https://github.com/nicoboss/nsz): lossless zstd compression
of Nintendo Switch NSP dumps into NSZ, and back.

Built as a library for [oxyromon](https://github.com/alucryd/oxyromon), with a
small `nszrs` CLI that mirrors the subset of `nsz` flags oxyromon uses.

## Status

| Feature                          | Supported |
| -------------------------------- | --------- |
| NSZ → NSP decompression          | yes       |
| NSP → NSZ, solid                 | yes       |
| NSP → NSZ, block (parallel)      | yes       |
| XCI ↔ XCZ                        | no        |
| Titlekeys from `.tik` in the NSP | yes       |
| BKTR (update) section splitting  | yes       |
| SHA-256 verification vs. CNMT    | yes       |

NCZ files produced by nsz-rs use the same on-disk format as nsz and can be
decompressed by either tool. As in nsz, only Program and PublicData NCAs whose
sections tile the file are compressed; everything else is copied verbatim.
Both directions stream one member at a time, so memory use doesn't grow with
the dump size, and a failed run removes its partial output.

`nszrs` shows a progress bar per file when stderr is a terminal.
`nszrs -D` verifies every NCA against the CNMT and fails on a mismatch.
Compressing a rights-managed NCA without its ticket (or a `title.keys` entry)
is an error rather than a silent uncompressed copy.

## CLI

```
cargo build --release -p nsz-rs
nszrs -D -F -o out/ game.nsz          # decompress
nszrs -C -L -o out/ game.nsp          # solid compress, long-distance matching
nszrs -C -B -s 20 -o out/ game.nsp    # block compress, 1 MiB blocks
```

| Flag                   | Meaning                                        |
| ---------------------- | ---------------------------------------------- |
| `-C` / `-D`            | compress / decompress                          |
| `-F`                   | re-pad the PFS0 header to 0x20 alignment       |
| `-L`                   | zstd long-distance matching                    |
| `-S` / `-B`            | solid (default) / block stream                 |
| `-l N`                 | zstd level (default 18)                        |
| `-s N`                 | block size exponent, 14..=32 (default 20)      |
| `-o DIR`               | output directory (default: next to the input)  |
| `-k PATH`              | `prod.keys` (default `~/.switch/prod.keys`)    |
| `-x`                   | skip CRC32 check of known keys                 |
| `-K`, `-P`             | accepted for `nsz` compatibility, no-op        |

## Library

The CLI and its progress bar sit behind the default `cli` feature; embed the
library without them:

```toml
nsz-rs = { version = "0.2", default-features = false }
```

```rust
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{Compression, compress_nsp, decompress_nsz};

// Keys are passed as a loader, called only if the container needs them.
let keys = || Keys::load("prod.keys", true);
let solid = Compression { level: 18, ldm: true, block_size_exponent: None };
// The last argument receives input bytes consumed; the calls add up to the
// input file size, ready to feed a progress bar.
compress_nsp("game.nsp".as_ref(), "game.nsz".as_ref(), keys, &solid, false, &mut |_| {})?;
// keys, fix_padding, verify, strict, progress
let report = decompress_nsz(
    "game.nsz".as_ref(), "game.nsp".as_ref(), keys, false, true, true, &mut |n| bar.inc(n),
)?;
```

You need your own `prod.keys` dumped from your own console, but only to
compress NCAs or to verify against a CNMT: containers without NCAs (homebrew,
for instance) and unverified decompression never load it.

## Credits

nsz-rs is a port of [nsz](https://github.com/nicoboss/nsz) by
[Nico Bosshard](https://github.com/nicoboss), which itself builds on NUT by
[Blake Warner](https://github.com/blawar). The NCZ format, the key derivation
and the container handling all come from their work; this crate only
re-implements it in Rust.

## License

MIT, see [LICENSE](LICENSE). The original nsz copyright notice is retained as
the license requires.
