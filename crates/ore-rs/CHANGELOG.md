# 0.1.0

## Features

- Added ORE compression of GameCube and Wii discs, bit-exact: Wii partitions stored decrypted and without their hashes, those recomputing would not give back kept as exceptions, and each group checked to encrypt back to what the disc holds; padding stored as its seed and long runs of zeros as their length; the rest cut into MinCDC chunks of 8 to 24 KiB, each stored once, and compressed in groups of 1 MiB (`stream`) or 4 MiB (`archive`) with Zstandard 19 or LZMA2
- Added the disc's CRC-32, MD5, SHA-1 and BLAKE3, recorded when it is written, so it can be checked against a DAT without decoding it
- Added ORE decompression, which checks the disc against the BLAKE3 it was written with, and `Reader`, which reads an ORE as the disc it holds, at any offset
- Added Wii common keys as runtime input, read from text, a BootMii `keys.bin` or an OTP dump; none ships with the crate, and an ORE keeps its title keys, so reading one needs none
- Added stores: a system's `.ore-store`, holding the chunks of every disc written to it once, in segments under 4 GiB by default, each game a thin ORE naming its chunks there and finding its store as the nearest one up from its folder; chunk ids of 32 bits by default, or 64
- Added Reed-Solomon parity to every store segment, RS(255,239) by default, with a CRC-32C a 4 KiB block, and `check_store`, which checks a store whole, rebuilds what is damaged in place, decodes every group and hashes every chunk, and names the games using what is past repair; a segment is written by one writer and sealed when done, never added to again
- Added `attach` and `detach`, moving a game into a store and out of it, its disc, hashes and metadata as they were, and `purge`, counting again which chunks the games use and copying what is left of mostly dead segments, and of small ones, into new ones
- Added `Reader::sizes`, what a game weighs: its own chunks, its share of those it shares, and about what it would weigh detached; and `Reader::inspect`, a thin ORE's identity without its store
- Added CDs, PlayStation's first: a cue sheet and a BIN a track compressed with each data sector's sync, address, EDC and ECC generated rather than stored, a sector that does not fit the standard kept whole, its audio tracks in FLAC in 8-sector frames, every track's, the cue sheet's and the SBI's hashes recorded under their names; decompressed back to those files, and an SBI beside the cue sheet kept and written back
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `orers` CLI, which compresses ISOs and cue sheets, alone or into a store, decompresses, attaches and detaches OREs, and checks, repairs and purges stores, as many as given, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature
