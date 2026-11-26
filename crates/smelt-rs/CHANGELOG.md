# 0.1.0

## Features

- Added floppies' images, ORE's media class 16, the Amiga's and the Atari ST's IPFs and the Famicom Disk System's disks, read through the byte view as a cartridge's ROM is; and `smelt_file_name`, the name of the file a ROM's or a floppy's ORE was written from, for an emulator that picks its loader by extension, `file_name()` in `smelt.hpp`
- Added a Wii U eShop title's files: `smelt_file_count` and `smelt_file_at`, each file's name and where it lies in the byte view, so an emulator reads the ORE as the title's NUS folder, and `smelt_ticket`, its ticket, kept apart from the byte view; `smelt.hpp` has them as `files()` and `ticket()`
- Added smelt, ore-rs's reader behind a C ABI, as `libsmelt.a` and `libsmelt.so` with `smelt.h`, written by cbindgen, and `smelt.hpp`, a header-only C++11 wrapper: the byte view, `smelt_open`, `smelt_clone`, `smelt_close`, `smelt_logical_size`, `smelt_platform`, `smelt_identity` and `smelt_read`, errors as codes with `smelt_last_error` the calling thread's last as text, and an ingot one thread's at a time
- smelt is MIT OR Apache-2.0, built over ore-rs without its `write` feature, which leaves out its GPL writer
- Added the drive view of a CD: `smelt_track_count`, `smelt_track_at`, `smelt_read_sectors`, whole 2,352-byte sectors by LBA, and `smelt_subq_replacement`, the Q subchannel its SBI replaces
- smelt is a workspace of its own, so that no workspace build links ore-rs's GPL writer into libsmelt; it ships the CD sector model's BSD-3-Clause notice, its ECC being MAME's
- A track's `control`, its Q subchannel's control bits: its cue sheet's FLAGS, and whether it holds data
- `smelt.h` is C89, cbindgen's output without the trailing enum commas and `//` comments C89 refuses
- An SBI smelt cannot take, partial or damaged, is refused with its own error where its Q is asked for, the disc reading all the same
