---
type: architecture
title: Settings and Configuration Model
description: The settings key/value table and its category constants in src/config.rs (BOOLEANS, choice_options, choice_list_options, integer_range, LISTS, PATHS, NULLABLES, SORTED_LISTS), the per-system override with global fallback read path, the config subcommand surface, and the GraphQL/MCP surfaces that reuse the same validators.
tags: [config, settings, cli, graphql, mcp, sqlite, format-tuning]
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-2d75c6f9d7c9aa4292af3447
    resource: repo://frontend/src/api.rs
  - id: openwiki-source-09f0260084265b9e66fa1efc
    resource: repo://frontend/src/state.rs
  - id: openwiki-source-8d91829e2ee1609cf3225d53
    resource: repo://migrations/20210323000007_create_settings.sql
  - id: openwiki-source-18a298e115dabea0e367ca81
    resource: repo://migrations/20230528135145_use_pipe_as_separator_in_settings.sql
  - id: openwiki-source-b3c27d89c13e00a083d3dc83
    resource: repo://migrations/20241008133304_add_more_chd_settings.sql
  - id: openwiki-source-e792c1af3374d9105b589924
    resource: repo://migrations/20241227212749_drop_hash_algorithm_setting.sql
  - id: openwiki-source-b5dc1d2e2c0457e339a18ec5
    resource: repo://migrations/20260317000001_add_system_id_to_settings.sql
  - id: openwiki-source-76521838de1f93f55c8cfb0e
    resource: repo://migrations/20260808000001_rename_rvz_bzip_compression.sql
  - id: openwiki-source-96c83262fa92895f13ea94e8
    resource: repo://migrations/20260810000001_add_archive_compression_algorithms.sql
  - id: openwiki-source-543688cbca9cffab391f2283
    resource: repo://migrations/20260811000001_add_prefer_format_setting.sql
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-02628a361dd4faa0f62f87a2
    resource: repo://src/archive.rs
  - id: openwiki-source-969423144217bcca78dffcb0
    resource: repo://src/benchmark.rs
  - id: openwiki-source-8afb0fa0e0b1612b673e51d6
    resource: repo://src/chdman.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-14bdaf940b9be722639ecfcd
    resource: repo://src/config/test_cli.rs
  - id: openwiki-source-49be2dfb7139ef7701e1c1f3
    resource: repo://src/config/test_set_new_directory_when_old_is_missing.rs
  - id: openwiki-source-8b8a5618fcb1f7cdea3de7ac
    resource: repo://src/config/test_system_bool_fallback.rs
  - id: openwiki-source-b130a948cf6dedfa0b9c4edd
    resource: repo://src/config/test_system_bool_override.rs
  - id: openwiki-source-10fddfccad5d7bf89d675953
    resource: repo://src/convert_roms.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-bfaccf08e532321769e4b6b5
    resource: repo://src/export_roms.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-72ac7329667332f8c07f2f9c
    resource: repo://src/import_roms/test_per_system_rom_directory.rs
  - id: openwiki-source-2e0110a5b76b7e42ecbced1f
    resource: repo://src/import_roms/test_prefer_format.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-389985405c3e5c30c5b6cede
    resource: repo://src/mcp/output.rs
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
  - id: openwiki-source-db13c01516e9bd04294e339d
    resource: repo://src/rvz.rs
  - id: openwiki-source-dfa02c19f5d8e4215d36f9c0
    resource: repo://src/server.rs
  - id: openwiki-source-dbad39b6e268c1f314baf29d
    resource: repo://src/server/test_mutations.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-17ea50e0748f5cbf91688f22
    resource: repo://src/validator.rs
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
verified:
  - by: openwiki/0.7.1
    at: 2026-10-10T22:10:44.671Z
---

# Settings and Configuration Model

oxyromon has no configuration file. Every user preference is one row of the
`settings` table in `oxyromon.db`, and `src/config.rs` is the single place that
knows what each key means: which category it belongs to, what values are legal,
and how the stored text becomes a Rust type. Format code, the `config`
subcommand, the GraphQL schema and the MCP tools all go through this module
rather than re-implementing their own notion of a setting.

The row itself is deliberately dumb (`src/model.rs`): `id`, `key`,
`value: Option<String>`, `system_id: Option<i64>`. Values are always text or
`NULL`; `system_id` is the scope. Type conversion, validation and defaults are
all the caller-side contract in `config.rs` plus the seeded rows in
`migrations/`.

## Storage, scoping and the two partial indexes

`migrations/20260317000001_add_system_id_to_settings.sql` added the scope column
by rebuilding the table through `settings_new` (SQLite cannot alter a uniqueness
constraint) and installing two partial unique indexes:

```sql
CREATE UNIQUE INDEX settings_key_global_unique ON settings(key) WHERE system_id IS NULL;
CREATE UNIQUE INDEX settings_key_system_unique ON settings(key, system_id) WHERE system_id IS NOT NULL;
```

So at most one global row exists per key, and at most one row per (key, system).
The migration copies the pre-existing rows in with `system_id` left `NULL`,
meaning an upgraded database behaves exactly as before until someone scopes a
key. Because `settings.system_id` is `REFERENCES systems(id) ON DELETE CASCADE`,
purging a system takes its overrides with it — see
[Data Model, SQLite Usage and Migrations](data-model-and-migrations.md).

Reads resolve scope with `IS ?` binding (`src/database.rs`), which treats a
bound `NULL` as "the global rows" instead of never matching:

```rust
pub async fn get_setting(connection, key, system_id) -> Option<Setting> {
    if let Some(id) = system_id
        && let Some(setting) = find_setting_by_key(connection, key, Some(id)).await
    { return Some(setting); }
    find_setting_by_key(connection, key, None).await
}
```

```mermaid
flowchart TD
  A["caller passes key plus optional system_id"] --> B{"system_id present"}
  B -->|"no"| D["find_setting_by_key with NULL, the global row"]
  B -->|"yes"| C{"per-system row exists"}
  C -->|"yes"| E["return the override row"]
  C -->|"no"| D
  D --> F["typed accessor parses the text value"]
  E --> F
  E -.->|"value NULL"| G["shadowed: no value at all, no second fallback"]
```

*Caption: `get_setting` resolves the override first and falls back to the global row; a per-system row that exists with a NULL value shadows the global instead of inheriting it.*

Writes funnel through one helper, `upsert_setting`: `find_setting_by_key` on the
exact scope, `update_setting(connection, id, value)` when the scoped row exists,
`create_setting(connection, key, value, system_id)` otherwise
(`src/database.rs`). No caller inserts directly, so the partial indexes never see
a duplicate.

Note the asymmetry the fallback creates. `unset_setting` writes `NULL` into the
row *for the requested scope* (`find_setting_by_key(connection, key, system_id)`),
so a per-system unset of a nullable key leaves a NULL row that `get_setting`
returns for that system — the global value is not consulted again, and a
`get_string` caller sees `None` while a `get_bool` caller would panic on the
missing value. Overrides win on read; clearing them is not the same as deleting
them.

`list_settings` and the GraphQL/MCP "effective settings" reads implement the
same precedence as a merge instead of a lookup: build a map keyed by `key` from
the global rows, overwrite with `find_settings(connection, Some(id))`, sort by
key. That is why `config --list -n <system>` prints the whole configuration as
that system sees it, not just its overrides.

## Typed accessors and their failure semantics

| Accessor | Missing row | NULL value | Written by |
| --- | --- | --- | --- |
| `get_bool` | panics (`.unwrap()`) | panics | `set_bool` |
| `get_integer` | panics | `None` | `set_integer` |
| `get_string` | panics | `None` | `set_string` |
| `get_list` | `vec![]` | `vec![]` | `add_to_list` / `remove_from_list` / `set_list` |
| `get_directory` | `None` | panics | `set_directory` |

`get_directory` also returns `None` when the stored path no longer resolves,
because it canonicalizes on read (`get_canonicalized_path`); `set_directory`
stores the string as given. `src/config/test_set_new_directory_when_old_is_missing.rs`
pins exactly that recovery path: after the stored directory is deleted, setting
a new one yields a readable value again. `get_list` splits on
`LIST_SEPARATOR = "|"`, and `set_list` stores an empty list as `NULL` rather than
an empty string; a `|` separator migration
(`20230528135145_use_pipe_as_separator_in_settings.sql`) rewrote the earlier `,`
values.

The panics are load-bearing rather than sloppy: a key that has a global row can
never be missing, because the default is seeded by a migration, so `get_bool` /
`get_integer` / `get_string` unwrapping the `Setting` is a "this key was added
without its default row" bug detector. Two consequences follow. `benchmark`
bypasses the accessors and reads `ROM_DIRECTORY` / `TMP_DIRECTORY` with
`find_setting_by_key(...).unwrap().value.unwrap()` because it wants the raw
text, and `config --get <unknown key>` panics for the same reason
(`print_setting` unwraps).

Consumers always pass a scope when one exists. `sort_roms::main` reads the whole
1G1R vocabulary per system (`PREFER_PARENTS`, `PREFER_REGIONS`,
`PREFER_VERSIONS`, `PREFER_FLAGS`, `REGIONS_ONE_STRICT`, the region/language/
discard lists) with `system_id = Some(system.id)`, and `convert_roms` /
`export_roms` do the same for the format-tuning keys. See
[Sorting and the 1G1R Election](../workflows/sorting-and-1g1r-election.md).

## Category constants: how a key is classified

`config.rs` classifies keys by membership tests, not by per-key metadata:

| Category | Form | Effect |
| --- | --- | --- |
| `BOOLEANS` | const slice | `--set` parses a bool; stored `"true"`/`"false"` |
| `choice_options(key)` | fn returning `Option<&[&str]>` | `--set` accepts only a listed variant |
| `choice_list_options(key)` | private fn returning `Option<&[&str]>` | `--add` validates each entry |
| `integer_range(key)` | fn returning `Option<&[usize; 2]>` | `--set` parses and range-checks |
| `LISTS` | const slice | `--add`/`--remove` only; `--set` refused |
| `PATHS` | const slice | canonicalize, create, store |
| `NULLABLES` | const slice | only these keys accept `--unset` |
| `SORTED_LISTS` | const slice | list order is *preserved*; others are re-sorted on add |

`choice_options` and `choice_list_options` are functions precisely because their
option sets are not local to config: `PREFER_FORMAT` borrows
`convert_roms::ALL_FORMATS`, `RVZ_/SEVENZIP_/ZIP_COMPRESSION_ALGORITHM` borrow
`VariantNames::VARIANTS` from `rvz.rs`, `archive.rs` and `chdman.rs`, and the
`REGIONS_*_SUBFOLDERS` / `PREFER_REGIONS` / `PREFER_VERSIONS` choices come from
the `SubfolderScheme`, `PreferredRegion` and `PreferredVersion` strum enums
declared in `config.rs` itself. Value casing follows those enums
(`serialize_all = "lowercase"`, so `none`/`broad`/`narrow`), except
`PREFER_FORMAT`, whose choices are the uppercase format names `ORIGINAL`, `7Z`,
`CHD`, `CSO`, `NSZ`, `RVZ`, `ZIP`, `ZSO`. `config.rs` also still exports
`HashAlgorithm`, which is no longer a setting — its row was deleted by
`20241227212749_drop_hash_algorithm_setting.sql` — but remains the hash-selection
enum threaded through `get_hash_and_size` by import and DAT creation.

`SORTED_LISTS` is easy to misread. `add_to_list` sorts the list after inserting
unless the key is in `SORTED_LISTS`, and that slice names exactly three keys:
`CHD_CD_COMPRESSION_ALGORITHMS`, `CHD_DVD_COMPRESSION_ALGORITHMS` and
`REGIONS_ONE`. Membership means "this list's order is semantic" — `REGIONS_ONE`
is a region *priority* order, and the CHD CD/DVD algorithm lists are the trial
order `chdman` receives via `--compression`. The newer CHD keys do not share
that treatment, and the asymmetry is worth knowing before you touch either
category: `CHD_HD_COMPRESSION_ALGORITHMS` and `CHD_LD_COMPRESSION_ALGORITHMS`
are validated choice lists but are absent from `SORTED_LISTS`, so `--add`
alphabetizes them and a preferred chdman trial order cannot be expressed for HD
or LD media. Likewise `NULLABLES` covers the CD and DVD CHD keys only: all four
`CHD_HD_*` / `CHD_LD_*` keys were seeded as `NULL` by
`20241008133304_add_more_chd_settings.sql` yet cannot be unset — `unset_setting`
refuses them and the MCP tool rejects them with the `NULLABLES` list — even
though their CD and DVD counterparts can. Emptying an HD/LD algorithm list entry
by entry still reaches `NULL`, because `set_list` stores an empty list that way,
but an HD or LD hunk size has no in-app path back to "let chdman choose".

Category membership also determines which mutators accept a key, and the paths do
not overlap uniformly. `set_setting` dispatches in the order `PATHS` →
`BOOLEANS` → `choice_options` → `integer_range` → `LISTS` → unknown-key error, so
a choice-list key (`CHD_CD_COMPRESSION_ALGORITHMS`, `REGIONS_ONE_ARCADE`) is
reported as `Unknown setting` by `--set` and can only be edited with `--add`.
Conversely `--set` refuses every `LISTS` key with "Lists can't be set directly".

## The `config` subcommand

`config::subcommand()` registers one `-n/--system <NAME>` flag (percent globs,
resolved through `find_systems_by_name_like`) and six mutually exclusive
operations — `--list`, `--get KEY`, `--set KEY VALUE`, `--unset KEY`,
`--add KEY VALUE`, `--remove KEY VALUE` — each declared `conflicts_with_all` of
the others, so an ambiguous invocation is rejected by clap instead of silently
ordering.

`main` then decides how many scopes the operation touches:

```mermaid
flowchart TD
  A["config with optional -n NAME"] --> B{"SET, UNSET, ADD or REMOVE present"}
  B -->|"yes, editing"| C["prompt_for_systems_like, multi-select on ambiguous glob"]
  C --> D{"empty result"}
  D -->|"yes"| E["warn No matching system found, return Ok"]
  D -->|"no"| F["loop the operation over every matched system_id"]
  B -->|"no, reading"| G["prompt_for_system_like, single system_id"]
  G --> F
  F --> H["operation applied with that scope, NULL when no -n"]
```

*Caption: one glob can fan out to many systems for edits, while reads always collapse to a single system.*

Validation results inside `set_setting` are reported, not returned as errors: an
illegal choice prints `Valid choices: [...]`, an out-of-range integer prints
`Valid range: [...]`, and an unknown key prints an error line — yet `main` still
returns `Ok(())` and the previous value stays untouched
(`src/config/test_cli.rs` asserts each of these non-destructive rejections).
The one path that can fail the process is `PATHS`: `get_canonicalized_path`
errors on a path that does not exist, so `--set ROM_DIRECTORY /nope` returns an
`Err` from `set_setting` before `create_directory` runs.

`unset_setting` prints `Setting "..." cannot be unset` for anything outside
`NULLABLES`; `add_to_list` rejects a duplicate with a skip, validates choice-list
entries against their `VARIANTS`, and refuses non-list keys; `remove_from_list`
warns when the value is absent rather than erroring.

## Defaults live in migrations, not in code

There is no default table in Rust. Adding a key's default means a new migration
using the repository's idempotent insert style:

```sql
INSERT OR REPLACE INTO settings ("key", value) VALUES ('SEVENZIP_COMPRESSION_ALGORITHM', 'lzma2');
```

`migrations/20210323000007_create_settings.sql` created the table and its first
rows; later migrations added the compression, CHD, arcade and `PREFER_FORMAT`
defaults, some seeded as `NULL` on purpose ("auto"). The same statement style is
used for retcons: `20230103125145_add_compression_settings.sql` lowercased the
subfolder-scheme values that earlier migrations had seeded as `"NONE"`, and
`20260808000001_rename_rvz_bzip_compression.sql` rewrote the RVZ algorithm value
`bzip` → `bzip2` because dolphin-tool would otherwise refuse to parse it. Two
consequences for readers: a "default" the README quotes can be stale in an old
database (`INSERT OR REPLACE` only touches the row at migration time), and the
only in-code fallbacks are the directory bootstrap and `archive.rs`'s
`DEFAULT_COMPRESSION_LEVEL` / `DEFAULT_ZSTD_COMPRESSION_LEVEL` used when a level
row is `NULL`.

The directory keys are the exception to migration seeding. `main.rs` calls
`get_rom_directory` and `get_tmp_directory` at startup purely for the side effect
of persisting the first-run value (`OXYROMON_ROM_DIRECTORY`, else
`~/Emulation`; `OXYROMON_TMP_DIRECTORY`, else `env::temp_dir()`). Both are
`PATHS` keys read globally (`system_id = None`) by the bootstrap, while
`util::get_system_directory` and `common::get_relative_path` read them with the
system's scope and fall back to the global helper, so a single system can be
relocated to another volume — `src/import_roms/test_per_system_rom_directory.rs`
sets two per-system `ROM_DIRECTORY` values and asserts each system's files land
under its own root. `server.rs`'s download handler reads the *global*
`ROM_DIRECTORY` directly and returns 500 when the row is absent.

## Format tuning keys: declared here, ranges owned elsewhere

Every `CHD_*`, `RVZ_*`, `SEVENZIP_*` and `ZIP_*` key is declared in `config.rs` —
in `choice_options` / `choice_list_options` and, for the integers, in
`integer_range` — but no range or algorithm vocabulary is defined there. The
`integer_range` arms point at constants owned by the modules that do the work:

| Key | Range owner |
| --- | --- |
| `CHD_CD/DVD/HD/LD_HUNK_SIZE` | `chdman::CHD_HUNK_SIZE_RANGE` = 16..1048576 |
| `RVZ_BLOCK_SIZE` | `rvz::RVZ_BLOCK_SIZE_RANGE` = 32..2048 (KiB) |
| `RVZ_COMPRESSION_LEVEL` | `rvz::RVZ_COMPRESSION_LEVEL_RANGE` = 1..22 |
| `SEVENZIP_COMPRESSION_LEVEL`, `ZIP_COMPRESSION_LEVEL` | `archive.rs` `_RANGE` = 1..9 |
| `SEVENZIP_ZSTD_COMPRESSION_LEVEL`, `ZIP_ZSTD_COMPRESSION_LEVEL` | `archive::ZSTD_COMPRESSION_LEVEL_RANGE` = 1..22 |

Likewise the algorithm choices are the `VARIANTS` of `ChdCd/Dvd/Hd/LdCompressionAlgorithm`
(`chdman.rs`), `RvzCompressionAlgorithm` (`rvz.rs`), and
`SevenzipCompressionAlgorithm` / `ZipCompressionAlgorithm` (`archive.rs`).
Validation therefore cannot drift from what the writers accept, and a format
module changing its range changes what `config --set` will accept.

Reading happens per system at conversion time: `convert_roms` pulls all four CHD
hunk sizes and algorithm lists for `Some(system.id)`, while `export_roms` reads
only the CD and DVD pairs — the HD and LD tuning keys therefore affect
`convert-roms` alone. `archive::get_archive_compression` maps an archive type onto its
(algorithm, level, zstd-level) key triple and produces an `ArchiveCompression`
that carries the level on the chosen algorithm's own scale. The consumers apply
the units and per-algorithm limits the setting layer deliberately does not:
`rvz.rs` multiplies `RVZ_BLOCK_SIZE` by 1024 because nod wants bytes, clamps the
level with `level.clamp(1, 9)` for bzip2/lzma/lzma2 and `clamp(1, 22)` for zstd
(the `integer_range` entry is the union 1..22), and prints a warning that
`RVZ_SCRUB is unsupported by nod, writing unscrubbed` rather than failing.
`chdman` omits `--hunksize` for a `None` hunk size and joins the algorithm list
with `,` into `--compression`, leaving chdman's own auto-selection in place when
the list is empty — which is why an unset (NULL) tuning key is a meaningful
value, not an absent setting.

`PREFER_FORMAT` is also written from outside `config`. `convert-roms --save`
persists the format it just converted to through `set_string` directly, and
`import-roms` auto-converts the games it imported whenever the setting is set.
The arcade restriction — `convert_roms::format_supported_for_system` allows only
`ORIGINAL`/`ZIP` — is enforced by the three validated entry points
(`config::set_setting`, `mutation::set_prefer_format`, and the MCP tool), each of
which refuses and leaves the stored value untouched;
`src/import_roms/test_prefer_format.rs` asserts that
`set_setting(.. "PREFER_FORMAT", "CHD", Some(arcade))` yields `None` while `ZIP`
sticks. `convert-roms --save` skips that check, so a per-system `PREFER_FORMAT`
that no validated surface would accept is reachable and will be honored by the
next import's auto-conversion pass.
See [Format Conversion and Export](../workflows/format-conversion-and-export.md).

## The non-CLI surfaces

Three other entry points read and write the same rows, and the interesting
question for any change is whether they reuse config's classification:

- **GraphQL** (`query.rs`, `mutation.rs`, behind the `server` feature).
  `settings` returns global rows only; `systemSettings(systemId)` returns the
  merged effective view the frontend needs. Mutations are thin wrappers over the
  same config functions — `addToList`, `removeFromList`, `setBool`,
  `setDirectory`, `setSubfolderScheme`, `setPreferRegions`, `setPreferVersions`,
  `setPreferFormat`, `unsetPreferFormat`. Choice validation moves into
  `src/validator.rs`: `VariantValidator<E>` checks a string against
  `E::VARIANTS` (the very slices `choice_options` exposes),
  `PreferFormatValidator` checks `ALL_FORMATS`, and `DirectoryValidator` requires
  an existing directory — so a bad value becomes a GraphQL error rather than the
  CLI's printed warning. `setPreferFormat` repeats the arcade check as an error.
  The generic-key mutations (`setDirectory`, `setSubfolderScheme`, `setBool`)
  take any key string, so they do not consult `PATHS` / `BOOLEANS` at all.
- **MCP** (`src/mcp/read.rs`, `src/mcp/write.rs`, behind the `mcp` feature).
  `get_settings` merges globals with the requested system's overrides and tags
  each output entry with the `system_id` it came from. `set_setting` imports
  `BOOLEANS`, `LISTS`, `NULLABLES`, `PATHS`, `choice_options` and `integer_range`
  and re-runs the config validation as actionable tool errors, with one deliberate
  divergence: a list value is the *complete desired set*, comma-separated, and the
  tool diffs it against `get_list` and issues `remove_from_list` / `add_to_list`
  per entry instead of a wholesale write.
- **Web UI** (`frontend/`). `get_raw_settings(system_id)` chooses
  `{ settings { key value } }` or `{ systemSettings(systemId: …) }`, and
  `split_list` splits the stored value on `|` — a second implementation of
  `LIST_SEPARATOR` that a separator change must keep in step.

## Adding a setting

The repository's own checklist (`AGENTS.md`) and the shape of `config.rs` agree
on the procedure; the only wrinkle is that three of the eight categories it
names — `CHOICES`, `CHOICE_LISTS` and `INTEGERS` — do not exist as constants at
all; they are the `choice_options`, `choice_list_options` and `integer_range`
functions.

```mermaid
flowchart TD
  A["pick a key name and add it to the right category in config.rs"] --> B{"value shape"}
  B -->|"boolean"| C["append to BOOLEANS"]
  B -->|"single choice"| D["add a strum enum with VariantNames and an arm in choice_options"]
  B -->|"multi choice"| E["add an arm in choice_list_options, plus NULLABLES and maybe SORTED_LISTS"]
  B -->|"integer"| F["add an arm in integer_range pointing at the owning module's RANGE constant"]
  B -->|"free-form list"| G["append to LISTS"]
  B -->|"directory"| H["append to PATHS"]
  C --> I["add a migration with INSERT OR REPLACE for the default, NULL where auto is correct"]
  D --> I
  E --> I
  F --> I
  G --> I
  H --> I
  I --> J["read it with get_bool, get_integer, get_list, get_string or get_directory, passing the system scope"]
  J --> K["if the web UI or MCP needs it, expose it in query.rs and mutation.rs or mcp write.rs"]
  K --> L["cargo sqlx prepare then cargo build with the server feature"]
```

*Caption: the path from a new key's category to a consumed, exposed setting.*

Concretely:

1. **Category.** Append the key to `BOOLEANS`, `LISTS`, `PATHS`, `NULLABLES` or
   `SORTED_LISTS` as needed, or add a `match` arm to `choice_options`,
   `choice_list_options`, or `integer_range`. If a new choice enum is needed,
   derive `EnumString` + `VariantNames` with `serialize_all = "lowercase"` so the
   stored value matches what `--set` and `VariantValidator` accept.
2. **Default.** Add a `migrations/<timestamp>_add_<name>_setting.sql` with
   `INSERT OR REPLACE INTO settings ("key", value) VALUES (…)`. Omitting this
   step is how a `get_bool` unwrap later panics. `cargo sqlx prepare --
   --features server` afterwards, per the migration workflow.
3. **Consume.** Read it with the typed accessor, threading the `system_id` you
   already have so the override/fallback path is exercised; consumers pass
   `Some(system.id)` even for keys that are conceptually global.
4. **Expose.** Add a query in `query.rs` and a mutation in `mutation.rs` (with a
   `validator(custom = ...)` for choices) if the web UI needs it; the MCP
   `set_setting` tool picks the key up automatically once the category functions
   know it, and `get_settings` lists it as soon as the row exists.
5. **Retcon existing values** with an `UPDATE settings …` migration if you are
   renaming a stored value, following the `bzip` → `bzip2` precedent.

## Tests worth knowing

`src/config/` holds one test per behaviour rather than a suite, each building a
throwaway database with `establish_connection` on a `NamedTempFile`:

- `test_system_bool_fallback.rs` — a global `PREFER_PARENTS = false` is what a
  system with no override reads back.
- `test_system_bool_override.rs` — with both scopes set, `Some(system.id)` reads
  the override and `None` still reads the global; the two rows coexist.
- `test_cli.rs` — drives the subcommand end to end through
  `subcommand().get_matches_from([...])`: valid choice, invalid choice leaving
  the old value, direct list `--set` refused, `--add`, invalid choice-list entry,
  `--remove`, `--unset` of a nullable list, `--unset` of a non-nullable bool,
  `--get`/`--list`, and an unknown key returning `Ok`.
- `test_add_to_list.rs`, `test_add_to_list_already_exists.rs`,
  `test_remove_from_list.rs`, `test_remove_from_list_does_not_exist.rs` — list
  add/remove and their no-op cases.
- `test_directory.rs` and `test_set_new_directory_when_old_is_missing.rs` — the
  canonicalize-on-read contract and recovery from a vanished directory.
- `test_directory` siblings outside the module (`src/validator/test_*`,
  `src/server/test_mutations.rs`, `src/server/test_queries.rs`) cover the
  GraphQL-side validators and the merged `systemSettings` query.

Tests that touch the global `ROM_DIRECTORY` take `config::MUTEX` (a
`cfg_if`-gated test-only lock) and point the settings at `TempDir`s, because the
directory bootstrap writes to a process-wide location otherwise.
