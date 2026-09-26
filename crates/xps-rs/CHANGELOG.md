# 0.1.0

## Features

- Added IPS patch application, with the truncation extension and Flips' warnings for a patch most likely not meant for its source, already applied, or scrambled, byte-identical to `flips --apply`
- Added BPS patch application, checking the patch, then the source's size and CRC before writing anything, then the output, telling a source that is the patch's output already apart from a wrong one
- Added streaming throughout: the source is read where the patch points and BPS target copies read back what was written, so neither file is held in memory
- Added hardening against malformed patches: every length, offset and distance is bounds-checked, so a bad patch fails instead of panicking
- Added a progress callback to the library API, reporting patch bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `xpsrs` CLI, which applies each patch to one source and names its output after the patch, as many as given, in oxyROMon's look, refusing to write over another output, a patch or the source; it sits behind the default `cli` feature
