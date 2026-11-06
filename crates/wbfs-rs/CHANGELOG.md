# 0.1.0

## Features

- Added WBFS creation from a raw GameCube or Wii disc, one disc a file in 2 MiB blocks, only the blocks holding something stored and the clusters nothing points to zeroed, byte-identical to `wit copy --wbfs`, single- and dual-layer discs alike, but for a leftover table of free blocks wit leaves and never reads
- A disc without a boot DOL, as Datel's discs and homebrew loaders such as Swiss are, is kept whole: its own apploader loads code from where no file points, which wit zeroes
- Added WBFS extraction, byte-identical to wit's for a Wii disc; a GameCube disc comes back as long as a GameCube disc, rather than padded as a Wii one; another version of the format, or a WBFS partition holding several discs, is refused rather than read in part
- Added Wii common keys as runtime input, read from text, a BootMii `keys.bin` or an OTP dump, every key given tried; none ships with the crate
- Added `create_from`, writing a WBFS from any `Source`, which can hand over each partition's title key itself, as an RVZ read through rvz-rs does, so that no common key is needed
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `wbfsrs` CLI, which writes ISOs to WBFS and extracts WBFSs, as many as given, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature
