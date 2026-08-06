# AGENTS.md — Guide for AI Agents Working on oxyromon

## Project Overview

**oxyromon** (oxyROMon) is a cross-platform opinionated CLI ROM organizer written in Rust. It validates ROM files against known-good databases (DAT files), imports them into a managed directory structure, sorts them by region/preference, and can convert between various ROM formats. It also includes an optional web UI (Leptos + GraphQL) behind the `server` feature flag.

- **Author:** Maxime Gauduin (alucryd)
- **License:** GPL-3.0+
- **Rust Edition:** 2024
- **MSRV:** 1.94.0
- **Repository:** https://github.com/alucryd/oxyromon

## Architecture

### High-Level Design

oxyromon is a CLI application built with `clap` for argument parsing, `sqlx` with SQLite for persistence, and `tokio` as the async runtime. Each CLI subcommand is implemented as its own module following a consistent pattern. An optional web server (GraphQL API + Leptos WebAssembly SPA) is gated behind the `server` Cargo feature.

```
┌─────────────────────────────────────────────────────┐
│  main.rs  (CLI entry point, subcommand dispatch)    │
├─────────────────────────────────────────────────────┤
│  Subcommand modules (import_roms, sort_roms, etc.)  │
├─────────────────────────────────────────────────────┤
│  Shared layers:                                     │
│    database.rs  – all SQL queries                   │
│    model.rs     – data structs & XML deserialization │
│    common.rs    – romfile abstraction & traits       │
│    config.rs    – settings management                │
│    util.rs      – filesystem helpers                 │
│    mimetype.rs  – file type detection                │
│    progress.rs  – progress bar helpers               │
│    prompt.rs    – interactive selection helpers       │
├─────────────────────────────────────────────────────┤
│  Format-specific modules:                           │
│    sevenzip.rs, chdman.rs, maxcso.rs, dolphin.rs,   │
│    nsz.rs, wit.rs, ctrtool.rs, flips.rs, xdelta3.rs,│
│    bchunk.rs, gdidrop.rs, crc32.rs                  │
├─────────────────────────────────────────────────────┤
│  Server modules (behind "server" feature):          │
│    server.rs, query.rs, mutation.rs, validator.rs   │
│    + Leptos SPA (frontend/), Tauri shell (desktop/) │
├─────────────────────────────────────────────────────┤
│  SQLite (via sqlx) + migrations/                    │
└─────────────────────────────────────────────────────┘
```

### Key Modules

| Module            | Purpose                                                                                                                                                                                                                                                                                                    |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `main.rs`         | Entry point. Registers all subcommands, establishes DB connection, dispatches to subcommand `main()` functions.                                                                                                                                                                                            |
| `model.rs`        | All data structures: DB row types (`System`, `Game`, `Rom`, `Romfile`, `Patch`, `Setting`), XML deserialization types (`DatfileXml`, `GameXml`, `RomXml`, etc.), and enums (`Merging`, `Sorting`, `Completion`, `RomfileType`).                                                                            |
| `database.rs`     | All database operations as standalone async functions. Uses `sqlx` with compile-time checked queries. Contains the `MIGRATOR` for schema migrations.                                                                                                                                                       |
| `common.rs`       | The `CommonRomfile` struct and a rich trait system (`FromPath`, `CommonFile`, `AsCommon`, `ToCommon`, `AsIso`, `AsCueBin`, `ToIso`, `ToCueBin`, `Patch`, `Playlist`, `Size`, `HashAndSize`, `HeaderedHashAndSize`, `Check`, `Persist`) that abstracts over different ROM file formats.                     |
| `config.rs`       | Settings management. Defines setting types (booleans, integers, paths, lists, choices), and provides get/set helpers. Also defines `HashAlgorithm`, `SubfolderScheme`, `PreferredVersion`, `PreferredRegion`, `ArcadeRomType`.                                                                             |
| `util.rs`         | Low-level filesystem operations (`create_file`, `copy_file`, `rename_file`, `remove_file`, `create_directory`, etc.) and directory structure helpers (`get_system_directory`, `get_one_region_directory`, `get_trash_directory`). Also contains `compute_system_completion` and `compute_alpha_subfolder`. |
| `mimetype.rs`     | File type detection using the `infer` crate with custom matchers for ROM-specific formats (CHD, CSO, RVZ, IRD, BPS, IPS, XDELTA, ZSO, RDSK, RIFF). Defines all file extension constants.                                                                                                                   |
| `progress.rs`     | Progress bar styles, `MultiProgress` management, `indicatif-log-bridge` integration, and categorized styled output helpers (`print_header`, `print_info`, `print_success`, `print_warning`, `print_error`, `print_skip`, `print_action`, `print_separator`).                                               |
| `import_dats.rs`  | Parses Logiqx-format DAT files, creates/updates systems, games, and ROMs in the database.                                                                                                                                                                                                                  |
| `import_roms.rs`  | Imports ROM files by hashing them and matching against the database. Handles archives, CHDs, CIAs, CSOs, RVZs, NSZs, ZSOs, and plain files.                                                                                                                                                                |
| `sort_roms.rs`    | Sorts imported ROMs into region folders and 1G1R directories based on user configuration. Implements the weighted election algorithm.                                                                                                                                                                      |
| `convert_roms.rs` | Converts between ROM formats (archive ↔ original, ISO ↔ CHD ↔ CSO/ZSO, ISO ↔ RVZ, etc.).                                                                                                                                                                                                                   |
| `check_roms.rs`   | Verifies ROM integrity by re-hashing and comparing against database records.                                                                                                                                                                                                                               |
| `rebuild_roms.rs` | Rebuilds arcade ROM sets between merging strategies (split, non-merged, full non-merged).                                                                                                                                                                                                                  |
| `export_roms.rs`  | Exports ROMs to various formats without modifying the originals.                                                                                                                                                                                                                                           |
| `sevenzip.rs`     | 7z/ZIP archive abstraction. `ArchiveRomfile` struct with `AsArchive`, `ToArchive` traits. Shells out to the `7zz`/`7z` executable.                                                                                                                                                                         |
| `chdman.rs`       | CHD format abstraction. `ChdRomfile` struct with `AsChd`, `ToChd` traits. Shells out to `chdman`.                                                                                                                                                                                                          |
| `server.rs`       | Axum-based web server with GraphQL (async-graphql), SSE for real-time updates, and embedded static assets from the Trunk/Leptos build (`target/assets`).                                                                                                                                                                           |
| `query.rs`        | GraphQL query resolvers. Uses DataLoader pattern for N+1 prevention.                                                                                                                                                                                                                                       |
| `mutation.rs`     | GraphQL mutation resolvers for settings and system management.                                                                                                                                                                                                                                             |
| `validator.rs`    | GraphQL input validators.                                                                                                                                                                                                                                                                                  |

### Format-Specific Module Pattern

Each external tool module (e.g., `chdman.rs`, `sevenzip.rs`, `maxcso.rs`, `dolphin.rs`) follows the same pattern:

1. Define a struct wrapping `CommonRomfile` (e.g., `ChdRomfile`, `ArchiveRomfile`).
2. Implement `Size`, `HashAndSize`, and `Check` traits for integrity verification.
3. Define `As*` trait (parse an existing file into the struct) and `To*` trait (convert from another format).
4. Shell out to an external executable via `tokio::process::Command`.
5. Provide a `get_version()` function to check if the tool is available.

### Data Flow

1. **Import DATs** → Parse XML → Create `System`, `Game`, `Rom` records in DB.
2. **Import ROMs** → Hash file → Match against DB `Rom` records → Move file to system directory → Create `Romfile` record → Link `Rom.romfile_id`.
3. **Sort ROMs** → Read user region/preference config → Classify games → Move files to appropriate directories (main, 1G1R, Trash) → Update `Game.sorting`.
4. **Convert ROMs** → Read source format → Convert to target format via external tool → Update `Romfile` path/size.
5. **Check ROMs** → Re-hash files → Compare against DB → Move failures to Trash.

## Database

### Technology

- **SQLite** via `sqlx` with compile-time query verification.
- Connection pool configured differently for CLI (1 connection, EXCLUSIVE locking) vs server (5 connections, NORMAL locking).
- WAL journal mode enabled for performance.

### Schema (Key Tables)

| Table      | Key Columns                                                                                                                                   | Purpose                                                    |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `systems`  | id, name, custom_name, custom_extension, description, version, url, completion, arcade, merging                                               | Game system definitions from DAT files                     |
| `games`    | id, name, description, comment, external_id, device, bios, jbfolder, regions, sorting, completion, system_id, parent_id, bios_id, playlist_id | Individual game entries                                    |
| `roms`     | id, name, bios, disk, size, crc, md5, sha1, rom_status, game_id, romfile_id, parent_id, original                                              | ROM file definitions (expected files)                      |
| `romfiles` | id, path, size, parent_id, romfile_type                                                                                                       | Actual files on disk (paths are relative to ROM_DIRECTORY) |
| `patches`  | id, name, index, rom_id, romfile_id                                                                                                           | Patch files (BPS, IPS, XDELTA)                             |
| `headers`  | id, name, version, size, system_id                                                                                                            | ROM header definitions for headered systems                |
| `rules`    | id, start_byte, hex_value, header_id                                                                                                          | Header detection rules                                     |
| `settings` | id, key, value                                                                                                                                | Key-value configuration store                              |

### Migrations

Migrations live in `migrations/` and use the `sqlx::migrate!()` macro (embedded at compile time). Files are named with timestamps: `YYYYMMDDHHMMSS_description.sql`.

**When adding a new migration:**

1. Create a new `.sql` file in `migrations/` with the next timestamp.
2. Run `cargo sqlx prepare` to update the `.sqlx/` directory (offline query data).
3. The `sqlx-data.json` file in the root may also need updating.

### Query Pattern

All database queries are standalone `pub async fn` functions in `database.rs`. They take a `&mut SqliteConnection` as the first argument and return domain types from `model.rs`. Queries use `sqlx::query_as!` or `sqlx::query!` macros for compile-time verification. Transactions are managed via `begin_transaction`, `commit_transaction`, and `rollback_transaction` helpers.

## Building & Running

### Prerequisites

- Rust 1.88.0+ (edition 2024)
- For the `server` feature: the `wasm32-unknown-unknown` target and [Trunk](https://trunkrs.dev)

### CLI Only

```sh
cargo build --release
```

### With Web UI

```sh
cargo build --release --features server
```

The `build.rs` script automatically runs `trunk build --release` (in `frontend/`) when the `server` feature is enabled (skip with `SKIP_TRUNK=true`). The Leptos app is compiled to WebAssembly, output to `target/assets/`, and embedded into the binary via `rust-embed`.

### Helper Scripts

Run from the repository root:

| Script         | Builds                                                            |
| -------------- | ----------------------------------------------------------------- |
| `build.sh`     | The web UI, then the CLI with it embedded (`--release --features server`) |
| `desktop.sh`   | The Tauri desktop app and its installers                          |
| `dist.sh`      | Release artifacts for every cross-compiled target, plus the desktop bundles for the host, into `dist/` |
| `docker.sh`    | The two container images, then pushes them                        |
| `test.sh`      | The test suite under `cargo llvm-cov`, then opens the report      |

### Environment Variables

| Variable                  | Purpose                                                                |
| ------------------------- | ---------------------------------------------------------------------- |
| `OXYROMON_DATA_DIRECTORY` | Override the data directory (default: platform data dir + `/oxyromon`) |
| `OXYROMON_ROM_DIRECTORY`  | Override the default ROM directory                                     |
| `OXYROMON_TMP_DIRECTORY`  | Override the default temp directory                                    |
| `OXYROMON_LOG_LEVEL`      | Control log verbosity (standard `env_logger` syntax)                   |
| `SKIP_TRUNK`              | Set to `true` to skip the web UI (`trunk build`) in `build.rs`          |
| `DATABASE_URL`            | Used by `sqlx` for compile-time query checking                         |

## Feature Flags

| Feature          | Description                                             | Default |
| ---------------- | ------------------------------------------------------- | ------- |
| `server`         | Builds the web server subcommand (GraphQL + Leptos SPA) | Off     |
| `use-rustls`     | Use rustls for TLS                                      | On      |
| `use-native-tls` | Use system OpenSSL for TLS                              | Off     |

Server-only code is gated with `#[cfg(feature = "server")]` throughout the codebase.

## Testing

### Running Tests

```sh
# Run all tests (includes server feature tests)
cargo test --features server

# Run with coverage
cargo llvm-cov --features server --lcov --output-path lcov.info

# Or use the helper script
./test.sh
```

### Test Infrastructure

- Tests use **temporary databases** (`NamedTempFile`) and **temporary directories** (`TempDir`) so they don't interfere with each other or with real data.
- A global **`MUTEX`** (`tokio::sync::Mutex`) defined in `config.rs` serializes tests that share global state (used via `let _guard = MUTEX.lock().await;`).
- Tests for subcommand modules exercise the full pipeline: create temp DB → import DAT → import ROM → run subcommand → assert DB state and filesystem state.
- Test DAT files and ROM fixtures live in the `tests/` directory.
- External tool tests will naturally be skipped if the tool isn't installed (tests check `get_version()` first or rely on the tool being available in CI).
- Tests for `wiremock` (dev dependency) are used for HTTP-based tests (e.g., `download_dats`).

### Test Pattern

Each module's tests are in separate files under a subdirectory matching the module name (e.g., `src/sort_roms/test_sort.rs`, `src/import_roms/test_original.rs`). They're included via `#[cfg(test)] mod test_name;` at the bottom of the parent module.

A typical test:

```rust
#[tokio::test]
async fn test() {
    // 1. Acquire the global mutex
    let _guard = MUTEX.lock().await;

    // 2. Set up temp DB and directories
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let rom_directory = TempDir::new_in("tests").unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in("tests").unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    // 3. Import a DAT file
    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar).await.unwrap();

    // 4. Perform operations being tested
    // ...

    // 5. Assert DB state and filesystem state
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), expected_count);
}
```

### CI

GitHub Actions workflow in `.github/workflows/continuous_integration.yml`:

- Runs on Ubuntu 24.04
- Installs system dependencies: `bchunk`, `mame-tools` (for chdman), `wit`
- Runs `clippy` with `--features server`
- Builds with `--release --features server`
- Runs tests with `cargo llvm-cov` for coverage
- Uploads coverage to Codecov

## Adding a New Subcommand

Follow this checklist:

1. **Create the module file** at `src/my_command.rs`.
2. **Implement the standard interface:**

   ```rust
   pub fn subcommand() -> Command {
       Command::new("my-command")
           .about("Description")
           .arg(/* ... */)
   }

   pub async fn main(
       connection: &mut SqliteConnection,
       matches: &ArgMatches,
       progress_bar: &ProgressBar,
   ) -> SimpleResult<()> {
       // Implementation
       Ok(())
   }
   ```

3. **Register in `main.rs`:**
   - Add `mod my_command;` to the module declarations.
   - Add `my_command::subcommand()` to the `subcommands` vec.
   - Add `Some("my-command") => my_command::main(...)` to the match block.
4. **Write tests** in `src/my_command/test_*.rs` files, using the test pattern above.
5. **Add `#[cfg(test)] mod test_*;`** at the bottom of your module file.

## Adding a New ROM Format

1. **Create a module** (e.g., `src/myformat.rs`) following the format-specific module pattern.
2. **Define the romfile struct** wrapping `CommonRomfile`.
3. **Implement core traits:** `Size`, `HashAndSize`, `Check`.
4. **Implement conversion traits:** `As*` (parse), `To*` (convert from other formats).
5. **Add MIME type detection** in `mimetype.rs` if the format has magic bytes.
6. **Add file extension constant** in `mimetype.rs`.
7. **Register the module** in `main.rs` (`mod myformat;`).
8. **Integrate with `import_roms.rs`** — add a branch in `import_rom()` for the new format.
9. **Integrate with `convert_roms.rs`** — add conversion paths to/from the new format.
10. **Integrate with `check_roms.rs`** — add a check branch for the new format.

## Adding a New Database Migration

1. Create a file in `migrations/` named `YYYYMMDDHHMMSS_description.sql`.
2. Write the SQL migration (SQLite dialect).
3. If adding a new column to an existing table, update the corresponding struct in `model.rs`.
4. If adding new query functions, add them to `database.rs`.
5. Run `cargo sqlx prepare -- --features server` to regenerate offline query data in `.sqlx/`.
6. Verify with `cargo build --features server`.

## Adding a New Setting

1. **Add the setting key** to the appropriate category constant in `config.rs`:
   - `BOOLEANS` for boolean settings
   - `CHOICES` for single-choice settings (also add the enum)
   - `CHOICE_LISTS` for multi-choice settings
   - `INTEGERS` for integer settings (define valid range)
   - `LISTS` for free-form list settings
   - `PATHS` for directory path settings
   - Add to `NULLABLES` if the setting can be unset
   - Add to `SORTED_LISTS` if order should be preserved
2. **Add a database migration** to set the default value (if needed).
3. **Use the setting** via `get_bool`, `get_integer`, `get_list`, `get_string`, or `get_directory` from `config.rs`.
4. **Expose in GraphQL** if needed — update `mutation.rs` and `query.rs`.

## Web UI (Server Feature)

### Stack

- **Backend:** Axum + async-graphql + SSE
- **Frontend:** [Leptos](https://leptos.dev) (CSR / client-side rendering, compiled to WebAssembly) + [Web Awesome](https://webawesome.com) web components
- **Build:** [Trunk](https://trunkrs.dev), output to `target/assets/`, embedded via `rust-embed`
- **GraphQL:** Single `/graphql` endpoint, schema defined in `query.rs` (queries) and `mutation.rs` (mutations)
- **SSE:** `/events` endpoint for real-time updates (e.g., purge progress)

The frontend lives in its own standalone crate under `frontend/` (kept separate
so its WebAssembly dependencies never pollute the native `oxyromon` build). It
talks to the same backend `/graphql`, `/events`, `/dats` and `/romfiles/{id}`
endpoints, so `server.rs` is unchanged by the framework choice.

### Frontend Structure

| Path                                   | Purpose                                                     |
| -------------------------------------- | ---------------------------------------------------------- |
| `frontend/index.html`                  | Trunk entry point (links the wasm bundle + generated CSS)  |
| `frontend/styles.css`                  | The app's own CSS — the shell, the rows, the bare targets  |
| `frontend/Trunk.toml`                  | Trunk config (pre-build hooks, dev proxies)                |
| `frontend/scripts/fetch-webawesome.sh` | Vendors Web Awesome + its icons into `frontend/vendor/`    |
| `frontend/src/main.rs`                 | Mounts the app to the DOM                                  |
| `frontend/src/app.rs`                  | Root component + reactive data-loading effects             |
| `frontend/src/page.rs`                 | Main page (systems/games/roms/romfiles tables, stats)      |
| `frontend/src/components/`             | Navbar, notifications, about/import/settings modals        |
| `frontend/src/state.rs`                | Global reactive state (`RwSignal`s in a `Copy` `AppState`) |
| `frontend/src/api.rs`                  | GraphQL client + query/mutation helpers                    |
| `frontend/src/sse.rs`                  | SSE client + notification helpers                          |
| `frontend/src/model.rs`                | Serde types mirroring the GraphQL responses                |
| `frontend/src/ui.rs` / `icons.rs`      | Reusable modal/windowing/media-query helpers + inline SVG icons |

### Frontend Dev

Requires the `wasm32-unknown-unknown` target and the [Trunk](https://trunkrs.dev)
bundler (no Node.js toolchain):

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk

# From the frontend/ directory:
cd frontend

# Dev server with hot reload (proxies /graphql, /events, /dats, /romfiles to a
# running `oxyromon server` on 127.0.0.1:8000 — see the [[proxy]] entries in Trunk.toml)
trunk serve

# Production build (outputs to ../target/assets/)
trunk build --release
```

The top-level `build.rs` runs `trunk build --release` automatically when the
`server` feature is enabled (skip with `SKIP_TRUNK=true`).

### Web Awesome

The UI is built from [Web Awesome](https://webawesome.com) (the successor to
Shoelace) — MIT licensed custom elements. Leptos renders them like any other
tag; two things are worth knowing:

- **Properties, not attributes, for state.** `prop:open=...` sets the JS
  property directly, which is what Lit-based components react to.
- **Custom events need a type annotation.** The `view!` macro maps an unknown
  `on:` name onto `Custom::new(...)`, but cannot infer the payload:
  `on:wa-after-hide=move |_: web_sys::Event| ...`. Known events (`on:click`)
  keep their own type and must *not* be annotated as `web_sys::Event`.
- **Custom events bubble.** They are dispatched `{ bubbles: true, composed:
  true }`, so a component nested inside another of the same kind — a
  `wa-split-panel` inside a `wa-split-panel` — delivers its events to the
  outer one's handler too. Compare `target` against `currentTarget` when that
  matters.

Two things about the cascade, both of which cost real debugging:

- **Our stylesheet declares its own layer.** `@layer app { ... }` in
  `styles.css`, which is loaded after `webawesome.css` and so sits after every
  `wa-*` layer. That is what lets `.plain-button` beat `native.css` without
  resorting to specificity. Order *within* the layer still matters: the bare
  target reset comes before the classes built on top of it, or its `padding: 0`
  wins over their padding.
- **The theme does not import the colour variants.** `--wa-color-brand-*` and
  friends only exist inside the components' shadow styles until
  `styles/color/variants.css` is loaded, which `index.html` does. Without it a
  rule using those tokens is silently dropped as invalid.

Because `native.css` styles a native `<button>` like a real button — chrome, a
fixed height, centred flex layout and nowrap text — anything that is a button
only for semantics (a whole list row, a drop target) needs `.plain-button` to
strip all of that back.

The app root carries `wa-cloak`, which holds it hidden until every custom
element inside has been upgraded. Without it there is a flash where each
`<wa-*>` is still an unknown inline element, and dialogs spill their contents
onto the page.

`scripts/fetch-webawesome.sh` vendors the runtime into `frontend/vendor/`
(gitignored) as a Trunk pre-build hook, pinned by version and skipped when
already present. It is fetched rather than committed because it is ~3.6 MB,
all of which is embedded into the `oxyromon` binary.

**Nothing may load from a CDN at runtime** — the UI is served by
`oxyromon server` and bundled into the desktop app, both of which have to work
offline. That is why the script also vendors the sixteen Font Awesome icons
Web Awesome references internally: without them `<wa-icon>` falls back to the
Font Awesome CDN and, for example, a dialog loses its close button.

Dark mode is the `wa-dark` class on `<html>`, set by `set_dark` in `navbar.rs`.
Nothing else is needed: the `--wa-*` tokens carry both light and dark values, so
`styles.css` never mentions a colour scheme.

### Adding a GraphQL Query/Mutation

1. **Backend:** Add the resolver in `query.rs` (for queries) or `mutation.rs` (for mutations) inside the `#[Object]` impl block.
2. **Backend:** If new types are needed, add them to `model.rs` with `#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]`.
3. **Frontend:** Add the query/mutation string in `frontend/src/api.rs`, deserializing into a type in `frontend/src/model.rs`.
4. **Frontend:** Call it from a Leptos component (typically via `spawn_local`, updating `AppState` signals).

## Desktop App (Tauri)

The `desktop/` crate is an optional [Tauri](https://tauri.app) v2 shell that
presents the same web UI in a native window. Like `frontend/`, it is a
standalone crate (`[workspace]` in its own `Cargo.toml`) so its webview
dependencies stay out of the CLI build.

### How it works

It deliberately does **not** reimplement the backend or talk to Tauri IPC:

1. `desktop/src/server.rs` reserves a free loopback port, then spawns the
   regular `oxyromon` binary — bundled as a Tauri **sidecar** — as
   `oxyromon server --address 127.0.0.1 --port <port>`.
2. It polls the port with `TcpStream::connect` until the server accepts
   connections (30s timeout).
3. `desktop/src/main.rs` opens a `WebviewUrl::External` window pointed straight
   at `http://127.0.0.1:<port>`.

Because the window is served **from** the sidecar's origin, the Leptos SPA keeps
using same-origin relative URLs for `/graphql`, `/events`, `/dats` and
`/romfiles/{id}`. This means **no CORS handling, no Tauri-specific frontend
build, and no changes to `server.rs`** — the desktop app is purely additive. The
sidecar's `CommandChild` is held in Tauri managed state and killed on
`RunEvent::Exit` so no orphaned server keeps the database locked.

The desktop app shares the CLI's database and settings (same
`OXYROMON_DATA_DIRECTORY`).

### Layout

| Path                               | Purpose                                                     |
| ---------------------------------- | ----------------------------------------------------------- |
| `desktop/tauri.conf.json`          | Tauri config (sidecar, icons, bundle targets, build hooks)  |
| `desktop/src/main.rs`              | Builder, window creation, exit handling                     |
| `desktop/src/server.rs`            | Sidecar lifecycle: port, spawn, readiness poll, shutdown    |
| `desktop/scripts/stage-sidecar.sh` | Builds `oxyromon` and stages it as `binaries/oxyromon-<triple>` |
| `desktop/capabilities/default.json`| Empty permission set (the SPA never calls Tauri IPC)        |
| `desktop/icons/`                   | Bundle icons generated from `frontend/icon.svg`             |

`binaries/`, `gen/` and `target/` are generated and gitignored.

### Dev

Requires the Tauri CLI and the platform webview packages (Linux:
`webkit2gtk-4.1`, `gtk3`, `librsvg`, `libayatana-appindicator`):

```sh
cargo install --locked tauri-cli --version "^2"

cd desktop
cargo tauri dev     # stages the sidecar, then runs the shell
cargo tauri build   # release build + installers in target/release/bundle/
```

`desktop.sh` in the repository root wraps the release build.

Both commands run `desktop/scripts/stage-sidecar.sh` first (via
`beforeDevCommand`/`beforeBuildCommand`), which builds
`cargo build --release --features server` and copies the binary to
`desktop/binaries/oxyromon-<target-triple>` — the name Tauri's `externalBin`
expects. Note the hook runs from the **repo root**, not `desktop/`.

Because that build enables the `server` feature, the desktop app transitively
needs the web UI toolchain too (Trunk + the `wasm32-unknown-unknown` target).

Plain `cargo build` in `desktop/` also works, but only once the sidecar has been
staged at least once (`tauri-build` fails if `binaries/oxyromon-<triple>` is
missing).

### Troubleshooting

- **Blank window on Linux.** WebKitGTK's DMA-BUF renderer misbehaves on some
  GPU/driver combinations (notably NVIDIA under Wayland). Run with
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` to confirm before chasing it further.
- **"failed to locate the bundled oxyromon binary".** The sidecar has not been
  staged for the current target triple; run `sh desktop/scripts/stage-sidecar.sh`.
- **Server fails to start.** The sidecar's stdout/stderr is forwarded to the
  desktop app's stderr, prefixed with `[oxyromon]`.
- **`failed to run linuxdeploy`.** The AppImage bundler ships its own binutils,
  which is too old to read the `.relr.dyn` sections modern distributions ship,
  so it fails on the first library it tries to strip. Build with
  `NO_STRIP=true`, which is what `desktop.sh` does.

## Error Handling

- The project uses `simple_error::SimpleError` as its main error type.
- The type alias `SimpleResult<T> = Result<T, SimpleError>` is defined in `main.rs` and used everywhere.
- Use `try_with!()` macro (from `simple-error`) for wrapping errors with context.
- Use `bail!()` macro (from `simple-error`) for returning errors with a message string.

## Coding Conventions

### Rust

- **Async everywhere:** All subcommand `main()` functions and most helpers are `async`.
- **Database connection passing:** Functions take `&mut SqliteConnection` (not the pool).
- **Progress bar:** Most functions accept `&ProgressBar` for user feedback. Use the styled `print_*` helpers from `progress.rs` instead of raw `progress_bar.println(...)`.
- **Traits over generics:** ROM format abstractions use trait objects and the trait system in `common.rs`.
- **Module-per-test:** Each test case gets its own file in a subdirectory (e.g., `src/import_roms/test_original.rs`).
- **Feature gating:** All server-related code uses `#[cfg(feature = "server")]`.
- **No unwrap in production code** where avoidable — use `try_with!` or `bail!` instead.
- **Lazy statics** for compiled regexes and shared state (via `std::sync::LazyLock`).
- **Parallel iteration** with `rayon` where beneficial (e.g., filtering large lists).

### CLI Output Helpers

All user-facing output should go through the categorized helpers in `progress.rs`. **Never** call `progress_bar.println(...)` directly from subcommand or format modules — use these instead:

| Helper            | Icon            | Purpose                         | Example                                          |
| ----------------- | --------------- | ------------------------------- | ------------------------------------------------ |
| `print_header`    | `◆` (bold cyan) | Section titles, system names    | `Processing "Nintendo - Game Boy"`               |
| `print_subheader` | `▸` (bold)      | Steps within a section          | `Processing games`, `Summary:`                   |
| `print_info`      | `ℹ` (dim)       | Informational messages          | `System: Test System`, speed results             |
| `print_success`   | `✔` (green)     | Completion, match found         | `Imported Test Game (USA)`, `Matches "rom.bin"`  |
| `print_warning`   | `⚠` (yellow)    | Non-fatal issues                | `No match`, `Multiple matches, skipping`         |
| `print_error`     | `✖` (red bold)  | Errors, failures                | `CRC mismatch`, `Please install chdman`          |
| `print_skip`      | `↪` (dim)       | Skipped/duplicate items         | `Already imported`, `Duplicate of "file.zip"`    |
| `print_action`    | `→` (dim)       | File operations in progress     | `Extracting "game.chd"`, `Compressing "rom.bin"` |
| `print_separator` | (blank line)    | Visual spacing between sections |                                                  |

All helpers require `use super::progress::*;` (or `use crate::progress::*;`) in the module. The `MultiProgress` instance is global (`std::sync::LazyLock`) and all progress bars should be created via `get_progress_bar()` so they're automatically registered with it. The `indicatif-log-bridge` (`LogWrapper`) in `main.rs` ensures that `log::info!` / `log::warn!` etc. don't collide with active progress bars.

### File Organization

- Subcommand modules: `src/<command_name>.rs` with tests in `src/<command_name>/test_*.rs`
- Format modules: `src/<tool_name>.rs` (e.g., `chdman.rs`, `sevenzip.rs`)
- Shared infrastructure: `src/database.rs`, `src/model.rs`, `src/common.rs`, `src/config.rs`, `src/util.rs`

### Frontend (Leptos)

- Rustfmt / clippy, same as the rest of the workspace (run from `frontend/`)
- Reactivity via `RwSignal`/`Effect`; global state is a `Copy` `AppState` provided through context
- Async work via `leptos::task::spawn_local`, updating signals imperatively
- Styling comes from Web Awesome: its components, its layout utilities (`wa-stack`, `wa-cluster`, `wa-grid`, `wa-split`) and its `--wa-*` tokens. Reach for `styles.css` only for what the library does not cover
- **Gotcha:** inside the `view!` macro, wrap any `>`/`<` comparison in parentheses or a block, otherwise it is parsed as a tag delimiter

## External Tool Integration Pattern

When integrating a new external CLI tool:

1. **Define the executable names** as a constant: `const MY_TOOL: &[&str] = &["mytool", "mytool-alt"];`
2. **Use `get_executable_path()`** from `util.rs` to find it in `$PATH`.
3. **Shell out with `tokio::process::Command`**, always logging the command with `log::debug!("{:?}", command)`.
4. **Check `output.status.success()`** and `bail!` on failure with stderr.
5. **Provide `get_version()`** so other modules can check availability before attempting operations.
6. **Always work in temp directories** (via `create_tmp_directory()`) for intermediate files.

## Documentation Requirements

Every new feature or notable change **must** update the following files:

- **`README.md`**: Update the relevant subcommand section with new flags, options, or behavior changes. Keep the usage block in sync with the actual `--help` output.
- **`CHANGELOG.md`**: Add an entry under the current version's `## Features`, `## Changes`, `## Improvements`, or `## Fixes` section as appropriate. If no section for the current version exists yet, create one at the top of the file following the existing format (see `0.21.0` or `0.22.0` for examples). The current version can be found in `Cargo.toml` under `version`.

This applies to all contributions — CLI changes, server changes, bug fixes, and improvements.

## Common Pitfalls

- **Romfile paths are relative** to `ROM_DIRECTORY`. Always use `strip_prefix` when storing and `rom_directory.join()` when reading.
- **The global test `MUTEX`** must be acquired in every test that touches shared state. Forgetting it causes flaky test failures.
- **External tools may not be installed.** Always check `get_version()` before using a tool and provide a helpful message.
- **Cross-filesystem moves fail.** The `rename_file` helper in `util.rs` falls back to copy+delete, but be aware of this.
- **SQLite compile-time checking** requires updating `.sqlx/` data when queries change. Run `cargo sqlx prepare -- --features server`.
- **The `server` feature** changes database connection pool behavior (max connections, locking mode). Always test with `--features server` in CI.
- **Temp directories** should be created via `create_tmp_directory()` which respects the user's `TMP_DIRECTORY` setting.

## TODO / Known Areas for Improvement

From the README:

- Add actions to the web UI (currently read-only except settings and system purge)
- Find a way to automatically download No-Intro DAT files
- Support merged sets for arcade systems
- Craft unit tests for arcade systems, NSZ, IRD/PS3
- Support rebuilding PS3 ISOs using IRD files
- Add a metadata scraper in the RetroArch format
