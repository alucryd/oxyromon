# 0.1.0

## Features

- Added smelt, ore-rs's reader behind a C ABI, as `libsmelt.a` and `libsmelt.so` with `smelt.h`, written by cbindgen, and `smelt.hpp`, a header-only C++11 wrapper: the byte view, `smelt_open`, `smelt_clone`, `smelt_close`, `smelt_logical_size`, `smelt_platform`, `smelt_identity` and `smelt_read`, errors as codes with `smelt_last_error` the calling thread's last as text, and an ingot one thread's at a time
- smelt is MIT OR Apache-2.0, built over ore-rs without its `write` feature, which leaves out its GPL writer
- Added the drive view of a CD: `smelt_track_count`, `smelt_track_at`, `smelt_read_sectors`, whole 2,352-byte sectors by LBA, and `smelt_subq_replacement`, the Q subchannel its SBI replaces
- smelt is a workspace of its own, so that no workspace build links ore-rs's GPL writer into libsmelt; it ships the CD sector model's BSD-3-Clause notice, its ECC being MAME's
- An SBI smelt cannot take, partial or damaged, is refused with its own error where its Q is asked for, the disc reading all the same
