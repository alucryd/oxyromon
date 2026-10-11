---
type: testing conventions
title: Test Harness and Conventions
description: How oxyromon's tests are structured and run — module-per-test files wired in with cfg(test), the temp-DB/temp-dir skeleton guarded by the global test MUTEX, prompts that never block under cfg(test), tool-dependent tests that skip rather than fail, and the workspace test and coverage commands.
tags: [testing, harness, mutex, tempfile, cargo-test, coverage, ci]
verified:
  - by: openwiki/0.7.1
    at: 2026-10-09T16:11:37.544Z
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-ea70eb6c045047448e446296
    resource: repo://.gitignore
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-982d918d7483241a1c39d015
    resource: repo://crates/xso-rs/tests/interop.rs
  - id: openwiki-source-8afb0fa0e0b1612b673e51d6
    resource: repo://src/chdman.rs
  - id: openwiki-source-f1234afbb261ce7190a87845
    resource: repo://src/check_roms.rs
  - id: openwiki-source-f026be5ceac01513e05fa46c
    resource: repo://src/check_roms/test_check_by_name.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-e507d9e57674a40c667bf90d
    resource: repo://src/common/test_path_original.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-14bdaf940b9be722639ecfcd
    resource: repo://src/config/test_cli.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-885c8766d237f8340462593b
    resource: repo://src/download_dats/test_nointro.rs
  - id: openwiki-source-dba7f68f124dba00236ec096
    resource: repo://src/download_dats/test_redump_failures.rs
  - id: openwiki-source-009a879733276999b4edfc7b
    resource: repo://src/import_dats/test_dat_updated_orphan_chd.rs
  - id: openwiki-source-a1b9081bbab0058e8553752c
    resource: repo://src/import_dats/test_dat.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-b1a773ad2966952330a5f92d
    resource: repo://src/import_roms/test_cia.rs
  - id: openwiki-source-e39f531dcbca745e3e2e92a4
    resource: repo://src/import_roms/test_original.rs
  - id: openwiki-source-2e0110a5b76b7e42ecbced1f
    resource: repo://src/import_roms/test_prefer_format.rs
  - id: openwiki-source-7fb657fead83b71b9836ddac
    resource: repo://src/info.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-fcdf0f9c4361c1ee384f2689
    resource: repo://src/prompt.rs
  - id: openwiki-source-8ae1d314abb7c3fe6703b5e4
    resource: repo://src/purge_roms/test_orphans.rs
  - id: openwiki-source-b05657110e0e9347d2742939
    resource: repo://src/server/test_queries.rs
  - id: openwiki-source-f134141ea6801addfb247057
    resource: repo://src/server/test_server.rs
  - id: openwiki-source-da31a7c38b2966a8a0185145
    resource: repo://src/server/test_uploads.rs
  - id: openwiki-source-0a9271029eb893aacec54c15
    resource: repo://src/sort_roms.rs
  - id: openwiki-source-b0cb8cd6c9587578828f3269
    resource: repo://src/sort_roms/test_all_regions_from_matches.rs
  - id: openwiki-source-ac4fb8289d5522d8a30dc0b8
    resource: repo://src/sort_roms/test_sort.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-fc509b708992c488b446093a
    resource: repo://src/util/test_compute_alpha_subfolder.rs
  - id: openwiki-source-06dfbbee2ae2d2a513ed6819
    resource: repo://test.sh
  - id: openwiki-source-13df766a80853b510f45e6c3
    resource: repo://tests/crates.rs
  - id: openwiki-source-9a02c1f55f47430403921057
    resource: repo://tests/mcp_stdio.rs
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
---

# Test Harness and Conventions

Almost every test in the repository is a **unit test of the binary crate**, not an
integration test: they live in `src/<module>/test_*.rs`, are pulled into the
parent module with `#[cfg(test)] mod test_name;` at the bottom of that module
([src/sort_roms.rs](../../src/sort_roms.rs#L975-L1015),
[src/import_roms.rs](../../src/import_roms.rs#L2701-L2730)), and reach the code
under test with `use super::*` (and `use super::super::import_dats;` to drive a
neighbouring subcommand). Because they compile *into* the `oxyromon` bin target's
test harness, they can touch private items and call a subcommand's `main()`
directly with a synthesized clap `ArgMatches` — no process is spawned
([src/config/test_cli.rs](../../src/config/test_cli.rs#L5-L35)).

Roughly 300 such modules exist; `src/sort_roms/` alone holds 42, one per sorting
scenario. `tests/` is **not** the ordinary Rust integration directory — it is the
fixture corpus the in-module tests read
([Test Fixtures and End-to-End Suites](test-fixtures.md)). Only two files there
are test targets: the CLI-parity guard `tests/crates.rs` and the end-to-end
`tests/mcp_stdio.rs`, which is the sole case that spawns the real binary via
`env!("CARGO_BIN_EXE_oxyromon")` ([tests/mcp_stdio.rs](../../tests/mcp_stdio.rs#L1-L43)).
A third style exists once, inline: `#[cfg(test)] mod tests { … }` inside
[src/iso.rs](../../src/iso.rs#L186-L192) for the CUE-parsing helpers.

## The canonical skeleton

`AGENTS.md` prescribes the shape, and the modules follow it literally
([AGENTS.md](../../AGENTS.md#L274-L307)):

```rust
#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar).await.unwrap();

    // when — call the function or subcommand under test
    // then — assert database rows *and* filesystem state
}
```

```mermaid
flowchart TD
  L["let _guard = MUTEX.lock"] --> DB["NamedTempFile plus establish_connection runs migrations and PRAGMAs"]
  DB --> R["TempDir::new_in tests becomes ROM_DIRECTORY"]
  R --> T["TempDir::new_in tests becomes TMP_DIRECTORY"]
  T --> DAT["import a DAT through import_dats subcommand main"]
  DAT --> COPY["copy fixtures out of tests into the scratch dir"]
  COPY --> ACT["act: subcommand main or the module function"]
  ACT --> A1["assert database rows"]
  ACT --> A2["assert on-disk layout under rom_directory"]
```

*Caption: the five-step lifecycle every fixture-consuming test repeats — lock, temp DB, temp ROM and TMP directories, DAT import, then act and assert both halves.*

`ProgressBar::hidden()` stands in for the real bar; the production `print_*`
helpers still write to it, so output formatting is exercised without polluting
`cargo test` output ([src/progress.rs](../../src/progress.rs#L26-L41)). The
`// given` / `// when` / `// then` comments are part of the convention. Some
suites factor the preamble into an `async fn setup(dat: &str)` helper — and it
must return the `NamedTempFile` and both `TempDir`s so the caller keeps them
alive; the comment in that file says so explicitly, because a dropped `TempDir`
deletes the directory the pool and the assertions still point at
([src/import_roms/test_prefer_format.rs](../../src/import_roms/test_prefer_format.rs#L9-L42)).

## Isolation: temp database, temp directories, one filesystem

`establish_connection()` is the same entry point `main.rs` uses, so each test
builds a **real schema**: it applies the embedded `MIGRATOR` and sets the whole
`PRAGMA` block (`foreign_keys`, `locking_mode`, WAL, `synchronous`,
`temp_store`, `mmap_size`, `cache_size`, `auto_vacuum`)
([src/database.rs](../../src/database.rs#L11-L72)). That has a consequence agents
miss: the pool geometry is **compile-time feature-dependent**. With `server` or
`mcp` enabled the pool is 5 connections with `NORMAL` locking; otherwise it is
1 connection with `EXCLUSIVE`. The same test source therefore runs against two
different database configurations depending on the feature flags of the command
line — which is precisely why CI and `AGENTS.md` insist on
`--features oxyromon/server` ([CI Pipelines and Runtime Environment](../operations/ci-and-environment.md)).

The scratch directories are created with `TempDir::new_in("tests")`, **not** in
`/tmp`. Two reasons, both load-bearing: fixtures and their imported results stay
on one filesystem, so the rename path in `rename_file()` is exercised rather than
its cross-device copy+delete fallback
([src/util.rs](../../src/util.rs#L123-L151)); and the leftovers are gitignored by
the `tests/.tmp*` entry ([.gitignore](../../.gitignore#L10-L21)). Because an
import *moves* the file, a fixture is never handed to the importer directly — it
is copied into the temp TMP directory first (see
[Test Fixtures](test-fixtures.md#the-corpus-is-read-only-imports-move-files)).

The two setters used to aim the settings at those directories,
`set_rom_directory` / `set_tmp_directory`, exist **only under `cfg(test)`**
alongside the mutex, inside a `cfg_if!` block in `config.rs`
([src/config.rs](../../src/config.rs#L631-L648)). Production code has no
equivalent: it goes through `get_rom_directory()` / `get_tmp_directory()`, which
fall back to `OXYROMON_ROM_DIRECTORY`, `$HOME/Emulation` and `env::temp_dir()` and
write the value back. The test helpers exist so a test can bypass that bootstrap
and point the library at its own directory — and each returns the `PathBuf`, which
is why the idiom is `let tmp_directory = set_tmp_directory(...).await;`. Since the
canonicalized settings live in the per-test database, no environment variables are
set or unset anywhere in the suite.

## The global `MUTEX`

```rust
pub static MUTEX: LazyLock<Mutex<i32>> = LazyLock::new(|| Mutex::new(0));
```

defined in `config.rs` behind `cfg(test)`
([src/config.rs](../../src/config.rs#L631-L648)) and taken as the **first
statement** of each test: `let _guard = MUTEX.lock().await;`. It is a
`tokio::sync::Mutex`, so it serializes across the libtest worker threads inside
one test binary — and since all ~300 `src/` modules compile into the *same*
binary, holding it makes the whole in-crate suite effectively sequential. That is
the price paid for the process-wide state the tests genuinely share: the global
`MULTI_PROGRESS` instance every progress bar registers with
([src/progress.rs](../../src/progress.rs#L17-L34)), the `tests/` directory that
hosts every scratch directory, and the fixed loopback ports the server tests bind.
`AGENTS.md` lists a forgotten `MUTEX` under **Common Pitfalls**: the symptom is
flakiness that only appears under a full `cargo test` run, not when the test is
run alone ([AGENTS.md](../../AGENTS.md#L667-L676)).

The lock is *not* universal, and the exceptions are deliberate: tests with no
process-wide footprint skip it. Pure `#[test]` units — the `mimetype` matchers,
`crc32`, the GraphQL validators, `compute_alpha_subfolder`, `prompt_for_games` —
take nothing ([src/util/test_compute_alpha_subfolder.rs](../../src/util/test_compute_alpha_subfolder.rs#L1-L21)),
and a database-only test like `sort_roms/test_all_regions_from_matches.rs` reads
a setting from its own temp DB without touching the filesystem, so it acquires no
guard ([src/sort_roms/test_all_regions_from_matches.rs](../../src/sort_roms/test_all_regions_from_matches.rs#L1-L23)).
The rule of thumb: if the test writes to `ROM_DIRECTORY`/`TMP_DIRECTORY`, boots a
server, or relies on the settings bootstrap, it locks.
## Prompts must never block

A subcommand that would ask the user something interactively must resolve without
a TTY under test. Two mechanisms carry this:

- **`cfg!(test)` as the "select everything" flag.** `prompt_for_games(games, all)`
  returns the whole list untouched when `all` is true, so the `dialoguer`
  multiselect is never constructed
  ([src/prompt.rs](../../src/prompt.rs#L123-L144)). The call sites pass
  `cfg!(test)` for exactly this reason
  ([src/check_roms.rs](../../src/check_roms.rs#L61-L82),
  [src/common.rs](../../src/common.rs#L36-L55)).
- **Explicit unattended modes.** Import helpers take `UnattendedMode::{None, Skip,
  First}`, and tests pass `Skip` rather than relying on `cfg!(test)`
  ([src/import_roms.rs](../../src/import_roms.rs#L41-L56)).

The `dialoguer` paths themselves are untestable by construction — they read the
terminal — so `src/prompt/test_prompt_for_games_single_match.rs` covers only the
early return (`len() <= 1` answers the question already)
([src/prompt/test_prompt_for_games_single_match.rs](../../src/prompt/test_prompt_for_games_single_match.rs#L1-L30)).
A new prompt call site therefore needs its own bypass, or the suite hangs until CI
times out.

## Tool-dependent tests skip, they do not fail

Anything that shells out to a tool may be absent, so the availability probe is the
first line of the test, before the mutex
([AGENTS.md](../../AGENTS.md#L265-L273)):

```rust
if ctrtool::get_version().await.is_err() {
    return;
}
```
([src/import_roms/test_cia.rs](../../src/import_roms/test_cia.rs#L7-L14))

Version floors are checked the same way, so an installed-but-too-old `chdman`
skips rather than fails: tests comparing against
`chdman::MIN_SPLITBIN_VERSION` (`"0.265"`) return early when the version sorts
lower ([src/chdman.rs](../../src/chdman.rs#L20),
[src/import_dats/test_dat_updated_orphan_chd.rs](../../src/import_dats/test_dat_updated_orphan_chd.rs#L10-L16)).
`get_version()` is the same function `info` and the `dependencies` GraphQL field
report at runtime, so the probe and the product share one definition of
"available" ([src/info.rs](../../src/info.rs#L25-L37)). The format crates follow
the identical rule against `maxcso`, `xdelta3` and `flips`; CI installs only
`mame-tools` (for `chdman`) and `xdelta3`, so the other interop suites skip in CI
by design ([Format Crate Test Strategy](format-crate-tests.md),
[continuous_integration.yml](../../.github/workflows/continuous_integration.yml#L23-L31)).

Note the consequence for green runs: a skipped test is reported as passing. A
tool-gated feature can be silently unexercised — check the `eprintln!`/skip branch
or the `info` output, not the exit code.

## Assert both halves

The convention's other half is that an assertion on rows alone is incomplete. A
subcommand's contract is *database state plus on-disk layout*, and tests assert
the two together:

- `sort_roms/test_sort.rs` checks the four `romfiles` rows **and** that each
  `path` equals the expected relative path under the system directory, then that
  `rom_directory.join(&romfile.path).is_file()`
  ([src/sort_roms/test_sort.rs](../../src/sort_roms/test_sort.rs#L82-L102)).
- `purge_roms/test_orphans.rs` deletes a game row out from under an imported
  romfile, purges, then asserts `find_romfiles()` is empty **and** the system
  directory has zero entries ([src/purge_roms/test_orphans.rs](../../src/purge_roms/test_orphans.rs#L54-L72)).
- `check_roms/test_check_by_name.rs` overwrites the imported file *after* import
  (so the fixture stays intact), runs `check-roms --system …`, and asserts the
  surviving row's path contains `/Trash/` and that the file really exists there
  ([src/check_roms/test_check_by_name.rs](../../src/check_roms/test_check_by_name.rs#L50-L73)).

Comparing the stored path against `system_directory.join(…).strip_prefix(&rom_directory)`
encodes the invariant that **`romfiles.path` is relative to `ROM_DIRECTORY`**; an
implementation that starts storing absolute paths fails here and nowhere else.
Tests that only need "it ran and the catalog agrees" assert the cheaper pair —
`info/test_info.rs` runs the whole dependency-and-statistics report and then checks
`count_systems`/`count_games`/`count_roms`
([src/info/test_info.rs](../../src/info/test_info.rs#L29-L36)).

## Tests that cross an HTTP boundary

Two patterns, neither of which touches the network. DAT downloads take a base URL
as a parameter, so the test mounts a `wiremock` `MockServer` and passes
`mock_server.uri()` into the function under test — dependency injection rather
than an environment variable ([src/download_dats/test_nointro.rs](../../src/download_dats/test_nointro.rs#L35-L51),
[src/download_dats/test_redump_failures.rs](../../src/download_dats/test_redump_failures.rs#L39-L58)).
`wiremock` is the root package's only dev-dependency
([Cargo.toml](../../Cargo.toml#L155-L156)).

The server suites build the same temp DB/directories, then run the *real* Axum
`main(pool, &matches)` in one future and the requests in another, racing them with
`tokio::select!` after a fixed warm-up sleep (100 ms, or 1000 ms for the larger
first case) whose only purpose is to let the listener come up
([src/server/test_queries.rs](../../src/server/test_queries.rs#L48-L117),
[src/server/test_server.rs](../../src/server/test_server.rs#L85-L98)):

```mermaid
sequenceDiagram
  participant T as server test
  participant S as server main with pool
  participant G as HTTP 127.0.0.1 port
  T->>T: MUTEX, temp DB, temp dirs, import DAT
  T->>S: spawn main with subcommand matches and pool
  T->>G: sleep 100 ms then POST graphql
  G-->>T: JSON response asserted field by field
  T->>G: POST dats multipart upload
  G-->>T: 202 Accepted
  T->>G: sleep 500 ms, query systemCount
  G-->>T: background import has landed
  S-->>T: select returns when either future completes
```

*Caption: the timing-based server harness — the server future and the client future race inside one `select!`, and background work is given a sleep before the asserting query.*

Multipart bodies are hand-assembled byte buffers rather than a client helper
([src/server/test_uploads.rs](../../src/server/test_uploads.rs#L12-L46)), and
background imports are awaited by sleeping and then re-querying
([src/server/test_server.rs](../../src/server/test_server.rs#L249-L292)). Each
server test holds the `MUTEX` and pins its own loopback port — the default 8000 or
an explicit `--port` of 8001, 8002, 8003, 8010, 8011 or 8012 — so at most one of
them is live at a time, and no test has to discover a free port. The whole `src/server/` tree is compiled
only with the `server` feature (`#[cfg(feature = "server")] mod server;` in
`main.rs`), so a `cargo test` without features contributes **nothing** from these
files ([src/main.rs](../../src/main.rs#L26-L52)).

## Running the suite

```sh
# everything: oxyromon with the web-server feature, plus the format crates
cargo test --workspace --features oxyromon/server

# one module or one test by name filter
cargo test --features oxyromon/server sort_1g1r

# coverage, as CI runs it
cargo llvm-cov --workspace --features oxyromon/server --lcov --output-path lcov.info

# one format crate, and its slow tests
cargo test -p xso-rs
cargo test --release -p xso-rs -- --ignored
```

The feature has to be written `oxyromon/server` because `--workspace` selects
several packages, and an unqualified `--features server` would name a feature on
whichever package is being built. `server` implies `mcp`, so that single command
also runs `tests/mcp_stdio.rs` ([Cargo.toml](../../Cargo.toml#L36-L51)). Only one
test in the workspace is `#[ignore]`d — the 2 GiB `index_shift_round_trips` in
`crates/xso-rs`, which writes its sparse image under `CARGO_TARGET_TMPDIR`
([crates/xso-rs/tests/interop.rs](../../crates/xso-rs/tests/interop.rs#L183-L207)).

`./test.sh` runs the coverage pass twice
([test.sh](../../test.sh#L1-L4)):

```sh
cargo llvm-cov --workspace --features oxyromon/server --lcov --output-path lcov.info
cargo llvm-cov --workspace --features oxyromon/nod,oxyromon/server,oxyromon/sevenz --lcov --output-path lcov.info
```

The second line names `oxyromon/nod` and `oxyromon/sevenz`, which the root
`[features]` table does not declare — `nod` and `sevenz-rust2` are plain
dependencies now — so that pass selects nothing extra and, writing to the same
`lcov.info`, only overwrites the first if it succeeds at all. CI runs the first
form alone ([continuous_integration.yml](../../.github/workflows/continuous_integration.yml#L60-L64)).

Two build facts matter when a suite feels inexplicably slow. `[profile.dev.package."*"]`
sets `opt-level = 3` for dependencies, because hashing and the pure-Rust codecs run
about an order of magnitude slower at opt-level 0
([Cargo.toml](../../Cargo.toml#L162-L166)); and the whole in-crate suite is
serialized by the `MUTEX`, so wall-clock time scales with the number of
`#[tokio::test]` modules, not with your CPU count.

## Invariants agents break most often

1. **Forgetting `let _guard = MUTEX.lock().await;`** — passes in isolation, fails
   intermittently in a full run and in CI.
2. **Importing a fixture in place** — the importer renames, so the committed file
   under `tests/` disappears
   ([Test Fixtures](test-fixtures.md#the-corpus-is-read-only-imports-move-files)).
3. **`TempDir::new()` instead of `TempDir::new_in("tests")`** — crosses a
   filesystem, exercises the copy+delete fallback instead of the rename, and stops
   being gitignored under `tests/.tmp*`.
4. **Dropping the guards** — a `TempDir`/`NamedTempFile` that isn't bound to a
   living `_name` is deleted at the end of the statement, taking the ROM directory
   or database out from under the assertions.
5. **Asserting only the database** — a move that leaves the wrong path or an empty
   directory behind still looks correct in `romfiles`.
6. **Adding an interactive prompt without a `cfg!(test)` or `UnattendedMode`
   bypass** — hangs the suite.
7. **Failing when a tool is missing** — probe with `get_version()` and `return`
   instead; only `chdman` and `xdelta3` are guaranteed in CI.
8. **Testing `server`/`mcp` code without `--features oxyromon/server`** — the
   modules are not compiled, so the run is vacuously green.
9. **Editing one crate's `ui.rs`** — `tests/crates.rs` requires all five copies to
   be byte-identical ([tests/crates.rs](../../tests/crates.rs#L1-L16)).

## Adding tests for a new subcommand

`AGENTS.md`'s checklist ends with tests: create `src/my_command/test_*.rs`, one
scenario per file, named for the behaviour (`test_sort_1g1r_strict.rs`,
`test_dat_updated_orphan_chd.rs`), and register each with
`#[cfg(test)] mod test_*;` at the bottom of `src/my_command.rs`
([AGENTS.md](../../AGENTS.md#L322-L351)). Copy the skeleton verbatim, import the
system under test through `import_dats::main` rather than by inserting rows —
except for the handful of pure helpers that build a `System`/`Game`/`Rom` struct
in place, which is how `common/test_path_*.rs` pins the path-building logic
([src/common/test_path_original.rs](../../src/common/test_path_original.rs#L5-L18)).

See also [Test Fixtures and End-to-End Suites](test-fixtures.md) for the DAT and
ROM corpus, [Format Crate Test Strategy](format-crate-tests.md) for how
`crates/*` prove byte agreement with their upstreams, and
[Settings and Configuration](../architecture/settings-and-configuration.md) for
what `ROM_DIRECTORY`/`TMP_DIRECTORY` mean outside a test.
