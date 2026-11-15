# 0.1.0

## Features

- Added smelt, ore-rs's reader behind a C ABI, as `libsmelt.a` and `libsmelt.so` with `smelt.h`, written by cbindgen, and `smelt.hpp`, a header-only C++11 wrapper: the byte view, `smelt_open`, `smelt_clone`, `smelt_close`, `smelt_logical_size`, `smelt_platform`, `smelt_identity` and `smelt_read`, errors as codes with `smelt_last_error` the calling thread's last as text, and an ingot one thread's at a time
- smelt is MIT OR Apache-2.0, built over ore-rs without its `write` feature, which leaves out its GPL writer
