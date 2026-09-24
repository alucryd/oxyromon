# 0.1.0

## Features

- Added CUE/BIN to GDI conversion for Dreamcast GD-ROMs dumped by Redump, laid out as gdidrop does: pregaps dropped, the high-density area from sector 45000
- Added CUEs with several tracks in one BIN, converted as if Redump had split it, which gdidrop cannot do
- Added checks before anything is written: every INDEX within its track's part of the BIN, and no track overwriting a BIN it reads
- Added a progress callback to the library API, reporting input bytes consumed, and `input_size` to size it
- Added the removal of partial output on failure
- Added the `gdirs` CLI, which writes each GDI set to a folder of its own, as many as given, in oxyROMon's look; it sits behind the default `cli` feature
