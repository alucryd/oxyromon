# cia-rs

CIA (CTR Importable Archive) reading and unpacking in Rust, for Nintendo 3DS.
Part of [oxyROMon](https://github.com/alucryd/oxyromon), as a library and the
`ciars` CLI. Its extraction is byte-for-byte what
[ctrtool](https://github.com/3DSGuy/Project_CTR)'s `--plain` mode writes,
which it began as a port of (see [Credits](#credits)).

```rust
use std::path::Path;

let cia = cia_rs::Cia::parse(Path::new("game.cia"), &mut |_| {}).unwrap();
println!(
    "title {:016x} v{}, {} contents",
    cia.title_id(),
    cia.title_version(),
    cia.contents().len(),
);

cia_rs::unpack(
    Path::new("game.cia"),
    Path::new("out"),
    &cia_rs::UnpackOptions::default(),
    &mut |_| {},
)
.unwrap();
```

## The format

A CIA is a container, not a filesystem: a header, then up to five sections it
lays out, each starting on a 64-byte boundary.

```
offset  size    field
0x00    4       header size: always 0x2020
0x04    2       type: 0
0x06    2       format version: 0 (CIA), 0xFF (SimpleCIA)
0x08    4       certificate chain size
0x0C    4       ticket size
0x10    4       title metadata (TMD) size
0x14    4       footer size
0x18    8       contents size
0x20    2048    content index bitarray, bit N of byte N / 8's high bits
                ...    sections, in file order: certs, tik, tmd, contents,
                       footer; each aligned up, none present when empty
```

The header carries no magic; it is identified by its size and by the
arithmetic working out. Contents pack into their section one after another,
each aligned up from the previous by 16, and the TMD's 48-byte records name
them: id, index, size and the SHA-256 of the stored bytes. What the reference
tool does with this is what is reproduced here, offset for offset — an
extracted content is only hashable against a DAT if it lands exactly where
the container says it does.

## Scope

Deliberately narrow, so it is worth trusting:

- **Plain extraction only.** Contents are copied out as they sit: an
  encrypted content stays encrypted. No keys, no crypto.
- **No signature verification**, of the TMD or anything else — though the
  TMD's CMD group hashes are verified, so a corrupt content index is
  rejected rather than unpacked as garbage.
- **No parsing of what is inside the contents.** NCCH, ExeFS, RomFS and the
  rest are the job of another crate; a CIA is only as far as its parts here.

Wider than the reference in two ways, both cheap and both checked: the TMD's
signature may be any of the six known ES types (the reference accepts only
RSA-2048/SHA-256), and a title of any family keeps its content list — DSiWare
included, where the reference refuses non-CTR title types outright. SimpleCIA
homebrew containers (format version 0xFF) unpack to their single content.

## CLI

`ciars` unpacks CIAs into a folder named after each input:

```
cargo build --release -p cia-rs
ciars game.cia                 # to game/, next to it
ciars -o out/ *.cia            # into out/<name>/ folders
```

| Flag     | Meaning                                        |
| -------- | ---------------------------------------------- |
| `-o DIR` | output directory (default: next to each input) |

Parts come out named `certs`, `tik`, `tmd`, `content.<index>.<id>` and
`footer`. Like every oxyROMon tool, `ciars` draws oxyROMon's progress bar,
reports each input on a line of its own, and exits non-zero when any of them
failed.

## Verification

- `tests/kat.rs` checks the fixture CIA against the DAT entry it was built
  with: the content CRC and the TMD-with-appended-certificates CRC that
  oxyROMon imports against, the title id, version and content record.
- `tests/interop.rs` unpacks the same CIA with the reference binary and
  compares every part byte for byte; it is skipped unless a ctrtool is
  reachable through `$CTRTOOL` or `$PATH`.
- `tests/edge_cases.rs` walks the malformed paths the reference met only by
  crashing: truncated sections, a lying group hash, contents that extend past
  the container, a repeated content index.

```sh
cargo test -p cia-rs
```

## Credits

cia-rs began as a port of [ctrtool](https://github.com/3DSGuy/Project_CTR) by
[3DSGuy](https://github.com/3DSGuy); the container walk, the alignment rules
and the plain-mode extraction come from its work, and the TMD layout from the
[3dbrew](https://www.3dbrew.org/wiki/Title_Management_Data) documentation.

## License

MIT, the license of the libraries whose semantics are ported. See
[LICENSE](LICENSE).
