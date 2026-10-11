---
type: workflow
title: ROM Import Pipeline
description: How import-roms hashes a file, matches it against the DAT catalog, places it on disk and records a romfiles row linked from roms.romfile_id — per-format dispatch, the transaction-per-file boundary, the system/game id sets returned to callers, and the PREFER_FORMAT auto-conversion pass that follows.
tags: [import-roms, hashing, catalog-matching, romfiles, transactions, trash, prefer-format, chd, archives, jbfolder]
verified:
  - by: openwiki/0.7.1
    at: 2026-10-09T16:11:37.544Z
sources:
  - id: openwiki-source-02628a361dd4faa0f62f87a2
    resource: repo://src/archive.rs
  - id: openwiki-source-8afb0fa0e0b1612b673e51d6
    resource: repo://src/chdman.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-10fddfccad5d7bf89d675953
    resource: repo://src/convert_roms.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-6f95130cb448e88dd97ab560
    resource: repo://src/import_dats.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-d2a3ecfe56b135e9acbf7c81
    resource: repo://src/import_roms/test_mame_chd.rs
  - id: openwiki-source-c63a2a415ce9850aa169d04a
    resource: repo://src/import_roms/test_original_delete.rs
  - id: openwiki-source-f13ad111e0b94e6baae4cb31
    resource: repo://src/import_roms/test_original_headered.rs
  - id: openwiki-source-29b30bb0ed8fedcdfd03a4f3
    resource: repo://src/import_roms/test_original_trash.rs
  - id: openwiki-source-72ac7329667332f8c07f2f9c
    resource: repo://src/import_roms/test_per_system_rom_directory.rs
  - id: openwiki-source-2e0110a5b76b7e42ecbced1f
    resource: repo://src/import_roms/test_prefer_format.rs
  - id: openwiki-source-6a6bb3347c2c1219f0d0fab5
    resource: repo://src/import_roms/test_rvz_as_is.rs
  - id: openwiki-source-01ea4763ac48ce0a080c98ff
    resource: repo://src/import_roms/test_rvz.rs
  - id: openwiki-source-ddd20e064f59f29ed5468ee4
    resource: repo://src/import_roms/test_sevenzip_multiple_files_full_game.rs
  - id: openwiki-source-bbed279a7ad3cd152bdb54bb
    resource: repo://src/import_roms/test_sevenzip_multiple_files_mixed_games.rs
  - id: openwiki-source-bd10baabc6433c6d5ac9ecd0
    resource: repo://src/import_roms/test_sevenzip_multiple_files_partial_game.rs
  - id: openwiki-source-67041cdcd7062eab68afff98
    resource: repo://src/import_roms/test_zip_single_file_extract.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-b95ec25c5cfc0be350df258c
    resource: repo://src/mcp.rs
  - id: openwiki-source-762e9c43ab8143e7dda872d3
    resource: repo://src/mcp/write.rs
  - id: openwiki-source-351b888a3823a95b61bcb42a
    resource: repo://src/mimetype.rs
  - id: openwiki-source-d3402490d16a663b822e8a00
    resource: repo://src/nsz.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-fcdf0f9c4361c1ee384f2689
    resource: repo://src/prompt.rs
  - id: openwiki-source-2d7562030526fe478e02c8ca
    resource: repo://src/rebuild_roms.rs
  - id: openwiki-source-db13c01516e9bd04294e339d
    resource: repo://src/rvz.rs
  - id: openwiki-source-dfa02c19f5d8e4215d36f9c0
    resource: repo://src/server.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-196778315263fb8bcc8066e5
    resource: repo://src/xso.rs
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
---

# ROM Import Pipeline

`import-roms` turns a pile of files on disk into `romfiles` rows linked from
`roms.romfile_id`. Everything else in oxyromon — sorting, conversion, export,
integrity checking — consumes the result, so the two things this page is really
about are **(1) how a file gets matched to a catalog `roms` row** and **(2) what
the unit of work is**, because that decides what a crash or a rejected file costs.

The one surface is [`import_roms::main`](../../src/import_roms.rs#L138-L353).
The CLI dispatches to it directly ([src/main.rs](../../src/main.rs#L151)), the
HTTP server synthesises an `ArgMatches` for `POST /roms`
([src/server.rs](../../src/server.rs#L804-L817)), and the MCP `import_roms` tool
does the same ([src/mcp/write.rs](../../src/mcp/write.rs#L155-L236)). Other
modules bypass `main` and call the internals: `import-dats` re-imports orphaned
files through `import_rom` ([src/import_dats.rs](../../src/import_dats.rs#L880-L935))
and `rebuild-roms` fills arcade sets through `import_other`
([src/rebuild_roms.rs](../../src/rebuild_roms.rs#L155-L186)).

## Command line

```
oxyromon import-roms [OPTIONS] <ROMS>...
  -s, --system <SYSTEM>     Select systems by name (repeatable)
  -t, --trash               Trash invalid ROM files
  -d, --delete              Delete invalid ROM files (hard delete)
  -f, --force               Force import of existing ROM files
  -u, --unattended <MODE>   none | skip | first   (clap default: none)
  -x, --extract             Extract top-level archives before importing
  -a, --as-is               Import as-is, without format handling or conversion
```

`--trash` and `--delete` are mutually exclusive and bail before any work starts
([src/import_roms.rs](../../src/import_roms.rs#L155-L161)). The clap default for
`--unattended` is `none`, i.e. **interactive**: anything driven by the server or
MCP must pass the flag explicitly, and both of them default to `first` because no
one is at a terminal
([src/server.rs](../../src/server.rs#L749-L760)).

## System scoping, and why `-s` also enables headered matching

`-s` resolves each name with `find_systems_by_name_like`, whose predicate is
`WHERE name LIKE ?` — a raw SQL `LIKE` pattern (SQLite matches ASCII
case-insensitively), so `%PlayStation 3%` works while a name with no wildcards has
to match a system name exactly
([src/database.rs](../../src/database.rs#L294-L312)). The resolved list is then
de-duplicated by id, and when nothing is passed it becomes a single `None` entry
meaning "every system"
([src/import_roms.rs](../../src/import_roms.rs#L143-L153)).

That list is not just a filter — `main` loops **once per system**, so `-s A -s B`
scans the input twice, and each pass carries the `Header` row loaded for that
system ([src/database.rs](../../src/database.rs#L3031-L3047)). For directory
inputs the `WalkDir` walk restarts inside that loop, so files claimed by an
earlier pass have already been moved out and are not seen again.

The header is what unlocks headered matching. When `header` is `Some`, hashing
goes through `get_headered_hash_and_size`, which reads the first `header.size`
bytes, checks every `rules` row against them, and hashes *without* the header only
if all rules match ([src/common.rs](../../src/common.rs#L575-L671)). Systems whose
DAT declares a header definition therefore only match headered dumps **when named
with `-s`**; without it the file is hashed whole and, if the DAT is headerless,
lands in Trash. The pair `test_original_headered` / `test_original_trash` pins
exactly this: the same 272-byte file imports with a header and is trashed without
one.

## Input ordering and `--extract`

Before touching the database, `main` assigns each input path a priority: **child
CHDs first (0), parentless CHDs second (1), everything else last (2)**
([src/import_roms.rs](../../src/import_roms.rs#L171-L216)). This exists because
`as_chd` resolves a CHD's `Parent SHA1` by scanning *the directory the child sits
in* ([src/chdman.rs](../../src/chdman.rs#L636-L702)); importing — and therefore
moving away — the parent first would strand every child in the batch.

Each path then gets its own temp directory and is canonicalised. With `-x`, a
top-level archive (as reported by `get_mimetype`, membership in
`ARCHIVE_EXTENSIONS`) has all of its members extracted into the temp directory,
which is then walked as if it had been the input
([src/import_roms.rs](../../src/import_roms.rs#L218-L237)). Without `-x`, archives
are imported as containers. `test_zip_single_file_extract` is the fixture for this
path: a `.rom.zip` becomes a `.rom` in the system directory.

Directories are walked with `WalkDir`. Two things differ between directory mode
and the single-file mode:

- A directory containing `PS3_DISC.SFB` is treated as a **jukebox folder** and
  handed to `import_jbfolder` instead of being walked (see below).
- In directory mode an `Err` from `import_rom` is printed and the walk continues;
  a single file propagates the error and fails the run.

Both the directory walk and its error path keep output on the `progress` helpers,
which write to stderr; the walk's `Err` arm carries the guard comment that keeps
this code stdout-silent — under `oxyromon mcp` stdout *is* the JSON-RPC channel, so
a stray `println!` here would corrupt the protocol stream
([src/import_roms.rs](../../src/import_roms.rs#L307-L316),
[src/mcp.rs](../../src/mcp.rs#L1-L20)).

## import_rom(): one transaction per file

`import_rom` is the unit of work. It opens a transaction on entry and commits
exactly once on the success path
([src/import_roms.rs](../../src/import_roms.rs#L403-L599),
[src/database.rs](../../src/database.rs#L74-L85)). Every early return — empty
file, already in database, missing external tool — returns `Ok` **without
committing**, so the transaction is dropped and rolled back; only the filesystem
side effects those branches performed (a delete) survive. Nothing partial ever
reaches the database.

```mermaid
flowchart TD
    S["main: per input path, per selected system"] --> JB{"directory contains PS3_DISC.SFB"}
    JB -->|"yes"| JF["import_jbfolder (needs a system)"]
    JB -->|"no"| W["walk files, call import_rom per file"]

    W --> A["import_rom: begin transaction"]
    A --> B{"size equals 0"}
    B -->|"yes"| B1["delete file, return without commit"]
    B -->|"no"| C{"path already in romfiles and no --force"}
    C -->|"yes"| C1["skip, return without commit"]
    C -->|"no"| D["extension from mimetype, else from filename"]
    D --> E{"dispatch on extension"}
    E -->|"7z or zip"| F["import_archive"]
    E -->|"chd"| G["chdman required, then import_chd"]
    E -->|"cia"| H["ctrtool required, then import_cia"]
    E -->|"cso or zso"| I["import_cso / import_zso"]
    E -->|"nsz"| J2["import_nsz"]
    E -->|"rvz"| K["import_rvz"]
    E -->|"other, or anything with --as-is"| L["import_other"]

    F --> M["commit transaction"]
    G --> M
    H --> M
    I --> M
    J2 --> M
    K --> M
    L --> M
    E -.->|"external tool missing"| N["print error, return without commit"]
    F -.->|"no match"| O["trash with -t, delete with -d"]
    L -.->|"no match"| O
    O --> M
    M --> P["return system_ids and game_ids"]
```

*Figure: the per-file boundary. Every path out of `import_rom` before the commit leaves the transaction uncommitted; `--as-is` routes every extension to `import_other`.*

Two cheap guards run first: a zero-byte file is deleted outright, and a file whose
path is already a `romfiles.path` is skipped unless `--force`
([src/import_roms.rs](../../src/import_roms.rs#L427-L448)). The second guard only
fires once the file is *inside* a ROM directory — `get_relative_path` fails for
anything outside it — so re-running an import from a staging directory falls
through to matching, where the `Duplicate` result handles it.

Then the format is decided: `get_mimetype` first (magic bytes), falling back to the
lowercased filename extension ([src/mimetype.rs](../../src/mimetype.rs#L100-L122)).
Magic wins, which is why a mis-named file still takes the right branch — and why a
future RVZ version that the matcher does not recognise degrades to the extension.

The dispatch is an ordered `if`/`else if` chain in which every specialised branch
is additionally gated on `!as_is`: archive, CHD, CIA, CSO, NSZ, RVZ, ZSO, and
finally `import_other`. `--as-is` therefore does two things at once — it sends
every file to `import_other` (hash the container as it sits, move it, no
decompression), and it suppresses the auto-conversion pass below.

## Matching a file to a catalog rom

All eight format handlers funnel into
[`find_rom_by_size_and_hash`](../../src/import_roms.rs#L2162-L2499), which answers
one question per hash algorithm: *which catalog roms could this be?*

- **Progressive narrowing.** Three queries in order, stopping at the first that
  returns anything: `name + size + hash + game names`, then `size + hash + game
  names`, then `size + hash` alone. Each has a system-scoped variant added by
  `-s`. Game names come from the file stem and, inside an archive, from the entry's
  parent directory; the rom name from the entry name. Every query carries
  `romfile_id IS NULL AND parent_id IS NULL`, so only unclaimed roms compete — and
  not the merged arcade roms whose `parent_id` points at the parent-game or BIOS
  copy ([src/database.rs](../../src/database.rs#L2084-L2148)).
- **Algorithm order.** `HashAlgorithm::iter()` tries CRC, then MD5, then SHA1. The
  CHD handler uses `iter().rev()` so SHA1 goes first, because CHD DATs are
  SHA1-native ([src/import_roms.rs](../../src/import_roms.rs#L1050)).
- **`Duplicate` vs `Invalid`.** If nothing unclaimed matches, a follow-up
  `count_roms_with_romfile_*` decides between "already imported" (`Duplicate`) and
  "no match" (`Invalid`). That distinction is what feeds Trash subfolder names and
  the `--trash` decision.
- **Ambiguity ladder.** One candidate wins immediately. Otherwise: a candidate
  whose game was already imported *during this run* (the `game_ids` set threaded
  through every handler) wins; then, under `--unattended first`, the closest name
  by `jaro_winkler` against the file name wins; then anything unattended that is
  left returns `Skipped` rather than hanging on a prompt nobody will answer. An
  interactive run always gets a prompt — labelled `rom (game)` when `-s` narrowed
  the search, `rom (game) [system]` when it did not
  ([src/import_roms.rs](../../src/import_roms.rs#L2403-L2471)).
- **Already-linked rom.** If the chosen rom already has a `romfile_id`, the result
  is downgraded to `Duplicate`, which protects one-file-per-rom even when the path
  check at the top of `import_rom` did not catch it.

## Where the file goes

Placement is per-format, and the rule of thumb is: **keep the container when it
represents exactly one complete game, otherwise take the members out.**

| Case | On disk | Rows |
| --- | --- | --- |
| Plain file (`import_other`) | moved to `get_sorted_path(...)` | 1 romfile → 1 rom |
| Archive = whole game | archive kept intact, entries renamed inside it to the catalog rom names; the container becomes `<game name>.<archive ext>` when it holds several entries, and `<rom name stem>.<archive ext>` when it holds one — unless the system is arcade or the entry is a PS3 `.pkg`/`.pup`/`.rap`, which take `<game name>.<archive ext>` | 1 romfile → *n* roms |
| Archive = mixed or partial game | each matched member extracted and **copied** to the system directory (arcade and jbfolder games get a `<game>/` subfolder) | 1 romfile per member |
| CD CHD | unpacked to cue/bin in tmp; only if *every* track matches one game **and** one system is the CHD moved as `<game>.chd` | 1 romfile → 1 rom per track |
| DVD / HD / LD CHD | hashed by extracting (or reusing the CHD's own SHA1), moved as `<rom>.chd`; the HD and LD branches put arcade CHDs under a `<game>/` subfolder | 1 romfile → 1 rom |
| CIA | parts extracted with `ctrtool` for hashing; whole `.cia` moved as `<game>.cia` | 1 romfile → *n* roms |
| CSO / ZSO / NSZ / RVZ | container kept, renamed `<rom.name>.<ext>` after hashing the decoded image | 1 romfile → 1 rom |

The archive decision is the interesting one
([src/import_roms.rs](../../src/import_roms.rs#L937-L1035)): the archive is kept
intact only when every entry matched, all matches belong to a single game, *and*
the matched set covers every rom of that game
(`find_roms_by_game_id_no_parents`). Anything short of that falls through to the
per-member extraction branch, which **copies** the matched members out; the source
archive is never moved or removed by that branch, and only the extracted members
get `romfiles` rows (an archive left behind inside a ROM directory is exactly what
`purge-roms --foreign` reports). Unmatched members stay inside it; only a
**single-entry** archive whose sole entry fails all three algorithms is itself
trashed ([src/import_roms.rs](../../src/import_roms.rs#L924-L934)).

Hashing compressed formats is delegated to the wrapper types in the
[trait system](../concepts/romfile-trait-system.md): `ArchiveRomfile` reuses the
CRC already recorded in the archive's central directory instead of extracting
([src/archive.rs](../../src/archive.rs#L194-L215)), `XsoRomfile` and `NszRomfile`
decompress into a temp directory and delete the result
([src/xso.rs](../../src/xso.rs#L52-L70)), `RvzRomfile` hashes while decoding with
no ISO ever written ([src/rvz.rs](../../src/rvz.rs#L44-L67)), and `ChdRomfile`
answers SHA1 from `chdman info` when it can
([src/chdman.rs](../../src/chdman.rs#L179-L238)). The consequence visible from
the tests: a `.cso` imports against the catalog entry for the `.iso`
(`test_cso`), and a `.rvz` against `Test Game (USA).iso` (`test_rvz`) — while
`test_rvz_as_is` needs a DAT that publishes the RVZ's *own* hash, because
`--as-is` hashes the container.

CD CHDs also carry a version gate: a multi-track CHD needs
`chdman >= MIN_SPLITBIN_VERSION` (`0.265`) or the import is refused with a warning
before anything is written
([src/import_roms.rs](../../src/import_roms.rs#L1052-L1069),
[src/chdman.rs](../../src/chdman.rs#L18-L20)).

Every branch ends in the same persistence helper
([src/import_roms.rs](../../src/import_roms.rs#L2609-L2637)): build a
`CommonRomfile` scoped to the system, strip the (possibly per-system)
`ROM_DIRECTORY` prefix to get the stored relative path
([src/common.rs](../../src/common.rs#L354-L367)), update the `romfiles` row at that
path or create one with `RomfileType::Romfile`, then set `roms.romfile_id` for each
matched rom. The romfile row is deliberately shared: many roms may point at one
path, which is what makes multi-file archives, multi-track CHDs and CIA sets
representable.

## Rejected files: nothing, Trash, or deletion

If no algorithm matches, `trash_or_delete_romfile` runs
([src/import_roms.rs](../../src/import_roms.rs#L2639-L2699)):

- **Neither flag** — the file stays exactly where it was. Import is non-destructive
  by default.
- **`-d` / `--delete`** — hard delete, no database row.
- **`-t` / `--trash`** — moved to `<system or global Trash>/<Valid|Duplicate|Invalid|Skipped>/<stem> [algo:hash].<ext>`,
  with the computed hash baked into the filename so a later manual inspection says
  *why* it failed. If that path already exists the file is deleted instead, and the
  trashed file gets its own `romfiles` row.

Trash is a directory convention rather than a flag, and `purge-roms --trash` is the
only thing that empties it — see
[Integrity Checking, Purging and Trash](integrity-check-and-purge.md).

## Jukebox folders

A directory containing `PS3_DISC.SFB` is a PS3 JB folder and never reaches
`import_rom` ([src/import_roms.rs](../../src/import_roms.rs#L602-L792)). The rules
that make it different:

- It **requires a system**. The folder names no system of its own, so without `-s`
  the pipeline either prompts (`prompt_for_system_like(connection, None,
  "%PlayStation 3%")`) or, when unattended, warns and skips.
- Matching is **MD5 only**, because IRD catalogs only publish MD5s. The `SFB` file
  identifies the game; each remaining file is looked up by `size + md5 +
  sfb_rom.parent_id`, disambiguated by filename suffix and then by `jaro_winkler`.
- Files are moved into `<system directory>/<game name>/<rom name>`, and empty
  directories are pruned behind them.
- It returns only a `system.id`. Since the auto-conversion pass works from the
  *game* set, a run whose only imports are jukebox folders converts nothing.
- Trashing and deleting are not implemented here — a `TODO` leaves unmatched
  folders untouched.

The IRD side that makes these games exist is covered in
[IRD, PS3 Jukebox Folders](ird-ps3-jbfolders.md).

## After the files: auto-conversion, then completion

`main` accumulates the `(system_ids, game_ids)` pairs returned by every
`import_rom` call, and only after **all** inputs are committed runs
`auto_convert` — skipped entirely under `--as-is`
([src/import_roms.rs](../../src/import_roms.rs#L343-L400)). It groups the imported
games by system, reads `PREFER_FORMAT` for that system (per-system setting with the
global fallback of `get_setting`), and calls `convert_system` with
`ConvertOpts { recompress: false, diff: false, check: true, prompt_for_parents: false }`.

The ordering is the contract: a failed conversion never rolls back an import. The
ROM simply stays in the format it was imported in, with a warning. `check: true`
means each produced file is verified against the catalog before the source is
deleted — a failed check deletes the conversion and keeps the original
([src/convert_roms.rs](../../src/convert_roms.rs#L1946-L1993)). How
`PREFER_FORMAT` itself is validated — arcade systems reject everything outside
`ORIGINAL` and `ZIP` — is covered in
[Settings and Configuration](../architecture/settings-and-configuration.md).
`test_prefer_format` covers the four observable combinations: converts when set,
untouched when unset, untouched under `--as-is`, and `convert-roms --save` making
the choice permanent.

Finally `compute_system_completion` runs for each touched system
([src/util.rs](../../src/util.rs#L422-L492)). It also creates zero-byte placeholder
files for size-0 roms in partial games, so a partially imported set is visible on
disk as well as in the database.

## Failure modes worth remembering

| Situation | Behaviour |
| --- | --- |
| `chdman` missing for a `.chd` | error printed, file untouched, transaction rolled back, run continues |
| `ctrtool` missing for a `.cia` | same as above |
| Empty file | deleted immediately, no rows |
| Path already in `romfiles` | skipped unless `--force` |
| Ambiguous match, `--unattended skip` | `Skipped`, file left in place |
| Ambiguous match, `--unattended first`, no file name to rank | `Skipped` (the `first` heuristic needs a name) |
| Multi-entry archive with one bad member | bad member stays in the archive, good members extracted |
| Single-entry archive with no match | trashed or deleted per `-t` / `-d` |
| CD CHD whose tracks do not resolve to exactly one game and one system | import aborted; `CRC mismatch` plus trash/delete, but only when at least one track failed to match |
| Conversion failure after import | warning only; import stands |
| Error inside a walked directory | printed, walk continues; a top-level single file fails the run |

## Focused tests

`src/import_roms/` holds one file per behaviour rather than one per function, and
each is a full given/when/then over a temp database and temp ROM directory:

- `test_original`, `test_original_headered`, `test_original_trash`,
  `test_original_delete` — the happy path plus the three header/rejection outcomes.
- `test_sevenzip_single_file` (container kept, re-extensioned to `.7z`),
  `test_sevenzip_multiple_files_full_game` (one romfile, three roms),
  `test_sevenzip_multiple_files_mixed_games` (two games in one archive → two
  romfiles), `test_sevenzip_multiple_files_partial_game` (missing track → members
  extracted), `test_sevenzip_multiple_files_headered_mixed_games` (header rules
  applied per archive entry), `test_zip_single_file_extract` (`-x`).
- `test_single_track_chd`, `test_multiple_tracks_chd`, `test_iso_chd`,
  `test_mame_chd` (arcade CHD lands under a `<game>/` subfolder),
  `test_mame_mixed` (loose ROM plus RDSK CHD in one system).
- `test_cso`, `test_zso`, `test_nsz`, `test_rvz`, `test_rvz_as_is`, `test_cia`.
- `test_per_system_rom_directory` — with a per-system `ROM_DIRECTORY` override, the
  stored path is relative to that override and the file is *not* present under the
  global ROM directory.
- `test_prefer_format` — the auto-conversion matrix described above.

## Related

- [File Type Detection and Extension Constants](../concepts/file-type-detection-and-extensions.md) — the magic-byte matcher and the `ARCHIVE_EXTENSIONS` / `NON_ORIGINAL_EXTENSIONS` groups this dispatch reads.
- [The CommonRomfile Trait System](../concepts/romfile-trait-system.md) — the `As*`/`To*`/`HashAndSize`/`Persist` verbs each handler calls.
- [DAT Ingestion](dat-ingestion.md) — what must have run before an import can match anything, and the orphan re-import that reuses `import_rom`.
- [Format Conversion and Export](format-conversion-and-export.md) — the `convert_system` entry point auto-conversion calls.
- [Integrity Checking, Purging and Trash](integrity-check-and-purge.md) — what happens to the files `import-roms -t` retired.
