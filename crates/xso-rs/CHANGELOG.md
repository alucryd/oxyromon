# 0.1.0

## Features

- Added CSO v1 and ZSO compression from a raw ISO, and decompression back to it
- Added parallel block compression and decompression over rayon, with positional reads and a single ordered writer
- Added maxcso's rule that stores a block raw unless compressing it saves space even after alignment padding
- Added `index_shift` handling for images past 2 GiB, with a bounded LZ4 block decoder that tolerates the padding between blocks
- Added one encoder per format: zlib for CSO, trying all four of its strategies in maxcso's order, so the output is byte-identical to `maxcso --only-zlib`; and LZ4 HC at level 16 for ZSO. libdeflate and Zopfli compress CSO blocks better, but an ARK-5 PSP could not play games compressed with either
- Added block size defaults that follow the readers: 8 KiB for CSO (16 KiB from 2 GiB), which PSP CFW, PPSSPP and PCSX2 read; 2 KiB for ZSO, the only size Open PS2 Loader reads
- Added a progress callback to the library API, reporting input bytes consumed
- Added the removal of partial output on failure, in both directions
- Added the `xsors` CLI, which compresses ISOs and decompresses CSOs and ZSOs, as many as given, in oxyROMon's look; it sits behind the default `cli` feature
