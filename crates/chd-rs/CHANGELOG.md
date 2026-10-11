# 0.1.0

## Features

- Added the first slice of a port of `chdman` and MAME's CHD code: reading,
  inspecting and verifying CHD versions 3 to 5, of every media type, through
  `Chd::open`, `Chd::info`, `Chd::verify` and `Chd::read_bytes`
- Added extraction of DVDs and hard disks to raw images with `Chd::extract`,
  clones reading their unchanged hunks from a parent given to
  `Chd::open_with_parent`
- Added CHD v5 writing for DVDs and hard disks with `create`, from a raw
  image or against a parent to store only what changed, each hunk encoded
  with the shortest of the codecs offered
- Added the `none`, `flac`, `huff`, `lzma`, `zlib` and `zstd` codecs, through
  `flate2` (`zlib-rs`), `zstd`, `lzma-rust2`, `libflac-sys` and a built-in
  Huffman coder, so the files are interchangeable with `chdman`'s, which the
  interop tests check; most are byte-identical too
- Hunks are compressed on every core, as `chdman` does
- Added CD CHD writing with `create_cd`, from a CUE sheet, a GDI or an ISO,
  following `chdman` 0.289's `createcd`: its CUE and GDI parsing, WAV tracks,
  multisession and GD-ROM layouts, frame padding and track metadata
- Added CD CHD extraction with `extract_cd`, to a CUE, a GDI or a cdrdao TOC
  and their BINs, split by track on demand, following `chdman extractcd`
- Added DVD CHD writing with `create_dvd`
- Added LaserDisc CHDs with `create_ld` and `extract_ld`, from and to AVIs,
  following `chdman createld` and `extractld`: YUY2, UYVY, VYUY and
  left-predicted HuffYUV video with 8- or 16-bit PCM audio in, OpenDML AVIs
  past 2 GiB both ways, the `AVLD` VBI codes of NTSC and PAL captures, and
  the `avhu` codec in both directions
- Added `cd_input_size`, the total size of the files a CUE, GDI or ISO
  names, which `create_cd` reports progress against
- Added the `chdrs` CLI, behind the default `cli` feature, in oxyROMon's
  look: images to CHDs and CHDs back by extension, plus `info` and `verify`.
  CUEs and GDIs become CDs, ISOs DVDs, and CD CHDs extract to a CUE and its
  BINs