---
type: integration
title: DAT Providers and Naming Conventions
description: The external catalog providers oxyromon knows about (No-Intro dat-o-matic, Redump, MAME/FBNeo), the provider URLs and Redump system-code catalogue that drive download and update-check, the download path's pacing and save option, and how shiratsu-naming No-Intro/TOSEC tokens become the regions, languages, flags and version order used by both DAT import and sorting.
tags: [dat-providers, no-intro, redump, mame, fbneo, download-dats, naming-tokens, regions]
sources:
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-50f94ae1d9f2b065641770f1
    resource: repo://crates/wux-rs/src/nus.rs
  - id: openwiki-source-66de04ab1b5b16fdc2ef21f2
    resource: repo://data/No-Intro_NES.xml
  - id: openwiki-source-9594fdc8eb7c8cc0ab2dd040
    resource: repo://frontend/src/components/download_dat_modal.rs
  - id: openwiki-source-720480293ae71568113cbf54
    resource: repo://migrations/20261026000001_move_redump_to_redump_info.sql
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-68a9457efd4964d6e3fa17be
    resource: repo://src/download_dats.rs
  - id: openwiki-source-0e518f793c5021d3e200387b
    resource: repo://src/download_dats/test_nointro_failures.rs
  - id: openwiki-source-885c8766d237f8340462593b
    resource: repo://src/download_dats/test_nointro.rs
  - id: openwiki-source-dba7f68f124dba00236ec096
    resource: repo://src/download_dats/test_redump_failures.rs
  - id: openwiki-source-904cc3411b962b115194d55e
    resource: repo://src/download_dats/test_redump.rs
  - id: openwiki-source-316c0afa4f03f2eedd9022b5
    resource: repo://src/generate_playlists.rs
  - id: openwiki-source-6f95130cb448e88dd97ab560
    resource: repo://src/import_dats.rs
  - id: openwiki-source-2d4a135c93b4a3bee8970761
    resource: repo://src/import_dats/test_dat_arcade.rs
  - id: openwiki-source-d7a0d0f607461a016e622fb2
    resource: repo://src/import_dats/test_dat_custom_name_forced.rs
  - id: openwiki-source-733a84bef8a992742971d01c
    resource: repo://src/import_dats/test_dat_custom_name_revert.rs
  - id: openwiki-source-2843a1f9a1aeeffbc72b2e2d
    resource: repo://src/import_dats/test_dat_custom_name_updated.rs
  - id: openwiki-source-97dfaec832a2274db4c8c770
    resource: repo://src/import_dats/test_dat_headered_embedded.rs
  - id: openwiki-source-9d464240a7b7d0d02b932ef0
    resource: repo://src/import_dats/test_dat_outdated_forced.rs
  - id: openwiki-source-fd6c00ca00a8f9a4acc04e35
    resource: repo://src/import_dats/test_regions_france_germany.rs
  - id: openwiki-source-4e7875940682296f157f4fb7
    resource: repo://src/import_dats/test_regions_world.rs
  - id: openwiki-source-30ed0eedcb90a24300e7a1fe
    resource: repo://src/import_dats/test_zip_update_unknown_system.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-b23fc7fb5544ff535eae8c43
    resource: repo://src/mcp/prompts.rs
  - id: openwiki-source-f6b27667092492e425164ff8
    resource: repo://src/mcp/read.rs
  - id: openwiki-source-762e9c43ab8143e7dda872d3
    resource: repo://src/mcp/write.rs
  - id: openwiki-source-cc76cc07825d7eb399a2b402
    resource: repo://src/model.rs
  - id: openwiki-source-528b2bf14ffa06b3842b8c96
    resource: repo://src/mutation.rs
  - id: openwiki-source-f314ed3b08dd849d591fdb0f
    resource: repo://src/query.rs
  - id: openwiki-source-0a9271029eb893aacec54c15
    resource: repo://src/sort_roms.rs
  - id: openwiki-source-03bd0794be72fa393bc0353c
    resource: repo://src/sort_roms/test_order_prefer_versions_new_revision_vs_revision.rs
  - id: openwiki-source-e76904b48a6a0001f21994a4
    resource: repo://src/sort_roms/test_order_prefer_versions_old_revision_vs_vanilla.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-03238ed8280795dfd0fd3fae
    resource: repo://tests/header.xml
  - id: openwiki-source-0cc1fa6981e9c7b322a64061
    resource: repo://tests/profile.xml
  - id: openwiki-source-b6fbe21cd25e7e095cc8c367
    resource: repo://tests/Test%20System%20(20200721)%20(MAME).dat
  - id: openwiki-source-cc9a39413c8a670060b6243d
    resource: repo://tests/Test%20System%20(20261025)%20(CDN).dat
  - id: openwiki-source-706217a392bda126ca145591
    resource: repo://tests/Test%20System%20(20261025)%20(WUD).dat
generated: { by: "claude-code", at: "2026-10-26T22:40:03.236Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T22:40:03.236Z
---

# DAT Providers and Naming Conventions

oxyromon never talks to a catalog except through a Logiqx DAT file. Three provider
families feed it, and only one of them can be reached over the network.

| Provider | Contents | How it enters oxyromon |
| --- | --- | --- |
| No-Intro (dat-o-matic) | cartridge consoles, older computers | `import-dats` with a hand-downloaded DAT; `download-dats -n -u` can **only check for updates** |
| Redump (redump.info) | CD/DVD/BD optical systems | `import-dats`, or `download-dats -r` which **fetches and imports** brand-new and refresh DATs |
| MAME / FBNeo | arcade | `import-dats` only (Progetto-Snaps for MAME, the libretro FBNeo repo for FBNeo, with `-a` for FBNeo); never fetched |

The asymmetry is documented in the README (where "Find a way to automatically download
No-Intro DAT files" is still an open TODO) and encoded in `download_dats::main`:
No-Intro publishes a summary (`profile.xml`) but no direct DAT links, so the No-Intro
branch prints `Not supported` unless `-u` is passed; Redump publishes direct
`/datfile/<CODE>` ZIPs but no summary, so it can both download and update. The
`-n/--nointro` and `-r/--redump` flags conflict with each other and one of them is
required (`required_unless_present`). Of the remaining flags, `-u` and `-a` reach both
providers, but `-f/--force` and `-s/--save` are only passed down the Redump branches —
on the No-Intro path clap accepts them and nothing consumes them.

Nothing in the DAT path shells out to an external program: the Redump ZIP is read by
the in-tree `zip` crate, and the README states that every format is handled in-process,
so oxyROMon needs no external tool. The format tools oxyromon once launched — `chdman`
and `ctrtool` among them — were replaced by the crates under `crates/`; see
[External Tools](external-tools.md).

The Wii U shows how a provider's DAT shape reaches the import code. No-Intro's
CDN DAT lists each update or DLC as a game whose ROMs are the title's files under
the CDN's names: `tmd.<version>`, `cetk.<version>`, and a bare content ID such as
`00000000` with its `.h3`. `import-roms` keys on that TMD to import a title folder
whole, and a downloader's `title.tmd` and `.app` names match too. Redump's disc DATs
list each disc as an `.iso` instead, which a WUX or a lossless WUA is matched
against. See [Wii U Formats and wux-rs](../concepts/wii-u-formats-and-wux-rs.md).

Arcade versus console is not a provider switch but a DAT-shape switch: a DAT whose
entries are `<machine>` rather than `<game>` sets `systems.arcade = true`
(`arcade || !datfile_xml.machines.is_empty()` in `import_dat`). FBNeo's DATs list
`<game>` entries, so they need `import-dats -a/--arcade` to import as arcade;
`romof` cannot stand in for detection, since FBNeo's console DATs carry it too. A
plain update keeps a system arcade, and a forced import applies the flag as given.
Redump downloads always pass `arcade = false`. The flag later disables name-based
region parsing at import and routes sorting through `REGIONS_*_ARCADE` instead of
regions.

## Provider identity is stored on the system

Two constants define provider provenance and they are written into `systems.url`
verbatim from the DAT header (`<url>` of a No-Intro DAT is `www.no-intro.org`, of a
Redump DAT `https://redump.info/`):

```rust
pub const NOINTRO_SYSTEM_URL: &str = "www.no-intro.org";
pub const REDUMP_SYSTEM_URL: &str = "https://redump.info/";
```

`find_systems_by_url` is then the only way the code asks "which of my systems came
from this provider", and it scopes three distinct behaviours: the No-Intro update
check's candidate list, the Redump update list, and `generate-playlists`, which
restricts its system picker to Redump because multi-disc `Disc N` games only exist
there. Provenance therefore depends on the DAT header being truthful, not on how the
DAT was obtained — a Redump DAT imported by hand is just as "Redump" as one fetched
by `download-dats`.

Redump moved from `http://redump.org` to `https://redump.info`, and the new DAT
headers carry the new URL. Because provenance is an exact string match, the
`move_redump_to_redump_info` migration rewrites existing systems' `url` so they stay
in the Redump lists, and gives the twelve systems Redump renamed their new names,
keeping the old name as `custom_name` so their directories do not move. A rename
that would collide with an existing name or custom name is skipped, and that row
stays on the old URL. See
[Data Model, SQLite Usage and Migrations](../architecture/data-model-and-migrations.md).

## The Redump system-code catalogue

`REDUMP_SYSTEMS_CODES` is a compile-time `phf_map!` of the exact Redump system name
to the uppercase Redump system code used in the datfile URL (`Nintendo - GameCube` =>
`GC`, `Sony - PlayStation` => `PSX`, `Arcade - Sega - Naomi 2` => `NAOMI2`, …). The
names are the DAT headers' `<name>`, not the labels on redump.info's downloads page,
which differ (`Sony PlayStation` on the page, `Sony - PlayStation` in the DAT). It is the
whole set of systems oxyromon can fetch, and it is wrapped in `cfg_if!`:

```rust
cfg_if! {
    if #[cfg(test)] {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "Test System" => "TS"
        };
    } else {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! { /* 111 systems */ };
    }
}
```

Under `cfg(test)` the catalogue collapses to a single `Test System` => `TS` entry.
That is what keeps the download tests offline and deterministic: a `download_redump_dats`
run with `-a` iterates exactly one system and therefore fetches exactly one URL, and the
failure tests can hard-code that URL as `/datfile/TS`. No production code path differs —
only the data. Adding a Redump system means adding one line to the production half of
this map; nothing else needs to change.

Both consumer lists are drawn **from the catalogue**, intersected with the database:

- `find_redump_systems(connection, update)` keeps catalogue keys whose name is absent
  from (`update = false`) or present in (`update = true`) the set of systems whose
  `url` is `REDUMP_SYSTEM_URL`, then sorts by name. The two halves are complementary,
  which is why the web UI's *Update only* switch simply re-reads the list.
- `download_redump_dats` recomputes the same "not yet imported" set locally before
  offering the multiselect.

Because a name is only ever offered if the catalogue holds its code, a system
imported from a hand-downloaded Redump DAT under a name the catalogue does not know
is never offered for a download. Note the asymmetry: the *update* list
(`update_redump_dats`) is built from the database via `prompt_for_systems(..., Some(REDUMP_SYSTEM_URL), ...)`, not from the catalogue, so it can yield a name with no catalogue code, such as a system whose rename the migration skipped. `fetch_redump_dat` then fails that one system with `Unknown Redump system`, and the batch fails at the end naming it.

## Fetch control flow

```mermaid
flowchart TD
    Start["download-dats main"] --> P{"nointro or redump"}
    P -->|"nointro, no update"| W["warning: Not supported"]
    P -->|"nointro update"| NI["GET datomatic.no-intro.org/profile.xml"]
    NI --> NIM["match DB systems by profile name"]
    NIM --> NIC{"is_update db vs profile"}
    NIC -->|newer| NIOk["report available update"]
    NIC -->|older or equal| NISkip["skip or already up to date"]
    NIM -->|"no profile entry"| NIGone["System is no longer available"]
    P -->|redump| R{"update flag"}
    R -->|off| RL["catalogue minus imported systems"]
    R -->|on| RU["DB systems with redump url"]
    RL --> SEL["multiselect, or all with the all flag"]
    RU --> SEL
    SEL --> BATCH["download_redump_batch"]
    BATCH --> COL["collect_redump_dats"]
    COL --> DL["download_redump_dat per system"]
    DL --> GET["GET base/datfile/CODE, HTTP errors included"]
    GET -->|"fails"| ERR["error for this system"]
    GET --> Z{"ZIP entry count"}
    Z -->|"zero or several"| ERR
    Z -->|one| SAVE{"save dir given"}
    SAVE -->|yes| CP["copy DAT into save dir"]
    SAVE -->|no| PARSE
    CP --> PARSE["parse_dat with skip_header on"]
    PARSE --> IMP["import_dat with the force flag"]
    IMP -->|"newer or new"| IMPORTED["Imported"]
    IMP -->|"no newer"| UPTODATE["UpToDate"]
    IMPORTED --> RATE["sleep one second"]
    UPTODATE --> RATE
    ERR --> RATE
    RATE --> NEXT{"more systems"}
    NEXT -->|yes| DL
    NEXT -->|no| END["fail the run naming every failed system"]
```

Per-system fetch is `download_redump_dat`, which wraps `fetch_redump_dat`:
`GET {base}/datfile/{code}` into memory, rejecting an HTTP error status,
`ZipArchive::new` over the bytes, extraction into a `TempDir` under
`TMP_DIRECTORY`, then `parse_dat(..., skip_header = true)` and
`import_dat(..., None, None, force)`. It returns a `DatOutcome`: `Imported` when
`import_dat` produced a summary, and `UpToDate` when it returned `None` because
the DAT was no newer than the imported one. Everything else is an error for that
system: an unreachable host, an HTTP error status, a body that is not a ZIP, or a
ZIP that does not hold exactly one DAT. The failure test asserts each of those
shapes returns `Err` and leaves `find_systems` empty, and the Redump test asserts
that downloading the same DAT again is `UpToDate` while forcing it is `Imported`.

Pacing and failure handling:

- **Rate limiting** is one `sleep(Duration::from_secs(1))` after every system,
  failures included, applied by `download_redump_dat` around the fetch. There is no
  retry or backoff.
- **Batches carry on past a failure.** Every surface loops through one collector,
  `collect_redump_dats`, which prints each system's error, moves on to the next,
  and returns a `DatOutcomes` sorting every system into `imported`, `up_to_date`
  or `failed` (the name with its error). Both CLI paths go through
  `download_redump_batch`, which fails the run at the end with
  `Failed to download: …` naming every system that failed, so the exit status
  still reports it.
- **Unknown names** are rejected in `fetch_redump_dat`, where the code is looked
  up in `REDUMP_SYSTEMS_CODES`: `Unknown Redump system "..."` is an error for that
  system only.
- **`-s/--save <DIRECTORY>`** copies the extracted `.dat` (not the ZIP) into a
  directory created on demand, preserving its provider file name. This is the only way
  to keep a fetched DAT: the extraction `TempDir` is dropped when the fetch
  returns. The Redump test asserts the saved `Test System (20200721).dat` exists.

`-f/--force` is threaded only into the update path (`download_redump_dats` always
passes `force = false`), where it overrides the version gate below.

## Version gates and update checks

`is_update` is a plain lexicographic `new_version.cmp(old_version)`: `Less` prints
`Skipping older version`, `Equal` prints `Already up to date`, `Greater` prints
`Updating from x to y` and returns `true`. Both the No-Intro update check and the DAT
import gate use it — `create_or_update_system` returns `None` (skip the whole import)
unless `is_update(...) || force`. Because the comparison is textual, providers'
fixed-width date stamps (`20200721`) are what make it correct; `create-dats` follows
the same convention by defaulting a DAT version to the creation datetime. The tests
`test_dat_outdated_should_do_nothing` (6 games survive an older DAT) and
`test_dat_outdated_forced` (the same older DAT, with `force`, replaces the catalog
with its 3 games) pin both sides.

The same `force` flag also decides what happens to a system's custom name and extension.
On a plain update the stored customizations win — `custom_name.or(system.custom_name)`
— while a forced import applies the customizations exactly as given, so an omitted
`-n`/`-e` reverts to the DAT header value. `test_dat_custom_name_updated`,
`test_dat_custom_name_forced` and `test_dat_custom_name_revert` pin the three
combinations.

`update_nointro_dats` fetches `https://datomatic.no-intro.org/profile.xml`,
deserializes it into `ProfileXml` (a wrapper aliasing each `<datfile>` element to a
`SystemXml`, so only name and version matter) and, for each system in the database
whose url is `NOINTRO_SYSTEM_URL`, looks up the profile entry by exact name:

- found → `is_update(progress_bar, &system.version, &system_xml.version)` prints the verdict;
- not found → `System is no longer available`;
- nothing is ever downloaded. A profile that cannot be fetched (an unreachable host
  or an HTTP error status) is an error, as is a body that fails to deserialize
  (`Failed to parse No-Intro profiles`); `test_nointro_failures` pins the first two.

## Server and MCP surfaces

The same two primitives are reused by the `server` and `mcp` features, guarded by
`#[cfg(any(feature = "server", feature = "mcp"))]`:

- `find_redump_systems` backs the GraphQL `downloadableSystems(update:)` query, the
  MCP `downloadable_systems` tool, and the Leptos *Import DATs from Redump* modal
  (the *DATs from Redump…* item of the navbar's Import menu), which re-reads
  the list whenever it opens or flips *Update only*.
- `download_redump_systems` backs the GraphQL `download_dats` mutation and the MCP
  `download_dats` tool. It runs `collect_redump_dats` against the real Redump URL
  without a save directory, and each caller only maps the returned `DatOutcomes`
  to its own shape. Unlike the CLI, which can only offer names it already holds,
  these callers accept arbitrary strings; an unknown name lands in `failed`.
- The GraphQL mutation returns immediately and runs the fetches on a `tokio::spawn`
  task, reporting `download_dats_started`, then `download_dats_complete` — counting
  imported and already up-to-date DATs — or `download_dats_error` with the list of
  failed system names over SSE. Per-system events are deliberately suppressed: the
  selection can be the whole catalogue and each event is a UI toast. Individual
  failures are logged and collected rather than aborting the batch.
- The MCP tool likewise carries on past a failure and returns each system in one of
  three lists — `imported`, `up_to_date`, `failed` with its error — and is a tool
  error only when every system failed. It is refused by `read_only_denial` when the
  server runs read-only.

## shiratsu-naming: one parser behind regions, languages, flags, versions

`import_dats`, `sort_roms`, the GraphQL `game_information` resolver and the MCP
`parse_game_name` tool all parse game names with the `shiratsu-naming` crate rather
than each keeping its own regex. This is the single source of truth for what a token
means; changing region or language handling means changing this crate, not four
call sites.

**At import time** only regions are extracted, via `get_regions_from_game_name`:
`NoIntroName::try_parse` is tried first, and only if it fails does `TOSECName::try_parse`
run. The first `Region` token found is normalized with
`Region::to_normalized_region_string`, which yields hyphen-joined codes —
`Test Game (World)` → `US-JP-EU`, `Test Game (France, Germany)` → `FR-DE` — and stored
in `games.regions`. Two rules matter:

- Arcade systems skip parsing entirely and store an empty region string; arcade names
  are not No-Intro names.
- If both parsers fail, the game is **skipped with a warning** (`Failed to parse name: …`)
  after bumping the progress bar — not imported with empty regions, and not a fatal error.

**At sort time** the same tokens drive selection and ordering:

- `trim_ignored_games` partitions games out of the election when a `Languages` token
  contains no language in `LANGUAGES` (codes filtered by `LANGUAGE_REGEX` `[A-Z][a-z]`
  and variants by `VARIANT_REGEX` `[A-Z]{2}`), when a `Release` token's status is in
  `DISCARD_RELEASES`, or when a `Flag` token — split on `", "` — matches `DISCARD_FLAGS`.
  For arcade systems the same function ignores `comment` substrings instead, because
  arcade names carry no tokens.
- `sort_games_by_weight` scores a candidate +1 each for being a parent
  (`PREFER_PARENTS`), for matching the `PREFER_REGIONS` `broad`/`narrow` preference by
  comparing the number of regions parsed back with `Region::try_from_tosec_region`, for
  winning the `PREFER_VERSIONS` comparison, and once per `PREFER_FLAGS` match. Higher
  weight sorts first.
- `compare_versions` reads `NoIntroVersion` tokens numerically (`major`/`minor` parsed
  as `u64`, unparseable or absent treated as `0`), and a game that *has* a version token
  always beats one that does not: `Game (USA) (Rev 2)` outranks `Game (USA)` under
  `new` and is outranked by it under `old`.

Because the stored `games.regions` string is itself a shiratsu-norm region string,
sorting round-trips it through `Region::try_from_tosec_region` — a DAT that imports
with an unparseable region string silently sorts as no regions at all.

The parser is also exposed read-only: GraphQL `game_information` and the MCP
`parse_game_name` tool both return `title`, `regions`, `languages`, `release` and
`flags` from the same token walk, so an agent or UI can reason about disc and region
variants without importing anything.

## Header detectors that ride along with a DAT

A DAT may point at a header-skipping detector through `<clrmamepro header="..."/>`.
`parse_dat` looks for the first `clrmamepro` element that has a `header` attribute
(multiple `clrmamepro` elements are legal — one with `header`, another with
`forcenodump`), then resolves the file next to the DAT, and falls back to an
**embedded** copy shipped from `data/` via `#[derive(RustEmbed)] struct Assets`. That
is why a No-Intro NES DAT referencing `No-Intro_NES.xml` imports with a header even
when the detector file was not downloaded; `tests/header.xml` is the sibling-file form,
with its `start_offset` and `<data offset value>` rules persisted as a `Header` plus
`Rule` rows by `create_or_update_header`. `-s/--skip-header` suppresses all of it, and
the Redump download path passes `skip_header = true` unconditionally — optical images
have no cartridge header.

## Focused tests

The download module keeps its tests next to the code behind `#[cfg(test)] mod`
declarations and uses `wiremock` so no request leaves the machine:

| Test | What it pins |
| --- | --- |
| `src/download_dats/test_redump.rs` | mocks `^/datfile/[A-Z0-9-]+$` with `tests/Test System (20200721).zip`, asserts 1 system / 6 games / 8 ROMs imported and the `-s` copy written |
| `src/download_dats/test_redump_failures.rs` | an empty ZIP, a two-entry ZIP and a 404 each return `Err` and leave the database empty |
| `src/download_dats/test_nointro_failures.rs` | an HTTP error status and an unreachable host on the No-Intro profile each return `Err` |
| `src/download_dats/test_nointro.rs` | mocks `/profile.xml` with `tests/profile.xml` and runs the update check against a DAT-imported system |
| `src/import_dats/test_regions_world.rs`, `test_regions_france_germany.rs` | the normalized region string produced by token parsing |
| `src/sort_roms/test_order_*` | weight ordering for parents, broad/narrow regions, new/old versions and preferred flags, each with the arguments swapped |

Together with the `cfg(test)` catalogue swap, these tests are the reason the whole
provider layer is exercisable without network access — and the reason the catalogue's
production contents are never asserted.

See also [DAT Ingestion](../workflows/dat-ingestion.md),
[ROM Import Pipeline](../workflows/rom-import-pipeline.md), and
[Sorting and the 1G1R Election](../workflows/sorting-and-1g1r-election.md) for the
`sort-roms` election that consumes the regions and version tokens described above.
