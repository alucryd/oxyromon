---
type: quickstart
title: Quickstart & Task Routing
description: Entry point for the oxyromon wiki — what the project is (a Rust CLI ROM organizer whose optional server feature adds a GraphQL API, an embedded web UI and an MCP server), the shortest build-and-run loop, where the database and ROM directory live, the repo split, and a map from a task to the right wiki page.
tags: [quickstart, navigation, routing, cli, workspace, invariants]
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-36f054f1f38f2197a6af5bca
    resource: repo://build.rs
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-92ee3a60a9826201e8ce6fa7
    resource: repo://desktop/Cargo.toml
  - id: openwiki-source-182eb6366058c61b0d4d15e9
    resource: repo://frontend/Cargo.toml
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-8afb0fa0e0b1612b673e51d6
    resource: repo://src/chdman.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-63f87054c5f9055be025dd77
    resource: repo://src/ctrtool.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-7fb657fead83b71b9836ddac
    resource: repo://src/info.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-0a9271029eb893aacec54c15
    resource: repo://src/sort_roms.rs
  - id: openwiki-source-06dfbbee2ae2d2a513ed6819
    resource: repo://test.sh
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
---

# Quickstart & Task Routing

oxyromon is a cross-platform, opinionated CLI ROM organizer written in Rust: it validates ROM
files against Logiqx DAT catalogs (No-Intro, Redump, MAME/FBNeo), imports them into a managed
directory tree, sorts them by region and 1G1R preference, and converts or exports them between
formats. Internally it sticks to original and lossless formats; lossy output happens only on
export. The same binary can also run as a web server (GraphQL API plus an embedded Leptos SPA)
and as a Model Context Protocol server — both optional Cargo features, and both thin wrappers
over the very same subcommand implementations the CLI runs.

## Shortest working path

```sh
cargo build --release            # CLI only, no special requirements
cargo build --release --features server   # plus GraphQL + web UI (needs trunk + wasm32 target)
```

Then the core loop, in order:

```sh
target/release/oxyromon info                                     # dependencies + library counts
target/release/oxyromon config -s ROM_DIRECTORY /path/to/Emulation
target/release/oxyromon config -s REGIONS_ALL US,EU,JP
target/release/oxyromon config -s REGIONS_ONE US,EU
target/release/oxyromon import-dats "Nintendo - Game Boy (20260101).dat"
target/release/oxyromon import-roms ./incoming
target/release/oxyromon sort-roms --system "Nintendo - Game Boy"
```

A system must exist in the database before ROMs can match anything, and `sort-roms` needs at
least one region in `REGIONS_ALL` or `REGIONS_ONE`. ROMs that check out land in the system's base
directory; sorting then distributes files across the system directory, `1G1R`, and `Trash`.

```mermaid
flowchart LR
  DAT["Logiqx DAT file"] -->|"import-dats"| DB[("oxyromon.db")]
  ROM["ROM files"] -->|"import-roms: hash, match, place"| DB
  ROM --> DISK[("ROM_DIRECTORY")]
  CFG["config settings: REGIONS_ALL, REGIONS_ONE"] --> DB
  DB -->|"sort-roms"| OUT[("system dir, 1G1R, Trash")]
```

*Caption: the DAT → import → sort loop, and the two stores it writes — the SQLite catalog and the ROM directory on disk.*

## Where things live

- **Database**: `<data_dir>/oxyromon/oxyromon.db`, created on first run together with its parent
  directory, with embedded migrations applied automatically. Override the directory with
  `OXYROMON_DATA_DIRECTORY` — note `README.md` still says `OXYROMON_DATA_DIR`, which is not what
  the code reads.
- **ROMs**: the `ROM_DIRECTORY` setting, defaulting to `~/Emulation`. `TMP_DIRECTORY` defaults to
  the system temp directory and should have several GB free for DVD-sized extractions. Both are
  ordinary settings rows: on first use each is seeded from `OXYROMON_ROM_DIRECTORY` /
  `OXYROMON_TMP_DIRECTORY` if set, otherwise from the platform default, and then **persisted**, so
  later edits belong in the database, not the environment.
- **Configuration** lives in the database, not a config file. `oxyromon config` lists, gets, sets,
  unsets and edits list entries; `-n/--system` scopes an operation to one system, whose value wins
  over the global one with automatic fallback.

## Repository split

| Path        | What it is                                                                                     |
| ----------- | ---------------------------------------------------------------------------------------------- |
| root        | The `oxyromon` package: the binary, `build.rs`, `migrations/`, `.sqlx/` offline query data.     |
| `crates/`   | Five format crates (`gdi-rs`, `nsz-rs`, `xdelta-rs`, `xps-rs`, `xso-rs`), workspace members, standalone library + CLI, published independently. |
| `frontend/` | The Leptos SPA. **Not** a workspace member — it declares its own `[workspace]`.                 |
| `desktop/`  | The Tauri shell that spawns `oxyromon server` as a sidecar. Also standalone.                    |
| `tests/`    | DAT and ROM fixtures plus the end-to-end suites (MCP stdio, format-crate CLI parity).          |

`frontend/` and `desktop/` keep their own workspace roots so WebAssembly and webview
dependencies never enter the native build; `build.rs` runs `trunk build --release` in `frontend/`
when the `server` feature is on and embeds the result from `target/assets/` (skip with
`SKIP_TRUNK=true`).

Only two dependencies are still shelled out to — `chdman` (CHD) and `ctrtool` (CIA). Everything
else is built in, and `oxyromon info` reports which ones it found.

## Task routing

| If the task touches…                                                                                     | Go to                                                                          |
| -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| how a command is wired, startup order, the shared subcommand contract                                     | [CLI Entrypoint and the Subcommand Contract](architecture/cli-entrypoint-and-subcommands.md) |
| the layered design and where GraphQL/MCP enter                                                            | [System Architecture Overview](architecture/overview.md)                       |
| schema, row types, migrations, `cargo sqlx prepare`                                                       | [Data Model, SQLite Usage and Migrations](architecture/data-model-and-migrations.md) |
| `mcp` / `server` / TLS features, `build.rs`, Trunk, `SKIP_TRUNK`                                          | [Feature Flags and Build Topology](architecture/feature-flags-and-build-topology.md) |
| setting keys, categories, global vs per-system scoping                                                    | [Settings and Configuration Model](architecture/settings-and-configuration.md) |
| progress bars, `print_*` output, interactive prompts                                                      | [Progress Bars, Categorized Output and Prompts](concepts/cli-output-progress-and-prompts.md) |
| extension constants, magic-byte matchers                                                                  | [File Type Detection and Extensions](concepts/file-type-detection-and-extensions.md) |
| adding or changing a format module (`archive`, `rvz`, `xso`, `chdman`, …)                                 | [Format Modules and Their Crate Backends](concepts/format-modules-and-format-crates.md) |
| `CommonRomfile` and its trait system                                                                      | [The CommonRomfile Trait System](concepts/romfile-trait-system.md)             |
| DAT parsing, header definitions, Redump/No-Intro downloads                                                | [Catalog Ingestion: DATs, Headers and Downloads](workflows/dat-ingestion.md)   |
| hashing, matching, placement, `PREFER_FORMAT` on import                                                   | [ROM Import Pipeline](workflows/rom-import-pipeline.md)                        |
<!-- openwiki: broken internal link [workflows/sorting-and-1g1r-election.md] file "workflows/sorting-and-1g1r-election.md" does not exist. Fix the href or restore the target, then delete this comment. -->
| regions, 1G1R election, subfolder schemes, `Trash`                                                        | [Sorting and the 1G1R Election](workflows/sorting-and-1g1r-election.md)        |
| `convert-roms` / `export-roms` and the conversion matrix                                                  | [Format Conversion and Export](workflows/format-conversion-and-export.md)      |
| `check-roms`, the purge family, empty-directory cleanup                                                   | [Integrity Checking, Purging and Trash](workflows/integrity-check-and-purge.md) |
| arcade merging strategies and M3U playlists                                                               | [Arcade Rebuilds and M3U Playlists](workflows/arcade-rebuild-and-playlists.md) |
| PS3 IRD files and JB folders                                                                              | [PS3 IRD Files and JB Folders](workflows/ird-ps3-jbfolders.md)                 |
| routes, `AppState`, DataLoader, uploads, shutdown                                                         | [HTTP Server, GraphQL API and Uploads](server/http-server-and-graphql-api.md)  |
| SSE events and how a progress bar becomes a stream                                                        | [SSE Events and Progress Bridging](server/sse-and-progress-bridge.md)          |
| Leptos components, Web Awesome, the CSS cascade                                                           | [Web UI (Leptos SPA + Web Awesome)](server/web-ui-frontend.md)                 |
| MCP tools, resources, prompts, `run_action`, read-only mode                                               | [MCP Server: Tools, Resources and Prompts](server/mcp-server.md)               |
| the Tauri sidecar, port reservation, child cleanup                                                        | [Desktop App (Tauri Sidecar)](server/desktop-tauri-shell.md)                   |
| DAT provider URLs and No-Intro/TOSEC name tokens                                                          | [DAT Providers and Naming Conventions](integrations/dat-providers-no-intro-redump.md) |
| chdman/ctrtool lookup, version probing, failure handling                                                  | [External Tool Integration](integrations/external-tools.md)                    |
| the `crates/` format crates and their shared CLI shape                                                    | [Format Crates in crates/](integrations/format-crates.md)                      |
| helper scripts, cross-compilation, containers, per-crate publishing                                       | [Builds, Distribution and Releases](operations/build-and-release.md)           |
| CI jobs, `OXYROMON_*` variables, `DATABASE_URL`, TLS/Windows build notes                                  | [CI Pipelines and Runtime Environment](operations/ci-and-environment.md)       |
| writing or running tests, the `MUTEX`, temp DB/dirs                                                       | [Test Harness and Conventions](testing/testing-conventions.md)                 |
| fixture corpus, MCP stdio and CLI-parity end-to-end suites                                                | [Test Fixtures and End-to-End Suites](testing/test-fixtures.md)                |
| known-answer vectors and upstream-tool interop in `crates/`                                               | [Format Crate Test Strategy](testing/format-crate-tests.md)                    |

## Invariants an agent hits immediately

- **`romfiles` paths are stored relative to `ROM_DIRECTORY`.** Strip the prefix on the way in, join
  it on the way out; treating the column as absolute silently corrupts the library.
- **All user-facing output goes through the `progress.rs` helpers** bound to the global
  `MultiProgress`. Never `println!` from a code path the MCP server can reach: with `oxyromon mcp`
  (or `/mcp`) stdout carries JSON-RPC.
- **Every notable change updates `README.md` and `CHANGELOG.md`** — new flags, behavior changes and
  fixes alike, under the current version's section.
- **One module per action, one interface.** `subcommand()` declares argv and does no I/O;
  `async main(connection, matches, progress_bar)` does the work. GraphQL and MCP both synthesize
  argv to reach it, so a new capability added anywhere else is invisible to two of the three
  surfaces.
- **Query changes require regenerating offline sqlx data** (`cargo sqlx prepare -- --features server`).
- **Validate the way CI does**: `cargo test --workspace --features oxyromon/server`, with the shared
  `MUTEX` held by anything touching global state.
