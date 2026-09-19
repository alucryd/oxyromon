# 0.2.1

## Changes

- Update dependencies to their latest versions
- Move into the [oxyromon](https://github.com/alucryd/oxyromon) repository, under `crates/nsz-rs`
- Require Rust 1.94, as the crate moves to the 2024 edition along with the rest of the oxyromon workspace
- `nszrs` parses its arguments with clap: the flags are unchanged, `-V`/`--version` is new, and `-h` lists every flag

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
