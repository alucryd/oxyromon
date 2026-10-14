# AGENTS.md — Rules for Working on oxyromon

oxyromon is a Rust CLI ROM organizer: it validates ROM files against Logiqx DAT
files, imports them into a managed directory, sorts them by region and 1G1R
preference, and converts between formats. The `server` feature adds a GraphQL
API, a Leptos web UI and a Tauri desktop shell; the `mcp` feature adds an MCP
server. All three surfaces drive the same subcommand modules and the same
SQLite database.

This file holds the rules. For how the code works, use the generated wiki in
`openwiki/`, starting at `openwiki/quickstart.md`, and the graft graph below.
Both describe the code; neither overrides it, and neither overrides this file.

- **Author:** Maxime Gauduin (alucryd) · **License:** GPL-3.0+
- **Edition:** 2024 · **MSRV:** 1.94.0
- **Workspace:** the `oxyromon` package at the root, plus its format crates in
  `crates/`. `frontend/` and `desktop/` declare their own `[workspace]` so
  their WebAssembly and webview dependencies stay out of the native build.

## Commands

```sh
cargo build --release                         # CLI
cargo build --release --features server       # plus web UI: needs trunk + wasm32-unknown-unknown
SKIP_TRUNK=true cargo build --features server # reuse the web UI already built

cargo test --workspace --features oxyromon/server            # everything
cargo test -p xso-rs                                         # one format crate
cargo test --release -p xso-rs -- --ignored                  # its slow tests
cargo clippy --workspace --all-targets --features oxyromon/server
cargo clippy --no-default-features --features use-rustls,mcp # the stdio-only MCP build

cargo sqlx prepare -- --features server       # after any query change
```

Helper scripts at the root: `build.sh`, `desktop.sh`, `dist.sh`, `docker.sh`,
`test.sh` (the test suite under `cargo llvm-cov`).

CI runs both clippy invocations above, a release build with `--features
server`, and the workspace tests under `cargo llvm-cov`. It installs
`mame-tools`, `ffmpeg` and `xdelta3` only as reference tools for the format
crates' tests, and runs `apt-get update` first: the runner's package lists go
stale within days and installs then 404.

## Invariants

- **Romfile paths are relative** to `ROM_DIRECTORY`. Strip the prefix when
  storing, join it when reading.
- **stdout can be a protocol channel.** With `oxyromon mcp` (or `/mcp`), stdout
  carries MCP JSON-RPC: never `println!`/`print!` from any code path the MCP
  server can reach. Log with `log::*` or use the `progress.rs` helpers, which
  write to stderr.
- **One module per subcommand, one interface.** `subcommand()` declares the
  clap `Command` and does no I/O; `async main(connection, matches,
  progress_bar)` does the work. GraphQL (`mutation.rs`) and MCP (`src/mcp/`)
  reach subcommands by synthesizing argv, so a capability added anywhere else
  is invisible to two of the three surfaces.
- **Synthesized argv must pass the clap parser exactly**, including the case of
  `PossibleValuesParser` values, and the MCP tool descriptions and parameter
  docs must list only values the parser accepts.
- **Format modules do not spawn processes.** Every format is handled by a crate
  in-process; `ctrtool.rs` is the only external tool left. Format modules are
  named after the format (`chd.rs`), or after the tool while one is still run
  (`ctrtool.rs`).
- **Query changes need the offline sqlx data regenerated** in `.sqlx/`.
- **The `server` and `mcp` features change the pool**: 5 connections with
  NORMAL locking instead of 1 with EXCLUSIVE. Test with `--features server`.
- **Temp directories come from `create_tmp_directory()`**, which honours
  `TMP_DIRECTORY`.
- **Cross-filesystem moves** go through `util::rename_file`, which falls back
  to copy and delete.
- **Nothing in the web UI may load from a CDN at runtime.** The server and the
  desktop app must work offline; `frontend/scripts/fetch-webawesome.sh` vendors
  Web Awesome and the Font Awesome icons it references.

## Rust Conventions

- **Errors:** `anyhow`. Functions return `anyhow::Result`; add context with
  `.context(...)`/`.with_context(...)` and fail with `bail!`. The format crates
  are the exception: see their conventions below.
- **No `unwrap()` in production code** where avoidable.
- **Async everywhere**; subcommand `main()`s and most helpers are `async`.
- **Pass `&mut SqliteConnection`**, not the pool. All queries are standalone
  `pub async fn`s in `database.rs` using the checked `sqlx::query!` /
  `query_as!` macros; transactions use `begin_transaction`,
  `commit_transaction` and `rollback_transaction`.
- **Server-only code** is gated with `#[cfg(feature = "server")]`, MCP code
  with `#[cfg(feature = "mcp")]`. `server` implies `mcp`.
- **Lazy statics** use `std::sync::LazyLock`; parallel filtering uses `rayon`.
- **Blocking work** (the format crates) runs through `progress::run_blocking`,
  which also feeds the crate's progress callback to the progress bar.

### Output

All user-facing output goes through the helpers in `progress.rs`. Never call
`progress_bar.println(...)` directly from a subcommand or format module.

| Helper            | Icon            | Purpose                         |
| ----------------- | --------------- | ------------------------------- |
| `print_header`    | `◆` (bold cyan) | Section titles, system names    |
| `print_subheader` | `▸` (bold)      | Steps within a section          |
| `print_info`      | `ℹ` (dim)       | Informational messages          |
| `print_success`   | `✔` (green)     | Completion, match found         |
| `print_warning`   | `⚠` (yellow)    | Non-fatal issues                |
| `print_error`     | `✖` (red bold)  | Errors, failures                |
| `print_skip`      | `↪` (dim)       | Skipped or duplicate items      |
| `print_action`    | `→` (dim)       | File operations in progress     |
| `print_separator` | (blank line)    | Spacing between sections        |

Create progress bars with `get_progress_bar()` so they register with the
global `MultiProgress`.

## Tests

- Each test case is its own file: `src/<module>/test_<case>.rs`, included with
  `#[cfg(test)] mod test_<case>;` at the bottom of the module.
- Tests use a `NamedTempFile` database and `TempDir::new_in("tests")`
  directories, set with `set_rom_directory` / `set_tmp_directory`.
- **Acquire the global `MUTEX`** (`config.rs`) in every test touching shared
  state: `let _guard = MUTEX.lock().await;`. Forgetting it causes flaky failures.
- Subcommand tests run the real pipeline: import a DAT from `tests/`, import
  ROMs, run the subcommand, then assert the database and the filesystem.
- `tests/mcp_stdio.rs` drives the real binary over JSON-RPC; `tests/crates.rs`
  checks the format crates' shared CLI shell.
- HTTP tests use `wiremock`.

## Format Crates

`crates/` holds `chd-rs`, `gdi-rs`, `nsz-rs`, `xdelta-rs`, `xps-rs` and
`xso-rs`, each a port of the tool oxyromon used to run (chdman, gdidrop, nsz,
xdelta3, Flips, maxcso), published on its own and keeping its upstream's
license. Every crate follows these, and a new one must too:

- **Library first, CLI behind a default `cli` feature.** oxyromon depends on a
  crate by `path` and `version` with `default-features = false`.
- **Synchronous, file to file.** Entry points take the input path, the output
  path, their settings, and `progress: &mut dyn FnMut(u64)`, called with the
  input bytes consumed since the last call; the calls add up to the input size.
- **Output goes to `<output>.part`** and is renamed into place once complete,
  so an existing output is only replaced by a complete one, and a failed run
  leaves nothing behind.
- **Errors** are a `thiserror` enum `Error` with a `Result<T>` alias. Shared
  variants keep shared wording: `Io` ("io error: …"), `BadMagic { expected,
  found }`, `Unsupported` ("unsupported format: …"), `Corrupt` ("corrupt data:
  …"), and for the patch crates `WrongSource` ("wrong source: …").
- **CLIs** share one shape and one look: `<cli> [OPTIONS] <INPUT>...`, each
  input converted in the direction its extension implies, `-o DIR` defaulting
  to next to each input, oxyROMon's progress bar, and a `✔`/`✖` line per input,
  exiting non-zero when any failed. An input whose output would land on another
  input, or on an earlier input's output, fails instead. That shell is
  `src/bin/<cli>/ui.rs`, identical in every crate, which `tests/crates.rs`
  checks: change them all together. They use clap's builder API. A flag means
  the same in every tool that has it (`-o DIR`, `-b SIZE` in bytes, and for the
  patch tools `-s FILE`, the source), and short forms are lowercase, for common
  options only.
- **Tests** check against the reference implementation, by known-answer
  vectors or by running the upstream tool, and skip rather than fail when it is
  missing. Slow ones are `#[ignore]`d.
- **Manifests** take `edition`, `rust-version` and `repository` from
  `[workspace.package]` and shared dependencies from
  `[workspace.dependencies]`. They carry no `[profile]`: only the root's
  applies.
- **Releases** are per crate: its own version, a `CHANGELOG.md` in oxyromon's
  format, and tags named `<crate>-<version>`. Publish a crate before an
  oxyromon release that needs its new version. `dist.sh` builds the CLIs
  alongside oxyromon.

## Checklists

**New subcommand.** Create `src/<name>.rs` with `subcommand()` and
`async main(connection, matches, progress_bar) -> Result<()>`. In `main.rs`,
add the `mod`, the `subcommand()` to the list, and the dispatch arm. Add tests.
Expose it to GraphQL or MCP by synthesizing its argv.

**New ROM format.** Write a module whose struct wraps `CommonRomfile` and
implements `Size`, `HashAndSize`, `Check`, plus `As*` and `To*` traits, calling
a crate rather than a process. Add the extension constant and any magic-byte
matcher in `mimetype.rs`. Add branches in `import_roms.rs`, `convert_roms.rs`
and `check_roms.rs`, and report it from `info.rs`.

**New migration.** Add `migrations/YYYYMMDDHHMMSS_description.sql`, update the
row struct in `model.rs` and the queries in `database.rs`, run `cargo sqlx
prepare -- --features server`, then build with `--features server`.

**New setting.** Add the key to its category in `config.rs`: `BOOLEANS`,
`choice_options` (plus its enum), `choice_list_options`, `integer_range`,
`LISTS` or `PATHS`. Add it to `NULLABLES` if it can be unset and to
`SORTED_LISTS` if its order matters. Seed the default in a migration. Read it
with `get_bool`, `get_integer`, `get_list`, `get_string` or `get_directory`.
The GraphQL and MCP settings surfaces reuse these validators.

**New GraphQL query or mutation.** Add the resolver to `query.rs` or
`mutation.rs`, any new type to `model.rs` with
`#[cfg_attr(feature = "server", derive(Clone, SimpleObject))]`, the query
string to `frontend/src/api.rs`, and its type to `frontend/src/model.rs`.

## Web UI Gotchas

The UI is Leptos (CSR) with [Web Awesome](https://webawesome.com) components,
built by Trunk into `target/assets/` and embedded with `rust-embed`. Style it
with Web Awesome's components, layout utilities and `--wa-*` tokens first;
`frontend/styles.css` is for what the library does not cover.

- **Set component state with properties**: `prop:open=...`, not attributes.
- **Custom events need a type annotation**:
  `on:wa-after-hide=move |_: web_sys::Event| ...`. Known events such as
  `on:click` keep their own type and must not be annotated.
- **Custom events bubble and cross component kinds.** A nested `wa-split-panel`
  fires on the outer one's handler, and a `wa-select` fires `wa-after-hide`
  when its listbox closes. Compare `target` with `currentTarget`.
- **`styles.css` declares `@layer app`**, loaded after every `wa-*` layer, which
  is what lets `.plain-button` beat `native.css`. Order within the layer
  matters: the bare-target reset comes before the classes built on it.
- **The theme does not import the colour variants.** `index.html` loads
  `styles/color/variants.css`; without it, rules using `--wa-color-brand-*` are
  dropped as invalid.
- **A `<button>` that is a button only for semantics** (a list row, a drop
  target) needs `.plain-button`.
- **The app root carries `wa-cloak`**, which hides it until every custom element
  is upgraded.
- **Dark mode** is the `wa-dark` class on `<html>`, set by `set_dark` in
  `navbar.rs`; `styles.css` never mentions a colour scheme.
- **In `view!`**, wrap any `<`/`>` comparison in parentheses or a block.

## Desktop Troubleshooting

The Tauri shell in `desktop/` spawns the bundled `oxyromon server` as a sidecar
and points its window at it; it adds no IPC and needs no change to `server.rs`.

- **Blank window on Linux:** try `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
- **"failed to locate the bundled oxyromon binary":** run
  `sh desktop/scripts/stage-sidecar.sh` (from the repo root).
- **`failed to run linuxdeploy`:** build with `NO_STRIP=true`, as `desktop.sh`
  does.
- **Server fails to start:** its output is on the desktop app's stderr,
  prefixed with `[oxyromon]`.

## Documentation

Every new feature or notable change must update:

- **`README.md`**: the relevant command section, keeping its help block in
  sync with the real `--help` output.
- **`CHANGELOG.md`**: an entry under the current version's `## Features`,
  `## Changes`, `## Improvements` or `## Fixes`, creating the version's section
  if needed. The version is in `Cargo.toml`.

`openwiki/` is regenerated by its scheduled workflow; fix the source and docs
rather than hand-editing its pages.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->

<!-- OPENWIKI:START -->

## OpenWiki

This repository has a generated `openwiki/` evidence index. It is optional just-in-time context, not required startup reading.

- Do not enumerate, preload, or search wikis at task start. Use retrieval when the user asks for it, when unfamiliar architecture or dependency behavior materially affects the task, or when source inspection leaves an important uncertainty. Stop once the question is grounded.
- When those conditions apply and OpenWiki retrieval tools are available, use `openwiki_search` for just-in-time context and `openwiki_read` for the relevant complete sections. If search returns `workspace_required`, ask which listed workspace to use and retry with its ID.
- Use `openwiki_list_workspaces` or `openwiki_list_wikis` when workspace membership itself needs to be discovered.
- If the retrieval tools are unavailable, read `openwiki/quickstart.md` and follow its links to the relevant pages.
- Treat source code and tests as authoritative. A brief's unknowns and review items are verification gaps, not automatic requirements.
- Prefer the narrowest quiet validation that proves the changed behavior. Preserve complete failure output.

The scheduled OpenWiki GitHub Actions workflow refreshes the repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.

<!-- OPENWIKI:END -->
