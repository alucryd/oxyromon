# 0.1.0

## Features

- Added xdelta3 patch decoding: VCDIFF with the default code table, per-window Adler-32 checks, and LZMA secondary compression decoded as the continuous xz streams xdelta3 writes, byte-identical to `xdelta3 -d`
- Added refusals with a clear error for what xdelta3 only writes when asked (DJW and FGK) or cannot decode itself (the patch's own code tables, windows copying from the target)
- Added hardening against malformed patches: lengths are cross-checked, windows capped at xdelta3's 64 MiB, and decoded sections bounded by their window, so a bad patch fails instead of panicking or allocating wildly
- Added a progress callback to the library API, reporting patch bytes consumed
- Added a `WrongSource` error, shared with xps-rs, for a patch applied to the wrong file
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `xdeltars` CLI, which applies each patch to one source and names its output as the patch records, as many as given, in oxyROMon's look, refusing to write over another output, a patch or the source; it sits behind the default `cli` feature
