# AGENTS.md — Guide for AI Agents Working on oxyromon

## Project Overview

**oxyromon** (oxyROMon) is a cross-platform opinionated CLI ROM organizer written in Rust. It validates ROM files against known-good databases (DAT files), imports them into a managed directory structure, sorts them by region/preference, and can convert between various ROM formats. It also includes an optional web UI (Svelte + GraphQL) behind the `server` feature flag.

- **Author:** Maxime Gauduin (alucryd)
- **License:** GPL-3.0+
- **Rust Edition:** 2024
- **MSRV:** 1.88.0
- **Repository:** https://github.com/alucryd/oxyromon

## Architecture

### High-Level Design

oxyromon is a CLI application built with `clap` for argument parsing, `sqlx` with SQLite for persistence, and `tokio` as the async runtime. Each CLI subcommand is implemented as its own module following a consistent pattern. An optional web server (GraphQL API + Svelte SPA) is gated behind the `server` Cargo feature.

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
│    + Svelte SPA (src/routes/, src/components/)      │
├─────────────────────────────────────────────────────┤
│  SQLite (via sqlx) + migrations/                    │
└─────────────────────────────────────────────────────┘
```

### Key Modules

| Module | Purpose |
|--------|---------|
| `main.rs` | Entry point. Registers all subcommands, establishes DB connection, dispatches to subcommand `main()` functions. |
| `model.rs` | All data structures: DB row types (`System`, `Game`, `Rom`, `Romfile`, `Patch`, `Setting`), XML deserialization types (`DatfileXml`, `GameXml`, `RomXml`, etc.), and enums (`Merging`, `Sorting`, `Completion`, `RomfileType`). |
| `database.rs` | All database operations as standalone async functions. Uses `sqlx` with compile-time checked queries. Contains the `MIGRATOR` for schema migrations. |
| `common.rs` | The `CommonRomfile` struct and a rich trait system (`FromPath`, `CommonFile`, `AsCommon`, `ToCommon`, `AsIso`, `AsCueBin`, `ToIso`, `ToCueBin`, `Patch`, `Playlist`, `Size`, `HashAndSize`, `HeaderedHashAndSize`, `Check`, `Persist`) that abstracts over different ROM file formats. |
| `config.rs` | Settings management. Defines setting types (booleans, integers, paths, lists, choices), and provides get/set helpers. Also defines `HashAlgorithm`, `SubfolderScheme`, `PreferredVersion`, `PreferredRegion`, `ArcadeRomType`. |
| `util.rs` | Low-level filesystem operations (`create_file`, `copy_file`, `rename_file`, `remove_file`, `create_directory`, etc.) and directory structure helpers (`get_system_directory`, `get_one_region_directory`, `get_trash_directory`). Also contains `compute_system_completion` and `compute_alpha_subfolder`. |
| `mimetype.rs` | File type detection using the `infer` crate with custom matchers for ROM-specific formats (CHD, CSO, RVZ, IRD, BPS, IPS, XDELTA, ZSO, RDSK, RIFF). Defines all file extension constants. |
| `progress.rs` | Thin wrappers around `indicatif` for progress bar styles. |
| `import_dats.rs` | Parses Logiqx-format DAT files, creates/updates systems, games, and ROMs in the database. |
| `import_roms.rs` | Imports ROM files by hashing them and matching against the database. Handles archives, CHDs, CIAs, CSOs, RVZs, NSZs, ZSOs, and plain files. |
| `sort_roms.rs` | Sorts imported ROMs into region folders and 1G1R directories based on user configuration. Implements the weighted election algorithm. |
| `convert_roms.rs` | Converts between ROM formats (archive ↔ original, ISO ↔ CHD ↔ CSO/ZSO, ISO ↔ RVZ, etc.). |
| `check_roms.rs` | Verifies ROM integrity by re-hashing and comparing against database records. |
| `rebuild_roms.rs` | Rebuilds arcade ROM sets between merging strategies (split, non-merged, full non-merged). |
| `export_roms.rs` | Exports ROMs to various formats without modifying the originals. |
| `sevenzip.rs` | 7z/ZIP archive abstraction. `ArchiveRomfile` struct with `AsArchive`, `ToArchive` traits. Shells out to the `7zz`/`7z` executable. |
| `chdman.rs` | CHD format abstraction. `ChdRomfile` struct with `AsChd`, `ToChd` traits. Shells out to `chdman`. |
| `server.rs` | Axum-based web server with GraphQL (async-graphql), SSE for real-time updates, and embedded static assets from the Svelte build. |
| `query.rs` | GraphQL query resolvers. Uses DataLoader pattern for N+1 prevention. |
| `mutation.rs` | GraphQL mutation resolvers for settings and system management. |
| `validator.rs` | GraphQL input validators. |

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

| Table | Key Columns | Purpose |
|-------|-------------|---------|
| `systems` | id, name, custom_name, custom_extension, description, version, url, completion, arcade, merging | Game system definitions from DAT files |
| `games` | id, name, description, comment, external_id, device, bios, jbfolder, regions, sorting, completion, system_id, parent_id, bios_id, playlist_id | Individual game entries |
| `roms` | id, name, bios, disk, size, crc, md5, sha1, rom_status, game_id, romfile_id, parent_id, original | ROM file definitions (expected files) |
| `romfiles` | id, path, size, parent_id, romfile_type | Actual files on disk (paths are relative to ROM_DIRECTORY) |
| `patches` | id, name, index, rom_id, romfile_id | Patch files (BPS, IPS, XDELTA) |
| `headers` | id, name, version, size, system_id | ROM header definitions for headered systems |
| `rules` | id, start_byte, hex_value, header_id | Header detection rules |
| `settings` | id, key, value | Key-value configuration store |

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
- For the `server` feature: Node.js (see `.nvmrc`) + pnpm

### CLI Only

```sh
cargo build --release
```

### With Web UI

```sh
cargo build --release --features server
```

The `build.rs` script automatically runs `pnpm install` and `pnpm build` when the `server` feature is enabled (skip with `SKIP_PNPM=true`). The Svelte app is built to `target/assets/` and embedded into the binary via `rust-embed`.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `OXYROMON_DATA_DIRECTORY` | Override the data directory (default: platform data dir + `/oxyromon`) |
| `OXYROMON_ROM_DIRECTORY` | Override the default ROM directory |
| `OXYROMON_TMP_DIRECTORY` | Override the default temp directory |
| `OXYROMON_LOG_LEVEL` | Control log verbosity (standard `env_logger` syntax) |
| `SKIP_PNPM` | Set to `true` to skip frontend build in `build.rs` |
| `DATABASE_URL` | Used by `sqlx` for compile-time query checking |

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | Builds the web server subcommand (GraphQL + Svelte SPA) | Off |
| `enable-asm` | ASM variants of MD5 and SHA1 hashes | On |
| `use-rustls` | Use rustls for TLS | On |
| `use-native-tls` | Use system OpenSSL for TLS | Off |

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
- **Frontend:** SvelteKit (static adapter) + Tailwind CSS 4 + Flowbite Svelte
- **Build:** Vite, output to `target/assets/`, embedded via `rust-embed`
- **GraphQL:** Single `/graphql` endpoint, schema defined in `query.rs` (queries) and `mutation.rs` (mutations)
- **SSE:** `/events` endpoint for real-time updates (e.g., purge progress)

### Frontend Structure

| Path | Purpose |
|------|---------|
| `src/routes/+layout.svelte` | Root layout |
| `src/routes/+page.svelte` | Main page |
| `src/components/` | Reusable Svelte components |
| `src/query.js` | GraphQL query definitions |
| `src/mutation.js` | GraphQL mutation definitions |
| `src/store.js` | Svelte stores |
| `src/events.js` | SSE client helpers |
| `src/app.css` | Global styles (Tailwind) |
| `src/app.html` | HTML template |

### Frontend Dev

```sh
# Install dependencies
pnpm install

# Dev server (proxies API to running oxyromon server)
pnpm dev

# Build for production (outputs to target/assets/)
pnpm build

# Lint & format
pnpm lint
pnpm format
```

### Adding a GraphQL Query/Mutation

1. **Backend:** Add the resolver in `query.rs` (for queries) or `mutation.rs` (for mutations) inside the `#[Object]` impl block.
2. **Backend:** If new types are needed, add them to `model.rs` with `#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]`.
3. **Frontend:** Add the query/mutation string in `src/query.js` or `src/mutation.js`.
4. **Frontend:** Call it from Svelte components using `graphql-request`.

## Error Handling

- The project uses `simple_error::SimpleError` as its main error type.
- The type alias `SimpleResult<T> = Result<T, SimpleError>` is defined in `main.rs` and used everywhere.
- Use `try_with!()` macro (from `simple-error`) for wrapping errors with context.
- Use `bail!()` macro (from `simple-error`) for returning errors with a message string.

## Coding Conventions

### Rust

- **Async everywhere:** All subcommand `main()` functions and most helpers are `async`.
- **Database connection passing:** Functions take `&mut SqliteConnection` (not the pool).
- **Progress bar:** Most functions accept `&ProgressBar` for user feedback.
- **Traits over generics:** ROM format abstractions use trait objects and the trait system in `common.rs`.
- **Module-per-test:** Each test case gets its own file in a subdirectory (e.g., `src/import_roms/test_original.rs`).
- **Feature gating:** All server-related code uses `#[cfg(feature = "server")]`.
- **No unwrap in production code** where avoidable — use `try_with!` or `bail!` instead.
- **Lazy statics** for compiled regexes and shared state (via `lazy_static!`).
- **Parallel iteration** with `rayon` where beneficial (e.g., filtering large lists).

### File Organization

- Subcommand modules: `src/<command_name>.rs` with tests in `src/<command_name>/test_*.rs`
- Format modules: `src/<tool_name>.rs` (e.g., `chdman.rs`, `sevenzip.rs`)
- Shared infrastructure: `src/database.rs`, `src/model.rs`, `src/common.rs`, `src/config.rs`, `src/util.rs`

### Frontend (Svelte)

- Prettier for formatting (config in `package.json`)
- ESLint for linting
- Print width: 120, semicolons, double quotes, ES5 trailing commas
- Tailwind CSS 4 for styling
- Flowbite Svelte for UI components

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
