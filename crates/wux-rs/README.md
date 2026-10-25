# wux-rs

Wii U disc images in Rust. Part of [oxyROMon](https://github.com/alucryd/oxyromon),
as a library and the `wuxrs` CLI. It compresses WUDs to WUX and back,
losslessly, and converts either to WUA, the archive
[Cemu](https://github.com/cemu-project/Cemu) plays, or to a NUS folder, which
WUP Installer GX2 installs on a console. Its WUXs are those
[wudd](https://github.com/wiiu-env/wudd) dumps and Cemu reads (see
[Credits](#credits)).

```rust
use std::path::Path;

let (wud, wux) = (Path::new("game.wud"), Path::new("game.wux"));
wux_rs::compress(wud, wux, &mut |_| {}).unwrap();
wux_rs::decompress(wux, Path::new("out.wud"), &mut |_| {}).unwrap();

// Cemu's keys.txt must hold the disc key and the Wii U common key.
let keys = wux_rs::load_keys(&wux_rs::cemu_keys_path().unwrap()).unwrap();
// A game and its update, from a NUS folder, make one WUA.
let update = Path::new("update/");
wux_rs::to_wua(&[wux, update], Path::new("game.wua"), &keys, &mut |_| {}).unwrap();
```

## The formats

**WUD** is a raw dump of the disc, 25,025,314,816 bytes for a whole one.

**WUX** stores the same bytes, each distinct 32 KiB sector once:

```
offset  size   field
0       4      magic: "WUX0"
4       4      magic: 0x1099d02e
8       4      sector size (0x8000)
12      4      padding
16      8      uncompressed size
24      4      flags (0)
28      4      padding
32      4n     table: one u32 per sector, naming the stored sector holding it
        ...    padding to a sector, then the stored sectors
```

Everything is little-endian. Sectors are stored in the order the disc first
uses them, and told apart by SHA-256, as wudd does. Since a disc is mostly
padding, a WUX is often a fraction of its WUD.

**WUA** is a [ZArchive](https://github.com/Exzap/ZArchive): the decrypted files
of one or more titles, each in a `<title id>_v<version>` folder, cut into
64 KiB blocks compressed with zstd. Like Cemu, a game, its update and its DLC
go in one WUA: the game from its disc, the others from NUS folders, the form
Nintendo's servers deliver them in, a signed `title.tmd`, a `title.tik` and
one encrypted `.app` per content. Getting there drops the disc's encryption and layout,
so a WUA can't be turned back into the disc it came from.

A WUA this crate writes keeps revert data in a `.nus` folder, which Cemu
ignores: per title, its TMD, ticket and certificates, its encrypted FST, and
its residue, the few bytes of its contents that are neither in a file nor
recomputed. Raw contents are zeros between files; hashed ones are too but for
32 bytes each, and their hash areas follow from their data. That is under
0.1% of a WUA, about 70 KB for a disc's game, and it turns the WUA back into a
NUS folder per title, whose contents are byte for byte those it was made
from, checked against the TMD. WUAs written without it, by Cemu or before,
can't be.

**NUS** is the form Nintendo's servers deliver a title in: a folder holding
its signed TMD, its ticket, its certificates, and each content as an
encrypted `.app`, with the H3 hashes of hashed ones in a `.h3`. A disc's game
partition holds the title's CDN contents byte for byte, so converting a disc
copies them as they are, checking each against the TMD, and takes the disc's
own TMD, ticket and certificates from its SI partition. Every `.app` and `.h3`
then matches No-Intro's CDN DAT; only `title.tmd` differs, as Nintendo signed
one for the disc and another for its servers. One quirk: the FST says it
starts at its partition's start, but it starts where the partition header
says.

## Keys

A disc's partitions are encrypted with a key of its own, and the game's files
with a title key that is itself encrypted with the Wii U common key. This
crate ships neither. Like Cemu, it takes a list of candidate keys, in the
format of Cemu's `keys.txt`, and tries each:

- the disc key is the one that decrypts the partition table's padding to
  zeros;
- the common key is the one whose title key decrypts the game partition's
  file table.

So the common key can sit in Cemu's `keys.txt` with the disc keys, where
Cemu ignores it.

## Scope

- **One game partition per disc.** Like Cemu, only a disc's first GM
  partition becomes a WUA folder: the console update a disc carries is left
  out. Updates and DLC come from their NUS folders.
- **No merging.** An update stays a folder of its own next to its game's, as
  Cemu lays them out, rather than overwriting the files it replaces.
- **A WUA reverts to NUS folders only.** The disc's layout and its console
  update partition are gone, so it can't become a disc image again.
- **No split dumps.** wudd splits dumps to FAT32 into 2 GiB parts; join them
  first.

## CLI

`wuxrs` compresses WUDs, decompresses WUXs and reverts WUAs, telling which
from each file's contents and extension, and writes next to each one unless told otherwise:

```
cargo build --release -p wux-rs
wuxrs game.wud                 # to game.wux, next to it
wuxrs game.wux                 # back to game.wud
wuxrs -f wua -o out/ *.wux     # to WUAs, in out/
wuxrs -f wua --add update/ game.wux   # the game and its update, in one WUA
wuxrs -f nus game.wux          # to the folder game/, for WUP Installer GX2
wuxrs game.wua                 # back to game/<title id>_v<version>/, per title
```

| Flag           | Meaning                                                    |
| -------------- | ---------------------------------------------------------- |
| `-o DIR`       | output directory (default: next to each input)             |
| `-f FORMAT`    | what to convert to, `wud`, `wux`, `wua` or `nus`           |
| `--add DIR`    | NUS folder of an update or DLC to add to the WUA, repeatable, with a single input |
| `--keys FILE`  | keys for WUA and NUS (default: Cemu's `keys.txt`)          |

Like every oxyROMon tool, `wuxrs` draws oxyROMon's progress bar, reports each
input on a line of its own, and exits non-zero when any of them failed. It
refuses to write one input's output over another input, or over an earlier
input's output.

## Verification

No Wii U disc can be shipped, so the tests build their own. `tests/common`
lays out a small disc as Cemu reads one: a partition table, an SI partition
holding the game's ticket, and a game partition with raw and hashed clusters,
a link, an empty file and nested directories, all encrypted with made-up
keys, and an update for it as a NUS folder. `tests/wua.rs` converts the disc
from WUD and from WUX, alone and with its update, and reads the WUA
back with [zarust](https://crates.io/crates/zarust), an independent ZArchive
implementation. `tests/wux.rs` round-trips generated images through WUX.

`tests/revert.rs` turns a WUA of the disc and its update back into NUS
folders, and checks them against the disc's own conversion and the update,
byte for byte, its hidden files, which only the residue keeps, included.

`tests/real.rs` runs on a real disc when given one, and is otherwise skipped:

```sh
WUXRS_DISC=game.wux WUXRS_WUA=game.wua cargo test --release -p wux-rs -- --ignored
```

It round-trips the disc through WUX, converts it to WUA, with the update in
`WUXRS_ADD` when set, and compares every file with a WUA Cemu made of the same
game.

## Credits

The WUX format comes from [wudd](https://github.com/wiiu-env/wudd) by
Maschell, and from Exzap's WUX tool before it. Reading the disc, its
partitions, file tables and hashed clusters, and the WUA layout, follow
[Cemu](https://github.com/cemu-project/Cemu)'s implementation, and the WUA
container follows Exzap's [ZArchive](https://github.com/Exzap/ZArchive).

## License

GPL-3.0-or-later, as wudd. Cemu's MPL-2.0 allows its use in GPL code. See
[LICENSE](LICENSE).
