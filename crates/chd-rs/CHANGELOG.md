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
- Added the `none`, `flac`, `huff`, `lzma`, `zlib` and `zstd` codecs, read
  through `flate2`, `zstd`, `lzma-rs`, `libflac-sys` and a built-in Huffman
  decoder and written back through `flate2`, `zstd`, `liblzma-sys`,
  `libflac-sys` and a built-in Huffman encoder, so the files are
  interchangeable with `chdman`'s, which the interop tests check
- Added the `chdrs` CLI, behind the default `cli` feature, in oxyROMon's
  look: images to CHDs and CHDs back by extension, plus `info` and `verify`