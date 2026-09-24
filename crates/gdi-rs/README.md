# gdi-rs

Dreamcast GD-ROM CUE/BIN to GDI conversion in Rust, on any platform. Part of
[oxyROMon](https://github.com/alucryd/oxyromon), as a library and the `gdirs`
CLI. Its GDIs are laid out as those of
[gdidrop](https://github.com/ElektroStudios/gdidrop-Dreamcast-Redump-Tool),
which it began as a port of (see [Credits](#credits)).

Redump dumps GD-ROMs as a CUE with one BIN per track; optical drive emulators
such as GDEMU load GDI instead. The track data is the same, save for pregaps:
a GDI track starts at its `INDEX 01`, so the pregap before it is dropped, and
the `.gdi` descriptor lists where each track sits on the disc instead.

```rust
use std::path::Path;

let gdi = gdi_rs::convert(Path::new("game.cue"), Path::new("out"), &mut |_| {}).unwrap();
```

This writes `out/game.gdi`, and one file per track named after its BIN —
and after its number, `game (Track 01).bin`, when the BIN holds several —
`.bin` for data, `.raw` for audio.

## Layout

Track by track, as gdidrop does it:

- A track with more than one `INDEX` starts at its second one: that many
  sectors of pregap are skipped in its BIN, and counted towards its position.
- A track's position is where the previous one ended; its length is what is
  left of its BIN, in 2352-byte sectors.
- After a track carrying the comment `REM HIGH-DENSITY AREA` — exactly that:
  gdidrop compares it whole — the next one starts no earlier than sector 45000,
  where a GD-ROM's high-density area begins. Redump writes that comment just
  before track 3's `FILE`, so it belongs to track 2.

gdidrop needs one BIN per track, the way Redump dumps GD-ROMs. gdi-rs also
takes a BIN holding several tracks, and converts it as if Redump had split it:
at each track's first `INDEX`, so a pregap stays with its own track and is
dropped with it. The result is the same as for the split set.

Where the input cannot be converted, gdi-rs refuses before writing anything:
an `INDEX` outside its track's part of the BIN, and an output directory where
a track would overwrite a BIN it reads.

## CLI

`gdirs` converts CUE/BIN sets, writing each GDI set to a folder of its own,
named after its CUE, next to the CUE unless told otherwise:

```
cargo build --release -p gdi-rs
gdirs game.cue                 # to game/game.gdi and its tracks
gdirs -o out/ *.cue            # to out/<game>/, one folder per CUE
```

| Flag     | Meaning                                        |
| -------- | ---------------------------------------------- |
| `-o DIR` | output directory (default: next to each input) |

The folder is also what keeps a data track from landing on its own BIN, which
is why gdidrop suffixes its tracks with ` [gdidrop]` instead. Like every
oxyROMon tool, `gdirs` draws oxyROMon's progress bar, reports each input on a
line of its own, and exits non-zero when any of them failed. It refuses to write
one input's output over another input, or over an earlier input's output.

## Verification

`tests/convert.rs` pins what gdidrop itself wrote, when gdi-rs was ported, for
synthetic CUE/BIN sets: a GD-ROM as Redump lays it out, a plain CD, and a
comment that only looks like the high-density marker. It also requires a
single BIN to give exactly what its split set gives.

## Credits

gdi-rs began as a port of
[gdidrop](https://github.com/ElektroStudios/gdidrop-Dreamcast-Redump-Tool) by
Feyris-Tan, maintained by [ElektroStudios](https://github.com/ElektroStudios).
Where each track sits on the disc, pregaps and the high-density area included,
comes from its work.

## License

BSD-2-Clause, same as gdidrop. See [LICENSE](LICENSE).
