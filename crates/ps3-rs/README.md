# ps3-rs

PlayStation 3 discs in Rust. Part of [oxyROMon](https://github.com/alucryd/oxyromon),
as a library and the `ps3rs` CLI: a JB folder and its IRD rebuild the Redump
ISO byte for byte, and an ISO extracts to its JB folder.

```rust
use std::path::Path;

let ird = ps3_rs::Ird::read(Path::new("Game.ird")).unwrap();
let folder = Path::new("Game");
let md5 = ps3_rs::rebuild(
    &ird,
    &mut |path| Some(folder.join(path)),
    Path::new("Game.iso"),
    &mut |_| {},
)
.unwrap();
```

## Scope

- **IRDs**, version 9, as ps3-disc-dumper and 3k3y's tools write them: the
  disc's identity, the sectors before its first file and after its last, each
  region's and each file's MD5, and the data1 the disc key is derived from,
  checked against the IRD's CRC32.
- **Rebuilding**: every sector in order, the IRD's header and footer where no
  file lies, each file where the disc's Joliet directories place it, zeros in
  the gaps, and the regions sector 0 does not list as plain encrypted with
  AES-128-CBC under the disc key, each sector's IV its number. Every file is
  checked against the size and MD5 the IRD gives it, and the image's MD5 is
  returned, to check against a DAT.
- **Extracting**: the disc's files, decrypted, each with its MD5, from the
  disc key, which the IRD's data1 gives or Redump publishes as a `.dkey`.
- **Disc keys**: whether a key is a disc's, by what the first encrypted sector
  of its EBOOT.BIN, or of another file whose first bytes are known, decrypts
  to, so a pool of `.dkey` keys can be tried against an ISO.
- **System updates**: the firmware version a `PS3UPDAT.PUP` installs. Most JB
  folders go without theirs, which many discs share.

Not ported: IRDs before version 9, and building an IRD from a disc.

Nothing is loaded whole. Output is written to `<output>.part` and renamed into
place once complete, so a failed run leaves an existing output alone and
nothing behind.

## CLI

`ps3rs` rebuilds each JB folder given into `<folder>.iso`, and extracts each
ISO into a folder named after it:

```
cargo build --release -p ps3-rs
ps3rs --ird Game.ird --update PS3UPDAT.PUP "Game/"    # Game.iso, next to the folder
ps3rs --ird Game.ird -o out/ Game.iso                 # out/Game/
ps3rs --key 73819CEFE085B2C8FE8A92E7F81E84BC Game.iso
```

| Flag            | Meaning                                                  |
| --------------- | -------------------------------------------------------- |
| `--ird FILE`    | the disc's IRD, which a JB folder needs                  |
| `--key HEX`     | the disc key, as in a `.dkey`, for an ISO with no IRD    |
| `--update FILE` | the `PS3UPDAT.PUP` to use when a JB folder has none      |
| `-o DIR`        | output directory (default: next to each input)           |

Like every oxyROMon tool, `ps3rs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another's, or over an input.

## Verification

- `tests/disc.rs` runs on a synthetic disc test-support builds and encrypts as
  Redump's dumps are, with a file of two extents and gaps in its encrypted
  region: its JB folder and IRD rebuild it byte for byte, it extracts to its
  JB folder, and a wrong file, a missing one or a truncated disc fail without
  leaving anything behind.
- A known-answer test derives Redump's disc key for Disgaea 3 (USA) from its
  IRD's data1. Its rebuilt ISO matched Redump's MD5, and extracted back to its
  JB folder.

## Credits

The disc key's derivation from data1 follows
[libray](https://pypi.org/project/libray/), and the IRD format is
[ps3-disc-dumper](https://github.com/13xforever/ps3-disc-dumper)'s, which
[PSDevWiki](https://www.psdevwiki.com/ps3/IRD) documents.

## License

GPL-3.0-or-later, but for the disc model in `src/ps3`, which ore-rs shares, MIT OR Apache-2.0. See [LICENSE](LICENSE), [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
