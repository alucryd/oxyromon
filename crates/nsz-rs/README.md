# nsz-rs

Lossless zstd compression of Nintendo Switch NSP dumps into NSZ, and back, in
Rust. Part of [oxyROMon](https://github.com/alucryd/oxyromon), as a library and
the `nszrs` CLI. Its NSZs are interchangeable with those of
[nsz](https://github.com/nicoboss/nsz), which it began as a port of (see
[Credits](#credits)).

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
the dump size. The output is written to `<output>.part` and renamed into
place once complete, so a failed run leaves an existing output alone.

Compressing a rights-managed NCA without its ticket (or a `title.keys` entry)
is an error rather than a silent uncompressed copy.

## CLI

`nszrs` compresses NSPs and decompresses NSZs, telling which from each file's
extension, and writes next to each one unless told otherwise:

```
cargo build --release -p nsz-rs
nszrs game.nsz                        # decompress, next to it
nszrs --long-distance -o out/ *.nsp   # solid compress, long-distance matching
nszrs -b 1048576 -o out/ game.nsp     # compress in 1 MiB blocks, in parallel
```

| Flag               | Meaning                                                        |
| ------------------ | -------------------------------------------------------------- |
| `-o DIR`           | output directory (default: next to each input)                 |
| `-l N`             | zstd level (default 18)                                        |
| `-b SIZE`          | compress in independent blocks of SIZE bytes, a power of two in 16384..=4294967296, instead of one solid stream |
| `--long-distance`  | zstd long-distance matching                                    |
| `--fix-padding`    | re-pad the PFS0 header to 0x20 alignment                       |
| `-k PATH`          | `prod.keys` (default `~/.switch/prod.keys`)                    |
| `--skip-key-check` | skip the CRC32 check of known keys                             |

Decompression verifies every NCA against the CNMT and fails on a mismatch. Like
every oxyROMon tool, `nszrs` draws oxyROMon's progress bar, reports each input
on a line of its own, and exits non-zero when any of them failed. It refuses to write
one input's output over another input, or over an earlier input's output.

## Library

The CLI and its progress bar sit behind the default `cli` feature; embed the
library without them:

```toml
nsz-rs = { version = "0.3", default-features = false }
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

nsz-rs began as a port of [nsz](https://github.com/nicoboss/nsz) by
[Nico Bosshard](https://github.com/nicoboss), which itself builds on NUT by
[Blake Warner](https://github.com/blawar). The NCZ format, the key derivation
and the container handling all come from their work.

## License

MIT, see [LICENSE](LICENSE). The original nsz copyright notice is retained as
the license requires.
