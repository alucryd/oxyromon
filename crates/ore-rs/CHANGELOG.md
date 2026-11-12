# 0.1.0

## Features

- Added ORE compression of GameCube and Wii discs, bit-exact: Wii partitions stored decrypted and without their hashes, those recomputing would not give back kept as exceptions, and each group checked to encrypt back to what the disc holds; padding stored as its seed and long runs of zeros as their length; the rest cut into MinCDC chunks of 8 to 24 KiB, each stored once, and compressed in groups of 1 MiB (`stream`) or 4 MiB (`archive`) with Zstandard 19 or LZMA2
- Added the disc's CRC-32, MD5, SHA-1 and BLAKE3, recorded when it is written, so it can be checked against a DAT without decoding it
- Added ORE decompression, which checks the disc against the BLAKE3 it was written with, and `Reader`, which reads an ORE as the disc it holds, at any offset
- Added Wii common keys as runtime input, read from text, a BootMii `keys.bin` or an OTP dump; none ships with the crate, and an ORE keeps its title keys, so reading one needs none
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `orers` CLI, which compresses ISOs and decompresses OREs, as many as given, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature
