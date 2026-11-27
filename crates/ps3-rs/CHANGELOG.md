# 0.1.0

## Features

- Added IRD parsing, version 9, as ps3-disc-dumper and 3k3y's tools write them, gzipped or not, checked against its CRC32, with the disc key derived from its data1
- Added rebuilding a Redump ISO from a JB folder and its IRD, byte-identical, checking every file against the IRD's size and MD5 and returning the image's MD5
- Added extracting an ISO to its JB folder with its disc key, returning each file's MD5
- Added reading the firmware version a `PS3UPDAT.PUP` installs
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added the `ps3rs` CLI, which rebuilds each JB folder given and extracts each ISO, as many as given, in oxyROMon's look, refusing to write over another output or an input; it sits behind the default `cli` feature
