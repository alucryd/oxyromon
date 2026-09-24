# 0.3.0

## Changes

- Update dependencies to their latest versions
- Move into the [oxyromon](https://github.com/alucryd/oxyromon) repository, under `crates/nsz-rs`
- Require Rust 1.94, as the crate moves to the 2024 edition along with the rest of the oxyromon workspace
- `nszrs` takes the same shape as every oxyROMon tool: compression or decompression comes from each file's extension, so `-C`, `-D`, `-K` and `-P` are gone, and the output directory is `-o`, defaulting to next to each input
- `nszrs` compresses in blocks with `-b SIZE`, in bytes as for `xsors`, in place of `-B`, `-S` and `-s`; `--long-distance`, `--fix-padding` and `--skip-key-check` lose their short forms
- `nszrs` parses its arguments with clap, adding `-V`/`--version`
- `nszrs` draws oxyROMon's progress bar and a result line per file, like every oxyROMon tool
- `nszrs` refuses to write one input's output over another input, or over an earlier input's output
- `nszrs` reports a missing home directory instead of looking for `prod.keys` relative to the current one

## Fixes

- A failed run no longer deletes an existing output: output is written to `<output>.part` and renamed into place once complete

# 0.2.0

## Changes

- `compress_nsp` and `decompress_nsz` now take a key loader (`impl FnOnce() -> Result<Keys>`) instead of `&Keys`, and only call it when keys are actually needed: compressing an NCA, or verifying against a CNMT. Containers without NCAs and unverified decompressions no longer need a `prod.keys`
- `nszrs` loads `prod.keys` per file, only when that file needs it, instead of refusing to start without one
- Errors reading `prod.keys` or `title.keys` now name the file

# 0.1.0

## Features

- Added NSP to NSZ compression, as a solid stream (multithreaded zstd) or independent blocks compressed in parallel
- Added NSZ to NSP decompression, restoring the original NSP byte for byte
- Added SHA-256 verification of decompressed NCAs against the container's CNMTs, including merged NSPs carrying several titles
- Added title key lookup from the NSP's tickets, falling back to `title.keys`
- Added BKTR subsection splitting for update NCAs, so each subsection is decrypted with its own counter before compression
- Added streaming pipelines whose memory use doesn't grow with the dump size, removing partial output on failure
- Added a progress callback to the library API, driven by the compressor's actual progress rather than by reads
- Added the `nszrs` CLI, mirroring nsz's `-C`/`-D` flags, with a progress bar; it sits behind the default `cli` feature, so library users can drop it with `default-features = false`
- Added hardening against malformed input: oversized header counts, truncated containers, and invalid keys fail with an error instead of a panic or a huge allocation
