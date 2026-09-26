# xps-rs

IPS and BPS patch application in Rust. Part of [oxyROMon](https://github.com/alucryd/oxyromon),
as a library and the `xpsrs` CLI; an XPS is either an IPS or a BPS. It began as
a port of the patch applier in [Flips](https://github.com/Alcaro/Flips), whose
output it reproduces byte for byte (see [Credits](#credits)).

```rust
use std::path::Path;

let (rom, patch) = (Path::new("game.sfc"), Path::new("hack.bps"));
xps_rs::apply(rom, patch, Path::new("hack.sfc"), &mut |_| {}).unwrap();
```

## Scope

Application only, of the two formats oxyROMon deals in, told apart by magic:

- **IPS**, with the truncation extension, and Flips' three warnings, since IPS
  carries no checksum to refuse a wrong file by: a patch that truncates a file
  no longer than that is most likely not meant for it, one that changes nothing
  was most likely applied already, and one whose records reach past its own
  truncation is scrambled. Each still applies, as in Flips.
- **BPS**, which checks itself: the patch against its own CRC, then the source
  against its size and CRC before anything is written, and the output against
  its CRC after. A wrong source is refused, and one that is the patch's output
  already is told apart.

Not ported: creating patches, UPS, and Flips' retry of SNES ROMs without their
copier header, which is its application's rather than its formats'.

Nothing is loaded whole: the source is read where the patch points, and a BPS
target copy reads back what was written. Output is written to `<output>.part`
and renamed into place once complete, so a failed run leaves an existing output
alone and nothing behind.

## CLI

`xpsrs` applies each patch to one source, and names what it writes after the
patch with the source's extension, as Flips does:

```
cargo build --release -p xps-rs
xpsrs -s game.sfc hack.bps            # hack.sfc, next to the patch
xpsrs -s game.sfc -o out/ *.ips       # several hacks of one game, in out/
```

| Flag        | Meaning                                        |
| ----------- | ---------------------------------------------- |
| `-s FILE`   | the source the patches apply to (required)     |
| `-o DIR`    | output directory (default: next to each patch) |

Like every oxyROMon tool, `xpsrs` draws oxyROMon's progress bar, reports each
input on a line of its own, with an IPS warning after it, and exits non-zero
when any of them failed. It refuses to write one patch's output over another's,
over a patch, or over the source.

## Verification

- `tests/apply.rs` holds an IPS and a BPS Flips made, the BPS using every
  action, and IPS built by hand for what Flips cannot create (runs, growth,
  truncation, and each warning). It also mangles both patches, truncated and
  byte-flipped, to check they fail cleanly.
- `tests/interop.rs` has the installed Flips create IPS and delta BPS patches of
  a few MiB that edit, grow and move data, and checks each applies to the same
  bytes, and that an IPS applied twice warns as Flips does. It is skipped when
  `flips` is not in `$PATH`. (Flips 2.01 crashes creating linear BPS and
  shrinking IPS patches, hence the hand-built ones.)

## Credits

xps-rs began as a port of [Flips](https://github.com/Alcaro/Flips) by
[Alcaro](https://github.com/Alcaro): its reading of both formats, its checks,
and its warnings.

## License

GPL-3.0-or-later, same as Flips. See [LICENSE](LICENSE).
