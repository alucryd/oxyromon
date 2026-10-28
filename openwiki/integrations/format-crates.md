---
type: integration
title: Format Crates in crates/
description: The eight in-repo format crates (chd-rs, cia-rs, gdi-rs, nsz-rs, wux-rs, xdelta-rs, xps-rs, xso-rs), seven of which replaced oxyromon's external tools — their shared synchronous path-in/path-out entry-point contract with a progress callback, the identical CLI shell enforced by tests/crates.rs, the .part output discipline, error conventions, library-first manifests and per-crate release flow.
tags: [crates, workspace, chd-rs, gdi-rs, nsz-rs, wux-rs, xdelta-rs, xps-rs, xso-rs, cli, progress, error-handling, releases]
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-ca6cb4b1a14fd7969dfae3ec
    resource: repo://CHANGELOG.md
  - id: openwiki-source-1519e62b0bf12b3d41c2e1b8
    resource: repo://crates/chd-rs/Cargo.toml
  - id: openwiki-source-f652141db50cb9f0dce8c6fe
    resource: repo://crates/chd-rs/README.md
  - id: openwiki-source-05b12b6f45cbcde3b69bdfdd
    resource: repo://crates/chd-rs/src/bin/chdrs/main.rs
  - id: openwiki-source-451efc56b1b11560177f2d9a
    resource: repo://crates/chd-rs/src/container.rs
  - id: openwiki-source-9a03a1f712d91d693134083a
    resource: repo://crates/chd-rs/src/error.rs
  - id: openwiki-source-3a39ba8cc3839b0fbf3be2e6
    resource: repo://crates/chd-rs/src/extract.rs
  - id: openwiki-source-a057427619dc76389568b31f
    resource: repo://crates/chd-rs/src/flac.rs
  - id: openwiki-source-3b812eb8bded02c6c14a4b23
    resource: repo://crates/chd-rs/src/ld.rs
  - id: openwiki-source-84d75d6d1e1e60c51643c99f
    resource: repo://crates/chd-rs/src/writer.rs
  - id: openwiki-source-87aab858391fe90e7855fcfa
    resource: repo://crates/chd-rs/tests/cd.rs
  - id: openwiki-source-6f5af9e6dfe3a21ba40721b8
    resource: repo://crates/chd-rs/tests/cli.rs
  - id: openwiki-source-38b8a6db58a8dd7d870046a0
    resource: repo://crates/chd-rs/tests/ld.rs
  - id: openwiki-source-dace695e1efd12a88e32dd04
    resource: repo://crates/chd-rs/tests/write.rs
  - id: openwiki-source-c7fa9c8f75ed83141fe6d91b
    resource: repo://crates/cia-rs/Cargo.toml
  - id: openwiki-source-039ef28c41d4fc7a1b163edc
    resource: repo://crates/cia-rs/tests/common/mod.rs
  - id: openwiki-source-16c849e86b1d340adbc8eca0
    resource: repo://crates/gdi-rs/src/error.rs
  - id: openwiki-source-677cadb1f4a36dab4e6c2937
    resource: repo://crates/gdi-rs/src/lib.rs
  - id: openwiki-source-efab198b41b24bf2a241e882
    resource: repo://crates/gdi-rs/tests/convert.rs
  - id: openwiki-source-b69e5f5da7beb0a728369e2f
    resource: repo://crates/nsz-rs/Cargo.toml
  - id: openwiki-source-23ec944a2b2aa3fdb67cb90d
    resource: repo://crates/nsz-rs/src/bin/nszrs/main.rs
  - id: openwiki-source-6fc8cca2c873b2934e5dd26b
    resource: repo://crates/nsz-rs/src/error.rs
  - id: openwiki-source-b26db718d3130938eabd548a
    resource: repo://crates/nsz-rs/src/keys.rs
  - id: openwiki-source-78b5264772a51da7dc97c2ca
    resource: repo://crates/nsz-rs/src/pipeline.rs
  - id: openwiki-source-95961593538c6afbde4e76cc
    resource: repo://crates/nsz-rs/tests/malformed.rs
  - id: openwiki-source-ce853e79f786a28d42f8717a
    resource: repo://crates/wux-rs/Cargo.toml
  - id: openwiki-source-35760035950e3f037cce37d1
    resource: repo://crates/wux-rs/src/bin/wuxrs/main.rs
  - id: openwiki-source-b50a26200a8a4e4e83f9eac1
    resource: repo://crates/wux-rs/src/error.rs
  - id: openwiki-source-bb63ebff30207378358552a3
    resource: repo://crates/wux-rs/src/io.rs
  - id: openwiki-source-fb3fc971ccda3173fb55c35e
    resource: repo://crates/wux-rs/src/lib.rs
  - id: openwiki-source-50f94ae1d9f2b065641770f1
    resource: repo://crates/wux-rs/src/nus.rs
  - id: openwiki-source-8b154e92f317de1d79a56d61
    resource: repo://crates/wux-rs/src/rebuild.rs
  - id: openwiki-source-a7055168e07a2f5ab1abc26e
    resource: repo://crates/wux-rs/src/wua.rs
  - id: openwiki-source-459bee48f81e7fcc3cd4f278
    resource: repo://crates/wux-rs/tests/nus.rs
  - id: openwiki-source-9c665d9fab5f86f2cc31d823
    resource: repo://crates/wux-rs/tests/revert.rs
  - id: openwiki-source-3177d2bf46b3b23d873f6783
    resource: repo://crates/xdelta-rs/src/bin/xdeltars/main.rs
  - id: openwiki-source-b284ee56f1a8ed8eeb895333
    resource: repo://crates/xdelta-rs/src/error.rs
  - id: openwiki-source-23afc243073cf6f118f3e57f
    resource: repo://crates/xdelta-rs/src/lib.rs
  - id: openwiki-source-7c41df92324ca8ddc11ad14b
    resource: repo://crates/xdelta-rs/tests/interop.rs
  - id: openwiki-source-9fe9baae9737aff7c0d60c42
    resource: repo://crates/xps-rs/src/bin/xpsrs/main.rs
  - id: openwiki-source-d67f333c93dce596979892b2
    resource: repo://crates/xps-rs/src/bps.rs
  - id: openwiki-source-1f66adc1dc3a11d514005522
    resource: repo://crates/xps-rs/src/error.rs
  - id: openwiki-source-a7121d88e575f6b311ff06c2
    resource: repo://crates/xps-rs/src/ips.rs
  - id: openwiki-source-a69d4f26e372ad7e44e682a0
    resource: repo://crates/xps-rs/src/lib.rs
  - id: openwiki-source-6933fe5a19eb2a199452b761
    resource: repo://crates/xso-rs/Cargo.toml
  - id: openwiki-source-0315d9b847e75c2c6933fffa
    resource: repo://crates/xso-rs/src/bin/xsors/main.rs
  - id: openwiki-source-f77ef39903c134a635cc3ced
    resource: repo://crates/xso-rs/src/bin/xsors/ui.rs
  - id: openwiki-source-21932b3fb8623d512be1bcb6
    resource: repo://crates/xso-rs/src/compress.rs
  - id: openwiki-source-1b459f92a1fa129b1324530b
    resource: repo://crates/xso-rs/src/decompress.rs
  - id: openwiki-source-aece083bcec2ba4297d1dbdc
    resource: repo://crates/xso-rs/src/error.rs
  - id: openwiki-source-026ef51e874e909eef0a2328
    resource: repo://crates/xso-rs/src/io.rs
  - id: openwiki-source-982d918d7483241a1c39d015
    resource: repo://crates/xso-rs/tests/interop.rs
  - id: openwiki-source-8fc81edf3c652f2d4cc794cf
    resource: repo://crates/xso-rs/tests/roundtrip.rs
  - id: openwiki-source-19a65c75a2d97f6840dee349
    resource: repo://dist.sh
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-c3dbd15c40e250cdf06fed8d
    resource: repo://src/chd.rs
  - id: openwiki-source-db15f8427180aeec55b3a076
    resource: repo://src/cia.rs
  - id: openwiki-source-7a3d5614c68ed1a3729171e7
    resource: repo://src/gdi.rs
  - id: openwiki-source-d3402490d16a663b822e8a00
    resource: repo://src/nsz.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-9b5bca3734529131a5ec0d1b
    resource: repo://src/xps.rs
  - id: openwiki-source-13df766a80853b510f45e6c3
    resource: repo://tests/crates.rs
generated: { by: "claude-code", at: "2026-10-28T08:10:36.025Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-28T12:28:25.132Z
---

# Format Crates in crates/

`crates/` holds eight format crates. Seven were written to replace the external
programs oxyromon used to shell out to. Each of those began as a port of one
upstream tool and keeps that tool's license. The eighth, `wux-rs`, replaced no
tool: oxyromon never ran one for the Wii U, so it follows wudd and Cemu and takes
wudd's GPL-3.0-or-later. Every crate and stays a standalone library plus its own CLI
published to crates.io; oxyromon is the workspace root and depends on them as
path dependencies. The result is that the formats in this table have no runtime
dependency to install, report `"built-in"` from `get_version()`, and can report
real byte-level progress instead of a spinner.

| Crate | Began as a port of | CLI | License | oxyromon module |
| --- | --- | --- | --- | --- |
| `chd-rs` | chdman | `chdrs` | BSD-3-Clause | `chd` |
| `cia-rs` | ctrtool (`--plain` unpacking) | `ciars` | MIT (upstream shipped none) | `cia` |
| `gdi-rs` | gdidrop | `gdirs` | BSD-2-Clause | `gdi` |
| `nsz-rs` | nsz (nicoboss) | `nszrs` | MIT | `nsz` |
| `xdelta-rs` | xdelta3 | `xdeltars` | Apache-2.0 | `xdelta` |
| `xps-rs` | Flips | `xpsrs` | GPL-3.0-or-later | `xps` |
| `xso-rs` | maxcso | `xsors` | ISC | `xso` |
| `wux-rs` | none; follows wudd and Cemu | `wuxrs` | GPL-3.0-or-later | `wux` |

The crates are workspace members (`members = ["crates/*"]`), so they share one
lockfile, one CI run and one set of conventions; `frontend/` and `desktop/` are
deliberately *not* members so their wasm and webview dependencies stay out of
the native build. CHD and CIA joined the set with 0.25: `chdman` and `ctrtool`
are gone as runtime dependencies and survive only as test-time reference
binaries, so oxyromon runs no child process for any format
([External Tools](external-tools.md)). CHD's container, codecs,
media types and `src/chd.rs` adapter have a page of their own —
[CHD Format and the chd-rs Crate](../concepts/chd-format-and-chd-rs.md) — so
this page covers only what that crate shares with the others. The same holds
for wux-rs and [Wii U Formats and the wux-rs Crate](../concepts/wii-u-formats-and-wux-rs.md). The `src/`
adapters that call into these crates are covered in
[Format Modules and Their Crate Backends](../concepts/format-modules-and-format-crates.md).

`chd-rs` is also the crate with the most codec dependencies: `rusty_flac` (a
pure-Rust FLAC codec, with its `libm` feature so encoder decisions do not hang on
the platform libm), `lzma-rust2`, `flate2` with the `zlib-rs` backend, `zstd`,
`sha1`, `crc32fast` and `rayon`. It needs no CMake: FLAC used to come from
`libflac-sys`, which built a vendored libFLAC. chd-rs calls
`Encoder::set_block_size`, which no rusty_flac release has yet, so the root
`Cargo.toml` patches `rusty_flac` to a revision of the alucryd/rusty_flac fork's
`feat/block-size` branch until one does. It names the revision, not the branch,
so a `cargo update` cannot move it. Its `chdrs` CLI sits behind the same default `cli` feature as the
others, and oxyromon depends on the crate with `default-features = false`, so
none of the CLI stack is linked into the organizer.

## The entry-point contract

Every crate's public entry point is synchronous, takes an input path, an output
path, a settings struct, and `progress: &mut dyn FnMut(u64)`. There is no async
runtime, no `Read`/`Write` handed to the caller, and no thread spawning visible
from outside (the crates that parallelise build their own rayon pools).

The progress callback takes **bytes of the input consumed since the last call**,
not a position, and the calls add up to the input size — where "input" means the
thing whose bytes dominate the work:

- `xso_rs::compress` / `xso_rs::decompress`: the ISO or CSO/ZSO being read.
- `chd_rs::create` / `create_hd` / `create_dvd` / `create_cd` / `create_ld`: the
  image, sheet or AVI being read — for a CD that total is what
  `chd_rs::cd_input_size` sums over the files a CUE, GDI or ISO names, not the
  sheet's own size.
- `chd_rs::Chd::extract` / `extract_cd` / `extract_ld`: the **CHD's logical
  size**, reported as decompressed bytes written — decompressing hunks rather
  than reading them is what takes the time, so `src/chd.rs` passes
  `chd.info().logical_size` as the bar length for an extraction.
- `xdelta_rs::decode` / `xps_rs::apply`: the **patch** size, reported window by
  window or action by action, since a patch is small and the source read is
  driven by it.
- `gdi_rs::convert`: the BINs behind the CUE, which is why the crate also
  exports `input_size(cue)` — a caller cannot know that total from the CUE's own
  size, and `gdi_rs` counts a track's dropped pregap as consumed too.
- `wux_rs::compress` / `decompress` / `to_nus` / `wua_to_wux` / `wua_to_wud`:
  oxyromon passes the input file's size, and the rebuilds scale the disc's
  written sectors to it. `wux_rs::to_wua` takes several inputs, a disc and NUS folders, and
  oxyromon passes the sum of their sizes.
- `nsz_rs::pipeline::compress_nsp` / `decompress_nsz`: the container being read;
  an internal `member_progress` adapter converts per-member positions into
  deltas so each byte is reported exactly once even when a member is read out of
  order (an update NCA's BKTR table is seeked to before the sequential pass
  reaches it).

oxyromon adapts that contract in exactly one place,
`progress::run_blocking(progress_bar, length, work)`, which clones the shared
bar, moves the closure to `tokio::spawn_blocking`, resets the bar to the bytes
style with `length`, and wires `bar.inc` into the crate's callback. `length` is
therefore always what the crate's calls sum to: `input.metadata()?.len()` for
`xso`, `nsz`, `xdelta` and `xps` and for a DVD, hard disk or LaserDisc being
compressed, `gdi_rs::input_size(&cue)` for `gdi`, `chd_rs::cd_input_size(&input)`
for a CD being compressed, and `chd.info().logical_size` for a CHD extraction. A
`JoinError` becomes `"Conversion task failed"`, kept distinct from the crate's
own error. Running these on a runtime worker instead would stall it for the
length of a several-gigabyte compression.

```rust
pub async fn run_blocking<T, E>(
    progress_bar: &ProgressBar,
    length: u64,
    work: impl FnOnce(&mut dyn FnMut(u64)) -> Result<T, E> + Send + 'static,
) -> anyhow::Result<T>
```

The same signature is what makes a new backend drop-in: `src/chd.rs`,
`src/xso.rs`, `src/nsz.rs`, `src/gdi.rs`, `src/wux.rs`, `src/xdelta.rs` and
`src/xps.rs` differ only in the settings they build and the length they pass.

## `.part` output discipline

Every crate writes to `<output>.part` **next to the destination** (same
directory, so the finish step is a rename, not a copy) and only renames it into
place once the write completed. On any failure the `.part` is removed. Two
guarantees follow, and each crate tests them: an existing output is replaced
only by a complete new one, and a failed run leaves nothing behind.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Planning
    Planning: validate the input and plan every output
    Planning --> Writing: input accepted
    Planning --> Rejected: bad magic or invalid option
    Writing: every byte goes to the part file beside the destination
    Writing --> Committed: write finished
    Writing --> Rejected: any error mid-write
    Committed: rename the part file over the destination
    Committed --> [*]
    Rejected: remove the part file, destination untouched
    Rejected --> [*]
```

*Caption: the write lifecycle every format crate shares; nothing at the destination path changes until the whole output is complete.*

The halves are literally the same code, duplicated per crate:
`xso-rs` and `wux-rs` factor it into `crate::io::{part, finish}`, plus
`finish_dir` in `wux-rs` for folder outputs, while `chd-rs`
(`writer::part_path` plus a rename-or-remove arm shared by `write_part`,
`Chd::extract`, `extract_cd` and `extract_ld`), `nsz-rs`,
`xdelta-rs`, `xps-rs` and `gdi-rs` each carry a private `part`/`finish` pair.
Four crate-specific variations matter:

- **`gdi-rs` stages a whole set.** A GDI is one descriptor plus one file per
  track, so it writes every `.part` first and renames them all only after the
  last one succeeds, then removes all of them if anything failed. Validation —
  the CUE layout, the output directory, and the rule that no track may be
  written over a BIN the CUE reads — happens in `plan`, before any file opens,
  so a rejected input is "not ours to delete".
- **`chd-rs` stages a CD set the same way.** `extract_cd` writes the CUE, GDI or
  cdrdao TOC and every track BIN to `.part` files, renames them all only once
  the last has been written, and removes them all if anything failed.
- **`nsz-rs` back-patches its header.** `with_output` creates the `.part`, seeks
  past the reserved PFS0 header, lets the caller stream members, then rewinds
  and writes the header from the recorded `(name, offset, size)` layout. It
  drops the `File` before renaming or removing, which Windows requires.
- **`wux-rs` writes folders.** `to_nus` and `wua_to_nus` stage a whole folder
  as `<output>.part` and move it into place with `io::finish_dir`. An existing
  folder there is replaced only when every file in it, recursively, is one the
  new folder holds too, an earlier output of the same. Otherwise the write fails
  with an `AlreadyExists` I/O error and leaves the folder alone.

`nsz-rs` additionally `create_dir_all`s the output's parent before opening the
`.part` — the only crate that creates directories for you.

## Errors: shared wording, per-crate additions

Each crate has one `thiserror` enum named `Error` plus a `Result<T>` alias, and
the shared variants keep identical `#[error(...)]` strings so a message reads the
same whichever backend produced it:

| Variant | Text | In |
| --- | --- | --- |
| `Io` | `io error: …` | all eight, `#[from] std::io::Error` |
| `BadMagic { expected, found }` | `bad magic: expected …, found …` | `chd-rs`, `xso-rs`, `nsz-rs`, `wux-rs`, `xdelta-rs`, `xps-rs` |
| `Unsupported` | `unsupported format: …` | `chd-rs`, `cia-rs`, `xso-rs`, `nsz-rs`, `wux-rs`, `xdelta-rs`, `xps-rs` |
| `Corrupt` | `corrupt data: …` | all eight |
| `InvalidOption` | `invalid option: …` | `chd-rs`, `xso-rs`, `gdi-rs`, `xdelta-rs` |
| `WrongSource` | `wrong source: …` | `xdelta-rs`, `xps-rs` only |

`WrongSource` exists only in the two patch crates and is the interesting one:
it means *the patch is fine and this is not its file*. BPS raises it after
checking the patch's own CRC, then the source's size and CRC — with a distinct
message when the "source" is actually the patch's output already — and xdelta
raises it when a window's Adler-32 does not match what it rebuilt. Crates add
only what their format needs: `nsz-rs` has `MissingKey`, `InvalidKey`,
`Verification`, `Compression` and `Hex`; `wux-rs` adds only `MissingKey`, for a
disc no candidate key opens; `chd-rs` has only `Compression`
alongside the shared five; `xso-rs` has nothing beyond them.

Errors are the *only* failure channel — no panics on malformed input. Crafted
inputs are tested explicitly in `nsz-rs/tests/malformed.rs`: an `NCZSECTN`
section count of `1 << 40`, `i64::MAX` or `-1`, a PFS0 with `u32::MAX` file
count, or a `u64::MAX` member offset must all fail rather than drive a huge
allocation. `xso-rs` bounds a block's stored span so a corrupt index cannot ask
for gigabytes, and `nsz-rs` refuses an implausibly large Meta NCA before
decompressing it.

The one exception to "error means nothing was applied" is `xps_rs::apply`'s
`Option<Warning>`: IPS carries no checksum, so Flips' three suspicions
(`NotThis`, `AlreadyApplied`, `Scrambled`) are reported while still applying the
patch. oxyromon prints it as `Patched, but …`.

## One identical CLI shell, checked by a test

Each crate's `[[bin]]` is gated on `required-features = ["cli"]` and consists of
a `main.rs` with a clap builder `cli()` plus one `convert(input, matches,
outputs)` closure, and a `ui.rs` that is **byte-identical across all eight**.
`tests/crates.rs` reads `crates/xso-rs/src/bin/xsors/ui.rs` and asserts the
copies in `chdrs`, `ciars`, `gdirs`, `nszrs`, `wuxrs`, `xdeltars` and `xpsrs`
equal it, so the shared shell must be changed in all eight at once.

```mermaid
flowchart TD
    A["ui::run parses argv"] --> B["Outputs::new records every input path"]
    B --> C["next INPUT"]
    C --> D["tool's convert closure"]
    D --> E{"Outputs::claim output path"}
    E -- "is one of the inputs" --> F["red cross line, run marked failed"]
    E -- "written for an earlier input" --> F
    E -- "free" --> G["library call with bar.inc callback"]
    G -- "Ok line" --> H["green tick line"]
    G -- "Err message" --> F
    H --> C
    F --> C
    C -- "inputs exhausted" --> I["ExitCode SUCCESS, or FAILURE if any failed"]
```

*Caption: the per-input loop `ui.rs` provides; only the `convert` closure is crate-specific.*

What `ui.rs` fixes for every tool: `<cli> [OPTIONS] <INPUT>...` with each input
converted in the direction its extension implies; `-o DIR` defaulting to next to
each input and created on demand; output names `<input stem>.<extension>`
keeping dots in the stem; oxyROMon's own progress-bar template, hidden when
stderr is not a terminal; one `✔`/`✖` result line per input on stdout/stderr;
`AFTER_HELP` pointing back at the project; and a non-zero exit as soon as any
input failed. `Outputs::claim` compares `std::path::absolute` paths, so
`xsors -o out/ a/game.iso b/game.iso` fails on the second input instead of
silently writing `out/game.cso` twice, and an input can never be overwritten by
its own output. The patch tools add a further guard in their own `convert`: the
output may not land on `-s`'s source file.

Flag meanings are fixed across tools that share them — `-o DIR`, `-b SIZE` in
**bytes** (`nszrs` converts it to the zstd block exponent the library wants and
rejects anything not a power of two in 16384..=4294967296; `chdrs` uses it for
the hunk size, 16..=1048576, and refuses one that differs from the parent CHD's),
and `-s FILE` for the patch crates' source — with lowercase short forms reserved
for common options. `--`-only long flags carry no short alias.

`wuxrs` departs from "the extension implies the direction" in one way: `-f`
picks the target (`wud`, `wux`, `wua` or `nus`), `--add DIR` adds an update or
DLC folder to a WUA, `--updates DIR` names the store of update partitions WUAs
share (the `.update-partitions` folder next to each WUA by default), and
`--keys FILE` overrides Cemu's `keys.txt` with another keys file or a
console's `otp.bin`. A `.wua` input goes back to its WUX
by default, or to NUS folders with `-f nus`.

`chdrs` is the one tool whose `main` does not hand argv straight to `ui::run`:
it intercepts `info` and `verify` as subcommands that read a CHD and report to
stdout only, and falls through to the shared loop for everything else. Those two
are where it departs from `chdman`'s exit codes deliberately — a hash mismatch
exits non-zero where `chdman` succeeds.

## Library-first manifests

Every manifest is written so the library is the product and the CLI is an
optional extra:

- `[features] default = ["cli"]`, `cli = ["dep:clap", "dep:console", "dep:indicatif"]`;
  `clap`, `console` and `indicatif` are `optional = true` and used only by
  `src/bin/<cli>/`.
- oxyromon depends on each crate with `default-features = false`, so it links
  the library and none of the CLI's UI stack.
- `edition`, `rust-version` and `repository` come from `[workspace.package]`, and
  shared dependencies (`clap`, `console`, `crc32fast`, `flate2`, `indicatif`,
  `lzma-rust2`, `rayon`, `sha1`, `tempfile`, `thiserror`, `zstd`) from
  `[workspace.dependencies]`. Feature choices for a shared dependency belong
  there too: Cargo unifies features across the workspace, so a member's
  `default-features = false` cannot remove what another member enables. `flate2`
  is declared once with `default-features = false` and the `zlib-rs` backend
  for that reason; while only `chd-rs` opted out, oxyromon's plain
  `flate2.workspace = true` still compiled the default `miniz_oxide` backend
  alongside it. `chd-rs` is the heaviest consumer of that list; its one
  dependency outside it is `rusty_flac`, pure Rust like the rest.
- No crate declares `[profile]`: only the root's applies, and the root sets
  `opt-level = 3` for dependencies in dev builds precisely because these codecs
  are unreadable at opt-level 0.
- Each carries its own `version`, `license` (the upstream's), `description`,
  `keywords`, `categories` and an `include` list.

## Releases and distribution

Releases are **per crate**: its own version number, its own `CHANGELOG.md` in
oxyromon's format, and git tags named `<crate>-<version>` (e.g. `nsz-rs-0.3.0`).
A crate must be published before any oxyromon release that needs its new
version, since the root pins each dependency by `path` **and** `version`. The
root's own `0.x` tags are a separate series.

```sh
cargo build --release -p chd-rs -p cia-rs -p gdi-rs -p nsz-rs -p xdelta-rs -p wux-rs -p xps-rs -p xso-rs  # the eight CLIs
cargo test -p xso-rs                            # one crate
cargo test --release -p xso-rs -- --ignored     # its slow tests
```

`dist.sh` cross-compiles the eight CLIs alongside oxyromon for every target
(Linux gnu/musl on aarch64 and x86_64, Windows gnullvm, and both Apple targets)
and packages each binary separately (`chdrs`, `ciars`, `gdirs`, `nszrs`, `wuxrs`,
`xdeltars`, `xpsrs`, `xsors` per target triple, signed on macOS), so a standalone tool is
installable without the organizer.

## Per-crate entry points worth knowing

- **`chd_rs::create` / `create_hd` / `create_dvd` / `create_cd` / `create_ld`**
  and the reading side — `Chd::open` / `open_with_parent`, `info`,
  `track_count`, `verify`, `extract`, `extract_cd`, `extract_ld`, and
  `cd_input_size` for the CD progress denominator. The media-specific writers are
  thin shells over `writer::write_part`, which is also where clone CHDs resolve
  unchanged hunks against a parent. Internal:
  [CHD Format and the chd-rs Crate](../concepts/chd-format-and-chd-rs.md).
- **`gdi_rs::convert(cue, output_dir, progress) -> Gdi`** plus
  `input_size(cue)`. `output_dir` must already exist, and the returned `Gdi`
  names the `.gdi` descriptor and each track file so the caller can wrap them.
- **`nsz_rs::pipeline::compress_nsp(input, output, keys, compression, fix_padding, progress)`**
  and **`decompress_nsz(input, output, keys, fix_padding, verify, strict, progress) -> Report`**.
  Keys arrive as a *loader* (`impl FnOnce() -> Result<Keys>`), invoked only when
  the pipeline actually needs one: compression calls it as it reaches the first
  `.nca` member, decompression only when verifying against a CNMT.
- **`xdelta_rs::decode(source, patch, output, progress)`** — `source` may be
  `None` only for a patch that copies from none — and `read_header(patch)`, which
  exposes the `target_name` a patch records (the CLI uses it for the output name,
  reduced to a bare file name so a recorded path cannot escape the output dir).
- **`xps_rs::identify(patch) -> Format`** and `apply(source, patch, output, progress)
  -> Option<Warning>`. `identify` reads five bytes of magic to choose IPS or BPS,
  and the CLIs call it before opening a progress bar.
- **`wux_rs::compress` / `decompress`, `to_wua(inputs, output, keys, store,
  progress)`, `to_nus(input, output, keys, progress)`, `wua_to_nus`,
  `wua_to_wux` / `wua_to_wud(input, output, store, keys, progress)`, `disc_info`,
  `store_entries` and `title_id`**, plus `Image` to read a WUD or WUX as one
  stream, `load_keys` for Cemu's `keys.txt` or an `otp.bin`, with `parse_keys` and
  `parse_otp` for each, and `parse_tmd` with its `Tmd` and
  `Content`, which oxyromon reads to grade a Wii U title against its newest
  version. `cemu_keys_path`, and the
  `dirs` dependency it needs, exist only with the `cli` feature. `STORE_DIR` names the
  `.update-partitions` store and `DISC_DIR` the `.wud` folder of a lossless WUA. Internal:
  [Wii U Formats and the wux-rs Crate](../concepts/wii-u-formats-and-wux-rs.md).
- **`xso_rs::compress` / `decompress` / `probe`**, with `Stats { input_size,
  output_size }` back for reporting a ratio. `probe` reads only the 24-byte
  header.

## nsz-rs and `prod.keys`

`nsz-rs` and `wux-rs` are the two crates that need user secrets. `wux-rs` needs
Wii U keys for WUA and NUS, rebuilding a disc from a WUA included, as its own
page explains. `nsz-rs` needs
them in one narrow case.
`nszrs` defaults to `~/.switch/prod.keys` (overridable with `-k`), and
oxyromon uses the same path via `dirs::home_dir()`. Keys are CRC32-verified
against the checksums of the legitimate sources and master keys — the same
sanity check the Python tool performs, so a tampered or truncated `prod.keys`
fails loudly instead of producing garbage; `--skip-key-check` exists for
synthetic test keys, and keys absent from the table are accepted unverified.

Because loading is lazy, **`prod.keys` is needed only to compress NSPs
containing NCAs**: unverified decompression never loads it, and containers
without NCAs (homebrew, for instance) never need it in either direction. That is
why oxyromon's `nsz::get_version()` returns `"built-in"` but
`"built-in, prod.keys not found"` when the file is missing — it can never fail,
and the warning exists so a user does not discover the missing file
mid-compression. oxyromon's own decompression path runs
`decompress_nsz(..., fix_padding = true, verify = false, strict = false)`, so
checking and extracting an NSZ works with no keys at all. Key-reading errors name
the file, since a lazily loaded key can fail far from where the path was given.

## How these crates are tested

The convention is *agreement with the reference implementation*, not
self-consistency: `tests/interop.rs` in `xso-rs`, `xdelta-rs` and `xps-rs` runs
the upstream binary (maxcso via `$MAXCSO`/`$PATH`, `xdelta3`, `flips`) and checks
that each reads the other's output, and that our CSO is byte-identical to
`maxcso --only-zlib`. `chd-rs` does the same against `chdman` (`$CHDMAN`, then
`$PATH`) across four suites: `cli.rs` compares `info` reports and `verify`
verdicts on repository CHD fixtures, `write.rs` checks that each tool verifies
and extracts the other's CHDs — plain and clone — byte-identically, `cd.rs`
compares CHDs built from CUE, GDI and ISO inputs, and `ld.rs` builds its AVIs
with `ffmpeg`. Where the bytes depend on the compression library (deflate,
zstd) the suites compare the header hashes and only demand byte parity under
`CHDRS_STRICT_PARITY=1`. Where chd-rs uses an encoder other than chdman's —
LZMA through lzma-rust2, FLAC through rusty_flac, which covers every
LaserDisc's audio — they never expect the same bytes: chdman must verify our
CHD and the header hashes must match.
Those tests **skip rather than fail** when the tool is absent; CI installs
`mame-tools`, `xdelta3` and `ffmpeg` — all only for the crates' tests — so the
chd-rs and xdelta-rs comparisons run, and slow cases are `#[ignore]`d. `nsz-rs`
leans on generated known-answer vectors (`tests/vectors.rs`, produced by
`scripts/gen_vectors.py`) plus round-trip and malformed-input suites. `wux-rs`
has no reference binary to run: its tests build a synthetic encrypted disc and
read the WUAs back with the independent `zarust` crate, and `tests/disc.rs`
rebuilds that disc from its WUA byte for byte.

Two suites specifically pin the `.part` contract: `xso-rs/tests/roundtrip.rs`
truncates a CSO so decompression fails mid-write and asserts the pre-existing
output still says `keep me` and no `.part` survives, and
`gdi-rs/tests/convert.rs` blocks one track's `.part` with a directory to prove a
half-written set is neither committed nor cleaned up beyond what the crate
created. CI covers the crates and oxyromon in one pass
(`cargo llvm-cov --workspace --features oxyromon/server`), so `tests/crates.rs`
runs alongside them.

## Adding or changing a crate

The seams are narrow and fixed by the conventions above:

1. Add the crate under `crates/` (it joins `members = ["crates/*"]`
   automatically), library-first, with the `cli` feature and
   `required-features = ["cli"]` on its `[[bin]]`.
2. Copy `src/bin/<cli>/ui.rs` verbatim — `tests/crates.rs` will fail otherwise —
   and write only `cli()` and `convert()`.
3. Keep the shared `Error` variants and their exact wording; add a variant only
   for something the shared set cannot say.
4. Write to `<output>.part` and rename on success; validate everything before
   the first file is created so an output you did not make is never removed.
5. Report input bytes consumed as deltas summing to the length a caller can
   compute up front; export a helper like `gdi_rs::input_size` or
   `chd_rs::cd_input_size` when you cannot.
6. Add the dependency to the root `Cargo.toml` with `path` + `version` +
   `default-features = false`, then wire the module behind
   `progress::run_blocking` and list it in `info`, the GraphQL dependency query
   and the MCP status tool so it stays reportable.
