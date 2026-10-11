---
type: concept
title: The CommonRomfile Trait System
description: How CommonRomfile plus the FromPath, CommonFile, GetRomfile, As*/To*, Size, HashAndSize, HeaderedHashAndSize, Check, Playlist and Persist traits abstract over every ROM file format, how format modules plug into them, and the tmp-directory contract around conversion and verification.
tags: [romfile, traits, common-romfile, transcode, conversion, headers, hashing, tmp-directory]
verified:
  - by: openwiki/0.7.1
    at: 2026-10-09T16:11:37.544Z
sources:
  - id: openwiki-source-02628a361dd4faa0f62f87a2
    resource: repo://src/archive.rs
  - id: openwiki-source-8afb0fa0e0b1612b673e51d6
    resource: repo://src/chdman.rs
  - id: openwiki-source-f1234afbb261ce7190a87845
    resource: repo://src/check_roms.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-2fc0edbbfefe697fad55714d
    resource: repo://src/common/test_path_archive_custom_extension.rs
  - id: openwiki-source-19b56a9614977b0ad4e3da9b
    resource: repo://src/common/test_path_archive_single_file.rs
  - id: openwiki-source-82966a917a34a1bc19c13d43
    resource: repo://src/common/test_path_custom_extension.rs
  - id: openwiki-source-91de67d63579950c326c1195
    resource: repo://src/common/test_path_playlist.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-10fddfccad5d7bf89d675953
    resource: repo://src/convert_roms.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-bfaccf08e532321769e4b6b5
    resource: repo://src/export_roms.rs
  - id: openwiki-source-7a3d5614c68ed1a3729171e7
    resource: repo://src/gdi.rs
  - id: openwiki-source-316c0afa4f03f2eedd9022b5
    resource: repo://src/generate_playlists.rs
  - id: openwiki-source-6f95130cb448e88dd97ab560
    resource: repo://src/import_dats.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-72ac7329667332f8c07f2f9c
    resource: repo://src/import_roms/test_per_system_rom_directory.rs
  - id: openwiki-source-3a747acc24019bf2bf34f312
    resource: repo://src/iso.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-351b888a3823a95b61bcb42a
    resource: repo://src/mimetype.rs
  - id: openwiki-source-cc76cc07825d7eb399a2b402
    resource: repo://src/model.rs
  - id: openwiki-source-d3402490d16a663b822e8a00
    resource: repo://src/nsz.rs
  - id: openwiki-source-313c10a1161be7da48ebd676
    resource: repo://src/purge_roms.rs
  - id: openwiki-source-db13c01516e9bd04294e339d
    resource: repo://src/rvz.rs
  - id: openwiki-source-0a9271029eb893aacec54c15
    resource: repo://src/sort_roms.rs
  - id: openwiki-source-4fe52c7cb090ec0387522035
    resource: repo://src/transcode.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-856ac41ecb89a38b80d7e895
    resource: repo://src/wbfs.rs
  - id: openwiki-source-196778315263fb8bcc8066e5
    resource: repo://src/xso.rs
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
---

# The CommonRomfile Trait System

`src/common.rs` is the layer that lets `import-roms`, `sort-roms`, `convert-roms`,
`export-roms`, `check-roms` and `purge-roms` talk about "a ROM file" without
knowing whether it is a `.7z`, a DVD CHD, a PSP CSO or a CUE/BIN set. The layer is
deliberately thin: one struct holding a path, plus traits that each own one verb
(naming, hashing, checking, persisting, converting). Format modules add behaviour
by implementing those traits for a wrapper type; the workflows only ever choose
which wrapper to build, and stay format-agnostic after that.

## The core types

```rust
pub struct CommonRomfile { pub path: PathBuf, pub system_id: Option<i64> }
pub struct IsoRomfile    { pub romfile: CommonRomfile }
pub struct CueBinRomfile { pub cue_romfile: CommonRomfile, pub bin_romfiles: Vec<CommonRomfile> }
```

`CommonRomfile` is *not* a parser and holds no I/O state — it is an absolute path
plus an optional system scope, and `Display` prints the path. Every other wrapper
is a `CommonRomfile` plus whatever a format needs (`ChdRomfile` carries `ChdType`,
`track_count` and SHA1s; `ArchiveRomfile` carries the entry name, archive type,
size and CRC recorded when the archive was parsed).

`FromPath::from_path` is the only constructor and never touches the filesystem;
`with_system` attaches a system id so that later path math can honour a
per-system `ROM_DIRECTORY` override instead of the global one. The database's
`Romfile` row, by contrast, stores a path *relative* to the ROM directory, and
`AsCommon` is the single bridge between the two worlds: it joins the row path onto
`get_rom_directory`. Every workflow starts a chain with `romfile.as_common(connection)`.

## Naming: `As*` parses, `To*` converts

The convention is fixed and documented at the top of `main.rs`, which carries
`#![allow(clippy::wrong_self_convention)]` for it: `As*` turns an existing file
into a wrapper, `To*` produces a new file converted *from* another format. Clippy
would object that a method named `as_*` should not consume `self`, and the file
says so explicitly — the wrappers **take `self` by value on purpose**
(`as_iso(self)`, `as_chd(self)`, `as_rvz(self)`, `as_cue_bin(self, bins)`, …),
because the wrapper *becomes* the path it validated. The exceptions prove the
ownership rule: `as_common` and `as_archive` take `&self`, since an
`ArchiveRomfile` keeps a clone of the container it points into and many entries of
one archive must be able to coexist. `To*` traits all borrow `&self` — the source
file has to survive its own conversion.

- `As*` validates and wraps, and the validation depth is the safety property.
  `as_iso`, `as_rvz`, `as_nsp`, `as_nsz`, `as_cue_bin` (and its BINs) and `as_gdi`
  (and its track files) check the lowercased extension against the constants in
  `mimetype.rs`; `as_xso`, `as_riff`, `as_rdsk` and `as_chd` call `get_mimetype` so
  the *content* must match, and `as_chd` additionally runs `chdman info` to fill in
  `ChdType`, size, SHA1s and track count. A wrapper is a claim about the file, so a
  weakly validating `As*` would let a misnamed file reach a decoder.
- `To*` writes a file: it picks the destination name (`file_name()` with
  `with_extension`), prints an action line, drives the backend, and returns the new
  wrapper built by running `CommonRomfile::from_path` **back through the matching
  `As*`**, so the output is re-validated on the way out.

```mermaid
flowchart LR
    DB["Romfile row"] -->|as_common| COMMON["CommonRomfile"]
    COMMON -->|as_iso| ISO["IsoRomfile"]
    COMMON -->|as_cue_bin| CUEBIN["CueBinRomfile"]
    COMMON -->|as_archive| ARCHIVE["ArchiveRomfile"]
    COMMON -->|as_chd| CHD["ChdRomfile"]
    COMMON -->|as_xso| XSO["XsoRomfile"]
    COMMON -->|as_rvz| RVZ["RvzRomfile"]
    COMMON -->|as_nsp| NSP["NspRomfile"]
    COMMON -->|as_nsz| NSZ["NszRomfile"]
    COMMON -->|as_rdsk| RDSK["RdskRomfile"]
    COMMON -->|as_riff| RIFF["RiffRomfile"]
    ARCHIVE -->|to_common| COMMON
    CHD -->|to_iso| ISO
    CHD -->|to_cue_bin| CUEBIN
    CHD -->|to_rdsk| RDSK
    CHD -->|to_riff| RIFF
    XSO -->|to_iso| ISO
    RVZ -->|to_iso| ISO
    CUEBIN -->|to_iso| ISO
    ISO -->|to_xso| XSO
    ISO -->|to_rvz| RVZ
    ISO -->|to_chd| CHD
    ISO -->|to_wbfs| WBFS["WBFS output"]
    CUEBIN -->|to_chd| CHD
    CUEBIN -->|to_gdi| GDI["GdiRomfile"]
    NSP -->|to_nsz| NSZ
    NSZ -->|to_nsp| NSP
    COMMON -->|to_archive| ARCHIVE
    ARCHIVE -->|to_archive| ARCHIVE
```

*Caption: format-to-format conversion paths — `As*` edges parse a file into a wrapper, `To*` edges convert from one format into another, and `CommonRomfile`, `IsoRomfile` and `CueBinRomfile` are the three hubs nearly every route passes through.*

`IsoRomfile` and `CueBinRomfile` live in `common.rs` because they are the neutral
targets: almost every disc format converts to or from ISO, and CUE/BIN is the
multi-file neutral shape. `CommonRomfile` is itself a member of the graph — it is
both the source of every `As*` and the destination of `ToCommon for
ArchiveRomfile` (extraction).

## File verbs: `CommonFile`

`CommonFile` is implemented **only for `CommonRomfile`**, so any wrapper that needs
to be moved, copied or deleted is first reduced to its inner file (via
`GetRomfile::romfile`). It owns five operations:

- `get_relative_path` strips the ROM directory — chosen per system through
  `get_directory(connection, "ROM_DIRECTORY", self.system_id)` with a fallback to
  the global `ROM_DIRECTORY` — and fails with a "Failed to convert … to relative
  path" context error when the file lives outside it. This is the exact string
  `romfiles.path` is written with and the exact string `purge-roms` walks the tree
  and matches against, so it is the system's file↔row identity.
- `get_sorted_path` computes where a file *should* live: 1G1R vs system vs Trash
  directory by `Game.sorting`, an optional alpha subfolder from
  `REGIONS_ONE_SUBFOLDERS` / `REGIONS_ALL_SUBFOLDERS`, a per-game directory for
  jbfolders and for arcade titles whose extension is not an archive, and finally
  the filename. The extension override
  argument matters: for extensions in `NON_ORIGINAL_EXTENSIONS` (chd, cso, nsz,
  rvz, zso, 7z, zip) the file is named after the *game*, not the DAT rom, because
  those containers are derived files; otherwise `System.custom_extension` wins over
  `rom.name`. `sort-roms` diffs this against the current path, and the
  `src/common/test_path_*.rs` suite pins it.
- `rename` is a no-op when the target equals the current path and otherwise returns
  a new `CommonRomfile`; `copy` flattens into the destination directory using only
  `file_name()`; `delete` removes the file.

Both `rename` and `copy` rebuild through `from_path`, which resets `system_id` to
`None`. Callers that persist after a move must re-apply `with_system` — exactly
what `import_roms::trash_or_delete_romfile` does before its `create`/`update`, so a
per-system ROM directory still resolves on the next path computation.

## The verification stack

`Size`, `HashAndSize` and `Check` are the three rungs every format module must
implement.

- `Size::get_size` on a `CommonRomfile` is a plain `metadata().len()`, unwrapped —
  so a file that vanished underneath the workflow panics rather than reporting an
  error. `ArchiveRomfile` and `ChdRomfile` return the size recorded at parse time
  and fall back to decoding into a temp directory; `ChdRomfile` refuses outright for
  `ChdType::Cd`, which has no meaningful single-file size.
- `HashAndSize for CommonRomfile` funnels through the free function `hash_reader`,
  which hashes *anything that implements `Read`* while advancing the progress bar.
  That is the seam that lets a decoder be hashed without being written to disk:
  `RvzRomfile::hash_disc` feeds the `nod` `DiscReader` straight into it, so a
  dual-layer Wii disc is verified without several gigabytes of temporary ISO.
  `XsoRomfile` and `NszRomfile` do the opposite — decode to a temp directory, hash
  the decoded file, delete it.
- `get_hash_algorithm` picks CRC, then MD5, then SHA1 from whichever checksum the
  DAT row holds (else `bail!("Not possible")`), and `compare_hash_and_size` fails
  with `Size mismatch` (only when the DAT declares a non-zero size) or `Checksum
  mismatch`. These two strings plus the wrapper's own errors are all
  `check-roms` sees: it counts a `Check` error as one bad file and moves it to
  Trash.
- `Check for CommonRomfile` verifies `roms[0]` only. Multi-file cases decompose:
  `ChdRomfile::check` extracts CUE/BIN and zips each BIN with its own DAT rom;
  `check_roms::check_archive` refuses an archive whose entry count differs from the
  rom count, then checks each entry.

A nuance worth knowing before optimising: `ArchiveRomfile` short-circuits
`HashAndSize` with the entry's stored CRC and size, so *import* of a file inside a
container needs no extraction — but `Check for ArchiveRomfile` always extracts and
delegates. Integrity checking of an archive entry is therefore a real decode.

## Header handling: `HeaderedHashAndSize`

Headered systems (old NES-era DATs) declare a header in the DAT's header detector;
`import-dats` writes it to `headers` (name, version, `size`, `system_id`) with one
`rules` row per byte pattern (a hex value plus a `start_byte` parsed from the
detector's hex offset). `find_header_by_system_id` returns at most one header per
system, and `check-roms` and `import-roms` fetch it once per system and thread it
down as `&Option<Header>`.

`get_file_and_header_size` performs the detection: it reads the first
`header.size` bytes, and for every rule compares the hex encoding of the buffer
from `start_byte` onward against the rule's lowercase hex prefix. **Only when every
rule matches** is `header_size` returned as `header.size` — and the file handle is
left positioned *past* the header, so the caller hashing that handle hashes the
header-stripped content. On any mismatch the handle is rewound to 0 and
`header_size` is 0, which is why the same ROM file hashes correctly whether or not
the header is actually present. `get_headered_size` subtracts the header from the
on-disk length, and `get_headered_hash_and_size` sets the bar to that stripped
length before digesting. `Check for CommonRomfile` branches on `&Option<Header>`
between the headered and plain paths, and wrappers with no header semantics (RVZ)
fall back to extracting a decodable ISO when a header unexpectedly shows up.

## `transcode.rs`: the neutral decoded shape and the tmp contract

`convert_roms` and `export_roms` need the same thing over and over: "give me this
database file in a format I can convert *from*, and tell me what file it originally
came from." That is `Decoded<T>`:

```rust
pub struct Decoded<T> { pub source: CommonRomfile, pub inner: T }
```

`inner` is the decoded wrapper; `source` is the file the row actually points at —
the archive, or the same file when nothing needed decoding
(`common_as_source`). Constructors are `archive_to_common` (extracts the single
file an archive holds), `chd_to_iso` (DVD CHDs only; returns `None` for other
`ChdType`s so the caller skips), `xso_to_iso`, `nsz_to_nsp`, `rvz_to_iso`,
`archive_to_cue_bin`, plus `romfile_as_chd` which resolves a CHD's parent from
`romfile.parent_id` before wrapping.

Every constructor takes a `&TempDir`, and the contract around it is
`util::create_tmp_directory`: `TempDir::new_in(get_tmp_directory(connection))`,
where `TMP_DIRECTORY` comes from settings, else `OXYROMON_TMP_DIRECTORY`, else the
system temp dir. Three consequences:

1. The scratch space is always inside the *configured* tmp directory, never the
   default `std::env::temp_dir()` on another volume, and `benchmark` measures that
   directory specifically because decode-then-convert throughput is bounded by it.
2. The `TempDir` is deleted when dropped, so a converted file is disposable unless
   it is written elsewhere. Hence the pattern in `convert_roms`: decode into the
   temp directory, then run the `To*` conversion again with
   `decoded.source.path.parent()` as the destination so the deliverable lands next
   to the original — never the temp file itself.
3. Workflows create one temp directory per game (`let tmp_directory =
   create_tmp_directory(connection).await?` inside the loop), so a large conversion
   never accumulates several decoded discs.

`check_and_persist` closes the loop and is where the trait system's requirements
are visible in a signature: `async fn check_and_persist<T: Check + GetRomfile>(…)`.
A wrapper must implement `Check` (verify the output) *and* `GetRomfile` (reach the
file to delete or rename) to participate. On check failure it prints
`Integrity check failed, reverting conversion`, deletes the new file and returns
`false`, and the caller leaves the transaction to roll back; on success it updates
the `romfile` row and deletes the source — optionally renaming the target onto the
source path when recompressing in place. Note that the multi-file wrappers
`CueBinRomfile` and `GdiRomfile` implement neither `GetRomfile` nor `Check`: they
are handled file-by-file by the caller, archiving each CUE and BIN separately.

```mermaid
sequenceDiagram
    participant CV as convert_roms to_rvz
    participant TR as transcode
    participant TD as tmp TempDir
    participant WR as RvzRomfile
    participant DB as SQLite romfile row

    CV->>TD: create_tmp_directory per game
    CV->>TR: archive_to_common with rom, romfile and tmp
    TR->>TD: extract archive entry
    TR-->>CV: Decoded with source archive and inner CommonRomfile
    CV->>WR: as_iso then to_rvz beside the source
    CV->>WR: check_and_persist on the target
    WR->>WR: check hashes the decoded disc
    alt check failed
        WR->>TD: delete new file
        CV-->>DB: no commit, transaction rolls back
    else check passed
        WR->>DB: Persist update path and size
        CV->>CV: delete source archive
        CV-->>DB: commit
    end
```

*Caption: one conversion through the trait system — `Decoded` separates the original file from its decoded form, the temp directory holds only the intermediate, and `Check` gates whether the row update and source deletion are committed.*

## Persistence and playlists

`Persist` is implemented **only for `CommonRomfile`**: `create` computes the
relative path and size and inserts a row with a `RomfileType` (`Romfile`,
`Playlist` or `Patch`), returning its id; `update` rewrites path and size. It is
the sole writer of `romfiles.path`, always reached at the end of a
`CommonFile::rename` chain (`sort-roms`, `check-roms`'s trash move,
`generate-playlists`, `import-roms`). The filesystem is therefore mutated before
the row, and conversions wrap both in a transaction so a failed `Check` reverts the
row while the code explicitly deletes the file it produced.

`Playlist` is the odd one out: it is implemented for `Game`, not for a romfile,
because the M3U path is a function of the game's sorting and subfolder scheme
mirroring `get_sorted_path`, with `DISC_REGEX` stripping ` (Disc N)…` from the name
to get the shared `<game>.m3u`.

## How a format module plugs in

Adding a format means adding a wrapper and filling in traits, then telling the
workflows about it — nothing in `common.rs` changes:

1. Wrapper struct over `CommonRomfile`, plus `GetRomfile`.
2. `Size`, `HashAndSize`, `Check` — usually by decoding to the temp directory and
   delegating to the decoded format's impl.
3. `As*` (validate + wrap) and `To*` (write + re-validate through `As*`).
4. Extension constants and, if the format has magic bytes, a matcher in
   `mimetype.rs`; see
   [File Type Detection and Extension Constants](file-type-detection-and-extensions.md).
5. Registration in the workflow layers: an extension branch in
   `check_roms::check_system`, source partitioning plus a target function in
   `convert_roms`/`export_roms`, and an import branch in `import_roms`.
   See [Format Conversion and Export](../workflows/format-conversion-and-export.md)
   and [Integrity Check and Purge](../workflows/integrity-check-and-purge.md).

The backend choice — in-process crate or external executable — is invisible from
here; see [Format Modules and Their Crate Backends](format-modules-and-format-crates.md).

## Focused tests

`src/common/` is a directory of single-purpose path tests rather than a
`#[cfg(test)] mod`. Each builds `System`/`Game`/`Rom`/`Romfile` values in memory,
calls `as_common().get_sorted_path()` and compares against an expected path — no ROM
bytes involved, which is why the naming rules can be changed with confidence.
`test_path_archive_single_file` and `test_path_archive_multiple_files` pin the
"derived containers are named after the game" rule;
`test_path_archive_custom_extension` pins the sharper edge — a `.7z` still becomes
`game name.7z` even when the system declares `custom_extension = cci`, because a
non-original extension outranks it — while `test_path_custom_extension` shows the
plain `.3ds` file becoming `game name.cci`. `test_path_original` keeps the
original-extension case at `rom name.rom`; `test_path_cso` and `test_path_rvz` pin
those formats; `test_path_chd_single_track` / `test_path_chd_multiple_tracks` pin CHD
naming; `test_path_subfolder_alpha_letter` / `test_path_subfolder_alpha_other` pin
the alpha subfolder; and `test_path_playlist` and
`test_path_playlist_subfolder_alpha` pin `Playlist`. Header behaviour is covered by
integration tests instead: `check_roms/test_original_with_header`,
`check_roms/test_sevenzip_with_header`, `import_roms/test_original_headered` and
`import_dats/test_dat_headered*`.
