---
type: architecture
title: System Architecture Overview
description: The layered shape of oxyromon — a clap entrypoint, one module per subcommand, shared database/model/common/config/util/transcode layers, format modules running every codec in-process over the crates/* format crates, and SQLite — plus how the GraphQL and MCP surfaces enter at the subcommand layer, and why ctrtool is the only remaining external tool.
tags: [architecture, layers, cli, graphql, mcp, sqlite, workspace, cargo-features]
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-36f054f1f38f2197a6af5bca
    resource: repo://build.rs
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-ca6cb4b1a14fd7969dfae3ec
    resource: repo://CHANGELOG.md
  - id: openwiki-source-92ee3a60a9826201e8ce6fa7
    resource: repo://desktop/Cargo.toml
  - id: openwiki-source-c1fba58d5f1ff9717d354a3c
    resource: repo://desktop/src/server.rs
  - id: openwiki-source-182eb6366058c61b0d4d15e9
    resource: repo://frontend/Cargo.toml
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-02628a361dd4faa0f62f87a2
    resource: repo://src/archive.rs
  - id: openwiki-source-c3dbd15c40e250cdf06fed8d
    resource: repo://src/chd.rs
  - id: openwiki-source-f1234afbb261ce7190a87845
    resource: repo://src/check_roms.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-14bdaf940b9be722639ecfcd
    resource: repo://src/config/test_cli.rs
  - id: openwiki-source-10fddfccad5d7bf89d675953
    resource: repo://src/convert_roms.rs
  - id: openwiki-source-63f87054c5f9055be025dd77
    resource: repo://src/ctrtool.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-bfaccf08e532321769e4b6b5
    resource: repo://src/export_roms.rs
  - id: openwiki-source-6f95130cb448e88dd97ab560
    resource: repo://src/import_dats.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-7fb657fead83b71b9836ddac
    resource: repo://src/info.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-b95ec25c5cfc0be350df258c
    resource: repo://src/mcp.rs
  - id: openwiki-source-8d46195d3bf969b1c0c3d484
    resource: repo://src/mcp/actions.rs
  - id: openwiki-source-f6b27667092492e425164ff8
    resource: repo://src/mcp/read.rs
  - id: openwiki-source-762e9c43ab8143e7dda872d3
    resource: repo://src/mcp/write.rs
  - id: openwiki-source-cc76cc07825d7eb399a2b402
    resource: repo://src/model.rs
  - id: openwiki-source-528b2bf14ffa06b3842b8c96
    resource: repo://src/mutation.rs
  - id: openwiki-source-d3402490d16a663b822e8a00
    resource: repo://src/nsz.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-313c10a1161be7da48ebd676
    resource: repo://src/purge_roms.rs
  - id: openwiki-source-f314ed3b08dd849d591fdb0f
    resource: repo://src/query.rs
  - id: openwiki-source-db13c01516e9bd04294e339d
    resource: repo://src/rvz.rs
  - id: openwiki-source-dfa02c19f5d8e4215d36f9c0
    resource: repo://src/server.rs
  - id: openwiki-source-4fe52c7cb090ec0387522035
    resource: repo://src/transcode.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
generated: { by: "openwiki/0.7.2", at: "2026-10-10T22:10:44.671Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-14T21:01:38.051Z
---

# System Architecture Overview

oxyromon is one Rust package that builds one binary (`oxyromon`) and can wear
three hats: a batch CLI, a web server carrying a GraphQL API and an embedded SPA,
and an MCP server. Those hats are not parallel implementations. Every mutation of
the library — importing DATs, importing ROMs, sorting, converting, checking,
purging — exists exactly once, as a subcommand module's `main()`, and the server
and MCP surfaces reach it by synthesizing argv and calling that function. The
layers below are ordered by who calls whom, not by directory.

## The layers

```mermaid
flowchart TD
  subgraph SURFACES["Entry surfaces"]
    CLI["clap argv parsed in main.rs"]
    GQL["POST /graphql plus multipart uploads"]
    MCPC["MCP stdio, or /mcp on the web listener"]
  end

  CLI -->|"connection, ArgMatches, ProgressBar"| SUB["Subcommand main in a feature module"]
  GQL -->|"mutation.rs spawn_cli_action, synthesized argv"| SUB
  MCPC -->|"mcp/actions.rs run_action, synthesized argv plus write lock"| SUB

  SUB --> SHARED["Shared layers: database.rs, model.rs, common.rs, config.rs, util.rs, mimetype.rs, progress.rs, prompt.rs, transcode.rs"]
  SHARED --> FMT["Format modules: archive, chd, ctrtool, rvz, wbfs, iso, xso, nsz, gdi, xdelta, xps, crc32"]
  FMT --> CRATES["In-process engines: crates chd-rs gdi-rs nsz-rs xdelta-rs xps-rs xso-rs, plus sevenz-rust2, zip, nod"]
  FMT --> EXT["ctrtool subprocess"]
  SHARED --> SQLITE[("oxyromon.db, SQLite via sqlx")]
  SHARED --> DISK[("ROM directory on the filesystem")]
  SUB --> PROG["progress.rs output on stderr"]
```

*Caption: the layer stack and the three places a request can enter — argv, GraphQL, and MCP — converging on the same subcommand `main()`, with every codec now running in-process except the one `ctrtool` subprocess.*

**Entrypoint.** `src/main.rs` is the root package's only binary target. It builds the clap
`Command` from each module's `subcommand()`, runs the process-wide startup
(`dotenv`, the `OXYROMON_LOG_LEVEL` logger bridged onto the global
`MultiProgress`, the data directory, `oxyromon.db`, the pool, the ROM and tmp
directory defaults), and then dispatches on the matched subcommand name. Batch
subcommands run inside a block that holds the single pooled connection; `mcp` and
`server` are dispatched *after* that block and are handed a cloned `SqlitePool`
because they must multiplex requests. See
[CLI Entrypoint and the Subcommand Contract](cli-entrypoint-and-subcommands.md).

**Subcommand modules.** One module per user-facing action (`import_dats`,
`import_roms`, `sort_roms`, `convert_roms`, `check_roms`, `export_roms`,
`rebuild_roms`, `purge_*`, `generate_playlists`, `download_dats`, `config`,
`info`, `benchmark`), all obeying `pub fn subcommand() -> Command` plus
`pub async fn main(connection, matches, progress_bar)`. `subcommand()` is a pure
argv-shape declaration with no I/O — which is precisely what lets a non-CLI host
build an `ArgMatches` itself.

**Shared layers.** `database.rs` holds every SQL statement as a standalone
`pub async fn` taking `&mut SqliteConnection`; `model.rs` holds the row types
those queries materialize, the enums stored as integers, and the Logiqx XML
deserialization types. `common.rs` defines `CommonRomfile` (a path plus optional
system id) and the trait system that abstracts over file formats. `config.rs`
reads and writes the `settings` table with per-system override falling back to
the global row; `util.rs` wraps filesystem operations and the directory layout;
`mimetype.rs` sniffs ROM magic bytes; `progress.rs` owns the progress bars and all
categorized output; `prompt.rs` supplies interactive selection.

**The decode layer.** `transcode.rs` is the one place that turns a stored romfile
into something a conversion can consume. Each helper returns
`Decoded<T> { source, inner }`: `source` is the romfile as the database knows it
— the path and size a conversion must be committed against — and `inner` is the
same content in a neutral format (`CommonRomfile`, `IsoRomfile`, `CueBinRomfile`,
`NspRomfile`) written into the caller's `TempDir`. `archive_to_common` extracts an
archive's single file, `common_as_source` views a plain file as its own source,
`xso_to_iso`/`nsz_to_nsp`/`rvz_to_iso` decode a compressed container, and
`archive_to_cue_bin`/`assemble_cue_bin` build a CUE/BIN set from an archive or from
loose romfiles. Because every path into a neutral format passes here, parent-CHD
resolution lives in one function: `romfile_as_chd` follows `romfile.parent_id` and
calls `as_chd_with_parent`, so `check_roms`, `convert_roms` and `export_roms` never
re-implement it, and `chd_to_iso` layers the DVD-only step on top — returning
`Ok(None)` for a non-DVD CHD so a caller can skip the chain rather than branch on
the type itself.

**Format modules.** Each builds a format-specific romfile struct and `As*` /
`To*` / `Check` traits around `CommonRomfile`. Every codec is now an in-process
engine: `chd.rs` drives `chd-rs` (a port of chdman, for CDs, DVDs, hard disks and
LaserDiscs), `xso.rs`/`nsz.rs`/`gdi.rs`/`xdelta.rs`/`xps.rs` drive the matching
`crates/*` member, `archive.rs` uses sevenz-rust2 and zip, and `rvz.rs`/`wbfs.rs`
use `nod`. Each runs its crate's synchronous entry point on the blocking pool —
`run_blocking` for the crate codecs that report byte progress, `spawn_blocking`
directly in `archive.rs`, `rvz.rs` and for the CHD header read — so the runtime is
never blocked by a compress or a hash. These modules report `"built-in"` from
`get_version()`; `nsz.rs` adds when `prod.keys` is missing so that is not first
discovered mid-compression. **`ctrtool.rs` is the only module left that spawns an
external process**, and only for CIA: `parse_cia` and `extract_files_from_cia` run
`ctrtool -p -v` and parse its listing, and `import_roms` checks
`ctrtool::get_version()` and prints `Required tool not found: ctrtool` instead of
failing deeper in. Details in
[Format Modules and Format Crates](../concepts/format-modules-and-format-crates.md).

**Storage.** One SQLite file. `establish_connection` shapes the pool from the
Cargo feature set — one connection with `PRAGMA locking_mode = EXCLUSIVE` for a
default CLI build, five with `NORMAL` when `server` or `mcp` is compiled in —
applies the tuning pragmas, and runs the compile-time-embedded `MIGRATOR`. Every
query is verified against the schema at compile time. See
[Data Model, SQLite Usage and Migrations](data-model-and-migrations.md).

## One implementation per action

This is the invariant to preserve when adding a capability. The subcommand's
`main(&mut SqliteConnection, &ArgMatches, &ProgressBar)` is the unit of work, and
the non-CLI hosts adapt to it rather than the reverse:

- **GraphQL.** `src/mutation.rs`'s `spawn_cli_action` takes a closure over a
  connection, spawns a task, sends `{prefix}_started` over SSE, acquires its own
  pooled connection (reporting an acquire failure as `{prefix}_error` rather than
  panicking), then runs the closure. Each mutation's closure builds an argv vec,
  parses it with the module's own `subcommand().try_get_matches_from(...)`, and
  calls that module's `main()` with a `ProgressBar::hidden()`.
- **MCP.** `src/mcp/actions.rs`'s `run_action` does the same thing synchronously:
  acquire the process-wide `write_lock`, acquire one connection, create a hidden
  progress bar, then drive the subcommand. Mutating tools such as `sort_roms`,
  `check_roms`, `convert_roms` and `purge_roms` build argv and hand it to
  `sort_roms::main`, `check_roms::main`, and so on.
- **HTTP uploads.** `server.rs`'s upload handlers stream the body to a temp
  directory and then drive the same work on a spawned task — `import_roms::main`
  and `import_irds::main` through synthesized argv, `POST /dats` through
  `import_dats`'s own `process_dat_upload` helper — reporting progress over SSE.

```mermaid
sequenceDiagram
  participant UI as Web UI
  participant M as mutation resolver
  participant T as spawned task
  participant B as SSE events
  participant S as subcommand main

  UI->>M: sort_roms with system_id
  M->>M: resolve system name, validate input
  M->>T: spawn_cli_action with closure
  T->>B: sort_roms_started
  T->>T: acquire pooled connection, else report error
  T->>S: parse argv then call the module main
  S-->>T: Ok or Err
  T->>B: sort_roms_complete or sort_roms_error
  M-->>UI: true as soon as the task is handed off
```

*Caption: the GraphQL path hands work to the subcommand layer and returns; the outcome arrives asynchronously over `/events`.*

Because argv is the interchange format, both hosts push the non-interactive flags
a human would answer at a prompt — `-y`/`--yes` for `confirm()` on destructive
subcommands, `-u`/`--unattended` for ambiguous ROM matches, `--system <name>` to
skip the system multiselect. Where argv is the wrong shape the host still does not
get a second implementation: it calls the module's exported helper directly —
`purge_systems::purge_system` for the GraphQL purge, `download_dats::download_redump_system`
for the MCP `download_dats` tool. A subcommand that grows a prompt that cannot be
bypassed, or that reads process state outside its three arguments, breaks both
servers and the test suite at once.

The hosts differ only around the shared core: GraphQL actions run *asynchronously*
and report only start/complete/error over SSE, while MCP actions run to completion
inside the request, hold the write lock for the whole action so SQLite writer
serialization is ordered instead of surfacing as `SQLITE_BUSY`, poll the bar
roughly twice a second into `notifications/progress`, and race the work against
per-request and server-shutdown cancellation tokens. MCP read tools take no lock
at all, and in `--read-only` mode the write routes are disabled so they never
appear in `tools/list`. See [MCP Server](../server/mcp-server.md) and
[HTTP Server and GraphQL API](../server/http-server-and-graphql-api.md).

## Entry surfaces and what they own

| Surface | Entered at | Owns |
| --- | --- | --- |
| CLI | `main.rs` clap dispatch, scoped connection | interactive selection, styled terminal output |
| GraphQL + uploads + SSE | `server.rs` axum router, `query.rs` / `mutation.rs` | schema, DataLoaders, `/events` broadcast, embedded assets |
| MCP | `mcp.rs` (`oxyromon mcp` stdio, or `/mcp` nested by `server.rs`) | tool/resource/prompt catalog, write lock, progress bridging |

`server.rs` builds one axum `Router`: `POST /graphql`, `GET /events`, multipart
`POST /dats`, `/roms`, `/patches`, `/irds`, `GET /romfiles/{id}` for downloads,
the nested `/mcp` Streamable HTTP service, then the embedded SPA from
`target/assets` via `rust-embed`. The MCP endpoint shares the listener and the
same trust level as `/graphql`; when mounted this way, its mutating actions also
fan their outcome into the SSE stream, so a browser watching `/events` reacts to
work started over MCP.

The desktop app adds no layer: `desktop/src/server.rs` reserves a loopback port,
spawns the same binary as a Tauri sidecar running `server --address 127.0.0.1
--port <port>`, polls until it accepts connections, and points a webview at it.

## Workspace and build boundaries

The repository is a Cargo workspace whose members are the root `oxyromon` package
plus `crates/*` — the format crates (`chd-rs`, `gdi-rs`, `nsz-rs`, `xdelta-rs`,
`xps-rs`, `xso-rs`) that replaced the external tools oxyromon used to shell out to.
They share one lockfile and CI but are versioned and published independently, and
oxyromon depends on them by `path` + `version` with `default-features = false`.

`frontend/` and `desktop/` are **not** workspace members. Each declares its own
`[workspace]` table so cargo does not walk up to the root package: the
WebAssembly toolchain of the Leptos SPA and the Tauri/webkit dependencies stay
out of the native CLI build entirely. They are reached at build time instead —
`build.rs` runs `trunk build --release` in `frontend/` when the `server` feature
is on (`SKIP_TRUNK=true` skips it) and the output is embedded by `rust-embed`.

Cargo features decide which surfaces exist at all. `server` implies `mcp`;
`mcp` alone compiles only the stdio server. `src/main.rs` gates whole modules
(`mcp`, `mutation`, `query`, `server`, `validator`) behind those features, so a
default build never compiles the HTTP stack — and `#[cfg(feature = "server")]`
on the `model.rs` types is what turns the same structs into GraphQL
`SimpleObject`s without duplicating them. See
[Feature Flags and Build Topology](feature-flags-and-build-topology.md).

## Cross-cutting invariants

- **`romfiles.path` is relative to the `ROM_DIRECTORY` setting.** Strip on write,
  join on read; `CommonFile::get_relative_path` and the `download_romfile`
  handler are the two ends of that rule.
- **stdout may be a protocol channel.** Under `oxyromon mcp` (or `/mcp`) stdout
  carries MCP JSON-RPC. All user-facing output goes through the `print_*` helpers
  in `progress.rs`, which write through the `MultiProgress` on stderr, and
  `log::*` records are bridged there too.
- **Blocking work stays off the async runtime.** Format-crate entry points are
  synchronous and take a `&mut dyn FnMut(u64)` progress callback;
  `run_blocking` runs them on the blocking pool, resetting the bar to a bytes
  style with the input size as its length and feeding that callback into it. The
  one external process, `ctrtool`, is spawned with `tokio::process::Command`
  directly, and `util::tool_version` is the only place a version probe shells out.
- **Availability is reported, not guessed.** `info`, the GraphQL `dependencies`
  query and the MCP `status` payload all walk the same `get_version()` set —
  `sevenz-rust2`, `chd-rs`, `ctrtool`, `xps-rs`, `xso-rs`, `nsz-rs`, `xdelta-rs`,
  and `nod` for RVZ/WBFS. An in-process engine always succeeds with `built-in`, so
  only `ctrtool` can come back missing, and a missing `ctrtool` is surfaced as a
  skip rather than a failure deep inside an import.
- **Shutdown is housekeeping.** `optimize_database` (incremental vacuum, optimize,
  WAL checkpoint truncate) runs after the CLI batch finishes, and
  `server.rs`'s `shutdown_signal` runs it before cancelling in-flight work, so a
  server exits with a checkpointed database.
- **The directory tree is reclaimed as well as the rows.** `util::get_kept_directories`
  names the ROM, system and `Trash` directories that must survive, and
  `util::remove_empty_directories` climbs from a deleted file's directory up to a
  kept or non-empty one — the rule `purge-roms` and DAT/IRD updates follow so an
  emptied game folder does not linger. `util::compute_system_completion` is the
  counterpart on the database side: import, sort, check, rebuild and purge all end
  by recomputing game and system completion rather than patching the counter
  themselves.
- **Tests use the same front door.** A test builds a temp database with
  `establish_connection`, points the ROM and tmp settings at `TempDir`s, parses
  argv with the module's `subcommand()`, calls `main(&mut connection, &matches,
  &ProgressBar::hidden())`, and asserts on database plus filesystem state, with
  the `cfg(test)`-only `MUTEX` from `config.rs` serializing shared state. There is
  no CLI harness — which is why the subcommand signature must stay host-agnostic.
