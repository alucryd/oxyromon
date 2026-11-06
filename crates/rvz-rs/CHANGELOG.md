# 0.1.0

## Features

- Added RVZ decompression of GameCube and Wii discs, Wii partitions re-encrypted and their hashes recomputed, byte-identical to dolphin-tool's
- Added `Reader`, which reads an RVZ as the disc it holds through `Read` and `Seek`, to hash it without writing it out
- Added RVZ compression from a raw disc or another RVZ, with Zstandard, bzip2, LZMA, LZMA2 or none, in chunks from 32 KiB to 2 MiB or of any multiple of 2 MiB: Wii partitions stored decrypted with only the hash exceptions recomputing would not give back, padding stored as its seed, and identical chunks once. With Zstandard, bzip2 or none, the output is dolphin-tool's byte for byte; with LZMA or LZMA2, each reads the other's
- Added scrubbing, zeroing the clusters nothing on the disc points to as it compresses, byte-identical to `dolphin-tool convert --scrub`
- Added Wii common keys as runtime input, read from text, a BootMii `keys.bin` or an OTP dump, every key given tried; none ships with the crate. An RVZ keeps its title keys, so reading or rewriting one needs none
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `rvzrs` CLI, which compresses ISOs and decompresses RVZs, as many as given, or rewrites RVZs with `--format rvz`, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature
