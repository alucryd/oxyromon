# 0.1.0

## Features

- Added WUD to WUX compression, storing each distinct sector once as wudd does, and decompression back to the exact WUD
- Added `Image`, which reads a WUD or a WUX as the same uncompressed bytes, through `Read` and `Seek` or at an offset
- Added WUD and WUX to WUA conversion, the way Cemu converts a disc: the game partition's files, decrypted, in a ZArchive
- Added NUS folders as WUA inputs, the form updates and DLC come in: a game and its update and DLC make one WUA, a folder per title, as Cemu makes them
- Added WUD and WUX to NUS conversion, the folder WUP Installer GX2 installs: the disc's TMD, ticket and certificates, and its contents copied as they are encrypted, with the H3 hashes of hashed ones, each checked against the TMD; the contents are the CDN's byte for byte
- Added revert data to the WUAs it writes, in a `.nus` folder Cemu ignores, and WUA to NUS conversion from it: each title comes back as a NUS folder whose contents are byte for byte those it was made from, checked against the TMD, for under 0.1% of the WUA
- Added lossless WUAs: a WUA written from a disc keeps, in a `.wud` folder Cemu ignores, what rebuilds the disc byte for byte, and `wua_to_wux` and `wua_to_wud` rebuild it, checked against its SHA-1. Its update partition, the same on every disc carrying that update once decrypted, goes to a store shared by WUAs, once, its titles decrypted with their title keys and compressed with a large zstd window; `disc_info` reads the disc's hashes and `store_entries` the entries a WUA needs
- Added OTP dumps to `load_keys`: a Wii U's 1 KiB `otp.bin` gives its Wii U and vWii common keys, which `parse_otp` also takes from bytes
- Added the update partition's blocks, each encrypted on its own, to the clusters an FST can list, and the partition header's hash type
- Added `parse_tmd`, which reads a Wii U TMD's title ID, version and content records
- Added a streaming, parallel ZArchive writer, and a reader
- Added key handling in the format of Cemu's `keys.txt`, finding the disc key and the Wii U common key among candidates; no key ships with the crate
- Added hash checks on hashed clusters, so a damaged disc fails rather than converting to a broken WUA
- Added a progress callback to the library API, reporting input bytes consumed
- Added writing to `<output>.part`, renamed into place once complete, so a failed run leaves an existing output alone and nothing else behind
- Added refusing to replace an existing output folder, of NUS or reverted titles, unless every file in it is one the conversion writes again, as an earlier output of the same
- Added the `wuxrs` CLI, which compresses WUDs, decompresses WUXs, converts either to WUA or NUS, and turns WUAs back into their disc or into NUS, with `--updates` for the store of update partitions, `--add` for an update or DLC's NUS folder, as many as given, in oxyROMon's look, refusing to write one input's output over another's; it sits behind the default `cli` feature; `cemu_keys_path`, and its `dirs` dependency, sit behind it too
