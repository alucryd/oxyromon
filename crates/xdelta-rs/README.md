# xdelta-rs

xdelta3 patch decoding in Rust. Part of [oxyROMon](https://github.com/alucryd/oxyromon),
as a library and the `xdeltars` CLI. It began as a port of the decoder in
[xdelta3](https://github.com/jmacd/xdelta), whose output it reproduces byte for
byte (see [Credits](#credits)).

```rust
use std::path::Path;

let (rom, patch) = (Path::new("game.rom"), Path::new("hack.xdelta"));
xdelta_rs::decode(Some(rom), patch, Path::new("hack.rom"), &mut |_| {}).unwrap();
```

## Scope

Decoding only: VCDIFF ([RFC 3284](https://www.rfc-editor.org/rfc/rfc3284)) with
the default code table, plus xdelta3's extensions:

- **Adler-32** checks on every window, so a patch applied to the wrong source
  fails instead of producing garbage.
- **LZMA secondary compression**, xdelta3's default since 3.0. Each section
  kind is a single xz stream that xdelta3 flushes at the end of every section
  and continues in the next window, so it is decoded that way, in pure Rust with
  [lzma-rust2](https://crates.io/crates/lzma-rust2).

Refused with an error rather than misread:

- **DJW and FGK** secondary compression, which xdelta3 only writes when asked
  with `-S djw` or `-S fgk`. A patch that names one but compresses nothing with
  it still decodes.
- **Code tables of the patch's own**, and **windows that copy from the target**,
  neither of which xdelta3 decodes.

A window holds at most 64 MiB, xdelta3's own limit, and is rebuilt in memory;
the source is read where copies point, never loaded whole. Output is written to
`<output>.part` and renamed into place once complete, so a failed run leaves an
existing output alone and nothing behind.

## CLI

`xdeltars` applies each patch to one source, and names what it writes as the
patch records, as xdelta3 does, or after the patch otherwise:

```
cargo build --release -p xdelta-rs
xdeltars -s game.rom hack.xdelta          # next to the patch
xdeltars -s game.rom -o out/ *.xdelta     # several hacks of one game, in out/
```

| Flag        | Meaning                                        |
| ----------- | ---------------------------------------------- |
| `-s FILE`   | the source the patches apply to (required)     |
| `-o DIR`    | output directory (default: next to each patch) |

Like every oxyROMon tool, `xdeltars` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one patch's output over another's, over a patch, or over the
source.

## Verification

- `tests/decode.rs` holds patches xdelta3 3.2 wrote, with several windows and
  LZMA sections compressed across them, and the targets they must reproduce. It
  also tries every truncation and a flipped byte at every position of one.
- `tests/interop.rs` has the installed xdelta3 encode a few MiB across its
  window sizes, levels and secondary compressors, and checks each decodes to the
  target. It is skipped when `xdelta3` is not in `$PATH`.

## Credits

xdelta-rs began as a port of [xdelta3](https://github.com/jmacd/xdelta) by
[Joshua MacDonald](https://github.com/jmacd): its window format extensions, its
secondary compression framing, and its limits.

## License

Apache-2.0, same as xdelta3. See [LICENSE](LICENSE).
