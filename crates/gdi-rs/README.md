# gdi-rs

Dreamcast GD-ROM CUE/BIN to GDI conversion in Rust — a port of
[gdidrop](https://github.com/ElektroStudios/gdidrop-Dreamcast-Redump-Tool),
which only runs on Windows. Built for
[oxyromon](https://github.com/alucryd/oxyromon).

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

```
cargo build --release -p gdi-rs
gdirs game.cue -o out/
```

gdidrop writes next to the CUE and suffixes its tracks with ` [gdidrop]` so
they do not overwrite the BINs; `gdirs` writes to `-o` instead, under the BINs'
own names.

## Verification

`tests/interop.rs` runs gdidrop itself on synthetic CUE/BIN sets — a GD-ROM as
Redump lays it out, a plain CD, and a comment that only looks like the
high-density marker — and requires the same descriptor and byte-identical
tracks. gdidrop is a .NET Framework program; `tests/reference` compiles its
parser, CueSharp, unmodified, with its conversion copied verbatim, as a .NET 8
console app. The tests build it when `dotnet` and a gdidrop checkout are
available, and skip otherwise:

```sh
GDIDROP_SOURCE=/path/to/gdidrop-Dreamcast-Redump-Tool cargo test -p gdi-rs
```

`tests/convert.rs` pins gdidrop's layout of the GD-ROM set, so it is checked
without dotnet too, and requires a single BIN to give exactly what its split
set gives.

## License

BSD-2-Clause, same as gdidrop. See [LICENSE](LICENSE).
