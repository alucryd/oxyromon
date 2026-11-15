# smelt-rs

smelt, the ORE reader library: [ore-rs](../ore-rs)'s reader behind a C
ABI, so an emulator reads an ORE as the disc or ROM it holds, at any
offset, with no temporary file. Ore is unrefined iron; smelting refines it.
Part of [oxyROMon](https://github.com/alucryd/oxyromon).

Unpublished, and its ABI unstable, until ORE's specification is.

- **`libsmelt.a` and `libsmelt.so`**, built by `cargo build --release -p
  smelt-rs`, with [`include/smelt.h`](include/smelt.h), which cbindgen
  writes from the crate, and [`include/smelt.hpp`](include/smelt.hpp), a
  header-only C++11 wrapper closing its ingot when it goes and throwing
  `smelt::error`.
- **The byte view**: `smelt_open`, `smelt_clone`, `smelt_close`,
  `smelt_logical_size`, `smelt_platform`, `smelt_identity` and
  `smelt_read`, what the ORE holds read as one run of bytes, a CD's tracks
  end to end. A thin ORE reads through its store, the nearest `.ore-store`
  up from its folder; one away from it is `SMELT_ERR_MISSING_STORE`.
- **The drive view**, for a CD: `smelt_track_count` and `smelt_track_at`,
  each track's number, mode, LBA, the pregap its file holds, its length,
  and where its file lies in the byte view; `smelt_read_sectors`, whole
  2,352-byte sectors by LBA across tracks, all or nothing; and
  `smelt_subq_replacement`, the Q subchannel the CD's SBI replaces, as
  libcrypt protected discs need, the drive generating every other.
- **One thread an ingot**: an ingot caches what it decodes, so each call
  may change it; `smelt_clone` gives another thread its own, sharing a thin
  ORE's store index, rather than a lock inside serialising a drive thread
  against the rest.
- **Not yet**: prefetch advice, to measure in a host first; DVD and
  GameCube discs read through the byte view meanwhile. See the ORE study's
  design 04.

```c
#include "smelt.h"

smelt_ingot *ingot;
uint8_t sector[2048];
if (smelt_open("Game (USA).ore", NULL, &ingot) != SMELT_OK) {
    fprintf(stderr, "%s\n", smelt_last_error());
    return 1;
}
smelt_read(ingot, 16 * 2048, sector, sizeof sector);
smelt_close(ingot);
```

After changing the ABI, write the header again; a test fails until it is:

```sh
cbindgen --config cbindgen.toml --output include/smelt.h
```

## License

MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)), so an emulator of any license can link
it. It builds ore-rs without its `write` feature, whose reader is under the
same licenses; ore-rs's writer, GPL, is not in libsmelt. FLAC and LZMA2's
decoders, rusty_flac and lzma-rust2, are Apache-2.0 alone, which a
GPL-2.0-only host should weigh.
