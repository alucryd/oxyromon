# 0.1.0

## Features

- Added CUE/BIN to GDI conversion for Dreamcast GD-ROMs dumped by Redump, laid out as gdidrop does: pregaps dropped, the high-density area from sector 45000
- Added CUEs with several tracks in one BIN, converted as if Redump had split it, which gdidrop cannot do
- Added checks before anything is written: every track's INDEX 01, found by its number, and every INDEX within its track's part of the BIN, and no track overwriting a BIN it reads
- Added a progress callback to the library API, reporting input bytes consumed, and `input_size` to size it
- Added writing every file to `<name>.part`, all renamed into place once complete, so a failed run leaves an existing set alone and nothing else behind
- Added the `gdirs` CLI, which writes each GDI set to a folder of its own, as many as given, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature
