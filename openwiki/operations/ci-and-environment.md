---
type: reference
title: CI Pipelines and Runtime Environment
description: How .github/workflows/continuous_integration.yml gates every change (apt reference tools for the format crates' tests, toolchain, trunk, the two clippy passes, release build and coverage), and exactly which runtime environment variables the binary reads, when, and for how long they stay authoritative.
tags: [ci, github-actions, clippy, coverage, environment-variables, trunk, sqlx, toolchain]
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-1bc83e59b3e3c64091f68169
    resource: repo://.github/workflows/docker.yml
  - id: openwiki-source-ea70eb6c045047448e446296
    resource: repo://.gitignore
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-36f054f1f38f2197a6af5bca
    resource: repo://build.rs
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-38b8a6db58a8dd7d870046a0
    resource: repo://crates/chd-rs/tests/ld.rs
  - id: openwiki-source-dace695e1efd12a88e32dd04
    resource: repo://crates/chd-rs/tests/write.rs
  - id: openwiki-source-7c41df92324ca8ddc11ad14b
    resource: repo://crates/xdelta-rs/tests/interop.rs
  - id: openwiki-source-fc949ff54825cd4750b9e1d5
    resource: repo://crates/xps-rs/tests/interop.rs
  - id: openwiki-source-0c6adbcf77da61d03e900821
    resource: repo://crates/xso-rs/tests/common/mod.rs
  - id: openwiki-source-33ab0e331223101757a2c45d
    resource: repo://docker.sh
  - id: openwiki-source-819b62175bb3c42d5bb268ab
    resource: repo://Dockerfile.alpine
  - id: openwiki-source-74b8949e6d5862f8f52e4791
    resource: repo://Dockerfile.archlinux
  - id: openwiki-source-182eb6366058c61b0d4d15e9
    resource: repo://frontend/Cargo.toml
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-c3dbd15c40e250cdf06fed8d
    resource: repo://src/chd.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-63f87054c5f9055be025dd77
    resource: repo://src/ctrtool.rs
  - id: openwiki-source-b94e49e3da43d66b03ed7c7e
    resource: repo://src/database.rs
  - id: openwiki-source-b1a773ad2966952330a5f92d
    resource: repo://src/import_roms/test_cia.rs
  - id: openwiki-source-b55a21a31ede1b56cd31a6a6
    resource: repo://src/main.rs
  - id: openwiki-source-b95ec25c5cfc0be350df258c
    resource: repo://src/mcp.rs
  - id: openwiki-source-6330834d320d84b814c24c5f
    resource: repo://src/progress.rs
  - id: openwiki-source-db13c01516e9bd04294e339d
    resource: repo://src/rvz.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-1c38b81542bfb683299ad2cd
    resource: repo://src/xdelta.rs
  - id: openwiki-source-9b5bca3734529131a5ec0d1b
    resource: repo://src/xps.rs
  - id: openwiki-source-196778315263fb8bcc8066e5
    resource: repo://src/xso.rs
  - id: openwiki-source-9a02c1f55f47430403921057
    resource: repo://tests/mcp_stdio.rs
generated: { by: "claude-code", at: "2026-10-14T21:01:38.051Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-14T21:01:38.051Z
---

# CI Pipelines and Runtime Environment

Two workflows live in `.github/workflows/`, and each guards a different thing:
`continuous_integration.yml` is the change gate for the code and the feature
matrix, `docker.yml` publishes a container image. Neither is a release workflow —
nothing here runs `dist.sh`, `docker.sh`, `desktop.sh` or `cargo publish` (see
[Builds, Distribution and Releases](build-and-release.md)).

The runtime side is small and worth pinning down exactly: six variables read at
three sites (`src/main.rs`, `src/config.rs`, `build.rs`) plus one compile-time
consumer (`sqlx`), and they do not share a lifetime. `OXYROMON_DATA_DIRECTORY`
and `OXYROMON_LOG_LEVEL` are honored on every run; `OXYROMON_ROM_DIRECTORY` and
`OXYROMON_TMP_DIRECTORY` are consulted only until a settings row exists;
`SKIP_TRUNK` and `DATABASE_URL` never reach the running binary at all. A separate
category, `CHDMAN`/`FFMPEG`/`MAXCSO`, exists only inside the format crates' test
harnesses.

## Workflow trigger matrix

| Workflow | Trigger | Runs on | What it produces |
| --- | --- | --- | --- |
| `continuous_integration.yml` | `push` and `pull_request` to `master` and `develop` | `ubuntu-26.04` | clippy verdicts, a release `server` binary, `lcov.info` uploaded to Codecov |
| `docker.yml` | `push` to `develop` | `ubuntu-latest` | `alucryd/oxyromon` image pushed to Docker Hub, tags from the git ref |

Both workflows fire on a push to `develop`, but nothing orders them: the image
publish derives its tags from the git ref via `docker/metadata-action` and does
not depend on the CI job, so a `develop` push publishes a container image while
the clippy/build/coverage verdict on that same commit is still running — and
even after it fails. `master` is gated by CI but publishes nothing.

This page itself is produced by the OpenWiki generator rather than by a workflow
file: `AGENTS.md` documents "the scheduled OpenWiki GitHub Actions workflow" that
refreshes `openwiki/`, but no such workflow exists in `.github/workflows/` in the
current tree, so the regeneration cadence is not something CI here defines or
gates.

## The CI job, step by step

The job is a single `build` job with no matrix; the feature matrix is expressed
as separate cargo invocations instead. Job-level `env` sets `CARGO_TERM_COLOR`,
`SKIP_TRUNK: "true"` and `TRUNK_VERSION: "0.21.14"`.

```mermaid
flowchart TD
  CO["checkout v7"] --> APT["apt-get update with Acquire Retries 3, then install mame-tools xdelta3 ffmpeg"]
  APT --> TC["rust-toolchain stable plus wasm32-unknown-unknown, clippy, llvm-tools-preview"]
  TC --> DEF["rustup default stable"]
  DEF --> CACHE["Swatinem/rust-cache v2"]
  CACHE --> TRUNK["install trunk from the pinned TRUNK_VERSION tarball into cargo bin"]
  TRUNK --> UI["cd frontend, trunk build --release (the one and only wasm build)"]
  UI --> COV["cargo install cargo-llvm-cov"]
  COV --> L1["clippy workspace all-targets features oxyromon/server"]
  L1 --> L2["clippy no-default-features features use-rustls,mcp all-targets"]
  L2 --> B["cargo build --release --features server"]
  B --> T["cargo llvm-cov workspace features oxyromon/server, lcov output"]
  T --> U["codecov-action v7 with CODECOV_TOKEN"]
```

*Caption: the CI job as a single linear pipeline — one wasm build, two clippy passes, one release build, one coverage run.*

Why each step exists:

- **`apt-get update` before any install, with `-o Acquire::Retries=3`.** The
  runner image bakes its apt lists at image-build time and they go stale within
  days; Ubuntu rolls superseded `.deb`s out of the pool when a newer point
  version lands, so installing against the cached lists 404s. The retries cover
  transient mirror failures on the refresh itself, not just the install.
- **`mame-tools` (which provides `chdman`), `xdelta3` and `ffmpeg` are installed
  purely as reference implementations for the format crates' tests**, as the
  workflow's own comment says: the chd-rs suites compare `chdrs` against
  `chdman`, the xdelta-rs suite compares against `xdelta3`, and `ffmpeg` makes
  the AVI fixtures the LaserDisc tests feed to both tools. Every one of these
  suites **skips rather than fails when its reference binary is missing** —
  `crates/xdelta-rs/tests/interop.rs` returns early when `xdelta3 -V` fails, and
  `crates/chd-rs/tests/ld.rs` skips unless it finds *both* `chdman` and
  `ffmpeg`. CI installs all three so those interop paths actually execute
  instead of silently reporting a green run.
- **These are not runtime dependencies.** The oxyromon binary shells out to no
  format tool any more: `get_version()` in `src/chd.rs`, `src/xdelta.rs`,
  `src/xps.rs`, `src/xso.rs`, `src/archive.rs`, `src/nsz.rs` and `src/rvz.rs`
  returns `"built-in"`, and the work goes through the workspace crates.
  `src/ctrtool.rs` is the one remaining module that runs an external tool, and
  `ctrtool` is deliberately *not* installed, so CIA import tests skip in CI.
  Neither `maxcso` nor `flips` is installed either, so the `xso-rs` and `xps-rs`
  interop suites also skip there — those two are a local responsibility
  (`$MAXCSO`, `flips` on `$PATH`).
- **`wasm32-unknown-unknown` on the toolchain** is needed by the "Build web UI"
  step: `frontend/` is a standalone crate with its own `[workspace]`, so no
  `cargo` invocation in the job compiles the Leptos app — only `trunk` does, and
  it needs the wasm target installed. **`llvm-tools-preview`** is required by
  `cargo-llvm-cov`. `rustup default stable` makes the freshly installed toolchain
  the one every later `cargo` invocation resolves to.
- **Toolchain pinning is asymmetric.** There is no `rust-toolchain.toml`
  anywhere in the tree, so `dtolnay/rust-toolchain@stable` installs whatever
  stable is current on the day the workflow runs; the only MSRV anchor in the
  manifests is `rust-version = "1.94.0"` in `Cargo.toml`, shared with the format
  crates through `[workspace.package]` and repeated in `AGENTS.md`, and nothing
  in CI enforces it). Trunk is pinned to a specific
  release by `TRUNK_VERSION` — the same `0.21.14` both Dockerfiles download — so
  the bundled UI is reproducible across CI and the published images without
  paying for a source build of trunk, while `cargo-llvm-cov` is installed
  unpinned from crates.io on every run. In practice a new stable release can
  break CI with no repository change.
- **A dedicated "Build web UI" step** (`cd frontend && trunk build --release`)
  is the whole reason the workflow sets `SKIP_TRUNK: "true"` at job level. The
  wasm bundle is target- and feature-independent, so it is compiled once into
  `target/assets` (`frontend/Trunk.toml` sets `dist = "../target/assets"`, where
  the `rust-embed` derive in `src/server.rs` looks for it) and every later cargo
  invocation — two clippy runs, a release build, and a workspace test run that
  rebuilds the `server` feature many times over — reuses those assets instead of
  recompiling wasm. `build.rs` honors the variable only when `server` is enabled,
  and `SKIP_TRUNK` is **not** declared with `cargo:rerun-if-env-changed`, so
  flipping it does not itself re-run the script. Because the trunk step runs
  before any cargo invocation, its `pre_build` hook — which fetches Web Awesome
  into the gitignored `frontend/vendor/` — executes exactly once per job instead
  of once per `build.rs` run.
- **Two clippy passes** — see the next section.
- **`cargo build --release --features server`** builds the artifact users
  actually get (server implies `mcp`, and the release profile uses
  `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, which ordinary debug
  builds never exercise).
- **`cargo llvm-cov --workspace --features oxyromon/server --lcov --output-path
  lcov.info`** runs the tests, not merely a coverage report of compilation: the
  whole workspace under the fullest feature set. `lcov.info` is gitignored; the
  `codecov/codecov-action@v7` step uploads it with `secrets.CODECOV_TOKEN`.

```sh
# the two commands CI treats as the compile gate, verbatim
cargo clippy --workspace --all-targets --features oxyromon/server
cargo clippy --no-default-features --features use-rustls,mcp --all-targets
```

## Feature-combination coverage, and what a change must keep compiling

The `--features oxyromon/server` name is package-qualified because
`--workspace` selects several packages, and cargo requires the qualifier once
more than one package is in scope.

The second pass is the interesting one. `server` implies `mcp`, so the first
pass can never catch a change that breaks the **mcp-without-server** build —
and that combination is a shipped artifact (`oxyromon mcp` over stdio, no web
server, no embedded UI). The dedicated
`--no-default-features --features use-rustls,mcp --all-targets` run exists
precisely so a refactor of `server`-gated code cannot silently rot it. Two
consequences for a change plan:

- Anything in `src/mcp/`, or any `#[cfg(feature = "mcp")]` region shared with
  server code, must compile with **no** `server` cfg active. Server-only items
  (`SseMessage`, the `sse_tx` field, `with_sse`, `mcp_http_service`, the
  `cfg_attr(feature = "server")` GraphQL derives in `model.rs`) are gated so the
  lean build never sees them; introducing a use of one outside a
  `#[cfg(feature = "server")]` boundary fails this pass.
- `--no-default-features` also drops `use-rustls` (the cargo default), so the
  pass names `use-rustls` explicitly and doubles as the documented
  non-default-features build path.

Note what the passes do and do not *run*: clippy `--all-targets` lints test
targets too but does not execute them, and the second pass has `server` off, so
the mcp-only combination is **linted, never executed**. Runtime coverage of the
stdio server comes from the `llvm-cov` run: `oxyromon/server` implies `mcp`, so
`tests/mcp_stdio.rs` (which opens with `#![cfg(feature = "mcp")]` and drives the
real binary over stdin/stdout with raw JSON-RPC) compiles and runs there.

A build with **neither** `mcp` nor `server` changes more than the subcommand
list: `src/database.rs` switches that case to `max_connections = 1` with
`PRAGMA locking_mode = EXCLUSIVE`, while either feature selects
`max_connections = 5` with `NORMAL`. Since the first clippy pass always turns
`server` on and the second always turns `mcp` on, **CI never compiles the
single-connection/EXCLUSIVE configuration**, and a `cfg_if` arm mistake on that
branch surfaces only for downstream packagers and `cargo build` without features.
Likewise `use-native-tls` and every Windows/macOS target are never compiled in CI
at all — the OpenSSL and cross-compiler paths are guarded only by `dist.sh` and
by packagers. Neither
clippy invocation passes `-D warnings`, and the manifests declare no `[lints]`
section, so this gate fails on hard errors in a feature combination, not on new
lint warnings. There is also no `cargo fmt` step: formatting is a convention,
not a CI check.

## Runtime environment variables

| Variable | Read at | Effect | Default |
| --- | --- | --- | --- |
| `OXYROMON_DATA_DIRECTORY` | `src/main.rs`, every run | Directory that holds `oxyromon.db`; created if missing | `dirs::data_dir()/oxyromon` |
| `OXYROMON_ROM_DIRECTORY` | `config::get_rom_directory` | Seeds `ROM_DIRECTORY` **only when no setting row exists**, then persisted | `dirs::home_dir()/Emulation` |
| `OXYROMON_TMP_DIRECTORY` | `config::get_tmp_directory` | Seeds `TMP_DIRECTORY` under the same first-run rule | `std::env::temp_dir()` |
| `OXYROMON_LOG_LEVEL` | `src/main.rs`, logger construction | `env_logger` filter (`off`, `debug`, `module=trace`, …) | unset, i.e. env_logger's own default filter |
| `SKIP_TRUNK` | `build.rs` only | Set to exactly `true` to skip `trunk build --release` for `server` builds | unset, i.e. build the UI |
| `DATABASE_URL` | `sqlx` macro expansion, compile time | Query verification when `.sqlx/` offline data is missing or stale | unset (relies on committed `.sqlx/`) |
| `CHDMAN` / `FFMPEG` / `MAXCSO` | format-crate test harnesses only | Absolute path to the reference binary the tests compare against | fall back to `PATH`; the test skips when neither yields a file |

The reference-tool variables never reach `src/`: each format crate's tests look
up their binary themselves, preferring the environment variable and otherwise
scanning `$PATH` for `chdman`/`ffmpeg`/`maxcso`, and print `skipping: …` when
nothing is found. That is the extension point for running the interop suites on
a machine without the distro packages: point `CHDMAN`, `FFMPEG` or `MAXCSO` at
locally built binaries. (`xdelta3` and `flips` are the exception — their suites
probe the bare command name on `$PATH` and have no override variable.)

Two lifetime distinctions matter more than the paths themselves:

**`OXYROMON_DATA_DIRECTORY` is re-read every run and never persisted.** It
selects the SQLite file location, and `main.rs` creates the directory
(`create_dir_all`, quiet), creates `oxyromon.db` if absent, and opens the pool —
which runs the embedded `sqlx::migrate!()` migrator — before any subcommand
runs. A failing migration therefore breaks every subcommand at startup, not just
migrations. The Docker images exploit this: both set
`OXYROMON_DATA_DIRECTORY=/data` and `OXYROMON_ROM_DIRECTORY=/roms` and declare
`VOLUME [ "/data", "/roms" ]`, so a container ignores the host's platform data
directory entirely.

**`OXYROMON_ROM_DIRECTORY` and `OXYROMON_TMP_DIRECTORY` are bootstrap-only.**
`main.rs` calls `get_rom_directory` and `get_tmp_directory` at startup purely for
the side effect of writing the first-run value into the `settings` table; on any
later run the DB row wins, so changing the environment variable has no effect on
an existing database. Use `oxyromon config --set ROM_DIRECTORY …` to move a
library, not the env var. Both are `PATHS` settings, and a stored path is
canonicalized on read — `get_directory` runs it through `canonicalize()`, so a
deleted or unmountable configured directory degrades to `None` and re-triggers
the bootstrap.

`OXYROMON_LOG_LEVEL` follows standard `env_logger` syntax and is consumed by
`Builder::from_env("OXYROMON_LOG_LEVEL")`; the resulting filter is then pushed
into `log::set_max_level` and the logger is wrapped in `indicatif_log_bridge`'s
`LogWrapper` over the process-wide `MultiProgress`, so log lines interleave with
progress bars instead of tearing them apart. This initialization lives *inside*
`if let Some((subcommand, …)) = matches.subcommand()`, together with
`dotenv().ok()`: with no subcommand (the container's bare `CMD ["oxyromon"]`) no
`.env` is loaded, no logger is installed, no database is opened, and the process
exits successfully. `dotenvy` reads a `.env` in the working directory before any
of the `OXYROMON_*` or `DATABASE_URL` lookups, and `.env` is gitignored.

```mermaid
sequenceDiagram
  participant CLI as oxyromon argv
  participant Main as src main
  participant Env as process env
  participant DB as SQLite in data dir
  CLI->>Main: parsed subcommand
  Main->>Env: dotenv then read OXYROMON_LOG_LEVEL
  Main->>Env: read OXYROMON_DATA_DIRECTORY
  Env-->>Main: data dir or platform default
  Main->>DB: create dir, create oxyromon.db, open pool, run migrations
  Main->>DB: get_rom_directory and get_tmp_directory
  DB-->>Main: stored settings, else env value persisted now
  Main->>DB: subcommand on one pooled connection, then optimize_database
```

*Caption: when each variable is consulted during startup, and which answers are persisted.*

Because both `OXYROMON_DATA_DIRECTORY` and `OXYROMON_LOG_LEVEL` are ordinary
process environment, a harness can point the CLI at a throwaway library and
still assert on the wire format. `tests/mcp_stdio.rs` does exactly that: it
spawns the built binary with `OXYROMON_DATA_DIRECTORY` set to a `TempDir` and
`OXYROMON_LOG_LEVEL=off`, so every migration and every catalog write lands in
the temp directory and nothing pollutes stderr or the developer's real database.

## sqlx, `.sqlx/` offline data, and `DATABASE_URL`

`Cargo.toml` declares `sqlx` with `default-features = false` and
`features = ["macros", "migrate", "runtime-tokio", "sqlite"]`, and the code uses
the `query!` macros, which are verified **at compile time**. Verification needs
either a live `DATABASE_URL` or the checked-in offline cache in `.sqlx/` (one
JSON file per query hash). CI sets no `DATABASE_URL` and no `SQLX_OFFLINE`
variable — with the variable absent, cargo finds the committed `.sqlx/` and the
macros check against those snapshots — as do the Docker builds and the cross
containers. That makes `cargo sqlx prepare -- --features server` part of any
change to `migrations/` or to a query: an unprepared query change does not merely
break the author's machine, it breaks the clippy passes, the release build and
every container build, because none of them has a database to introspect. The
root's `sqlx-data.json` (`{"db": "SQLite"}`) is a leftover of sqlx's older
single-file offline format and is no longer read.

## Windows and TLS build notes

No CI job builds a Windows or macOS target; those are produced only by
`dist.sh`, which ships `x86_64-pc-windows-gnullvm` against `llvm-mingw` rather
than MSVC. The system-OpenSSL alternative,
`--no-default-features --features use-native-tls`, is documented in the README
but never compiled in CI, so an OpenSSL-only break is invisible until someone
builds it. Nothing enforces that exactly one TLS backend is chosen; the flags
are additive and picking both is the operator's error to make.

## Docker publish workflow

`docker.yml` triggers on `push` to `develop`, logs in with
`secrets.DOCKER_USERNAME`/`DOCKER_PASSWORD`, derives tags and labels with
`docker/metadata-action@v6` for image `alucryd/oxyromon`, and builds **only
`./Dockerfile.archlinux`** via `docker/build-push-action@v7`. So automated
pushes never produce the version-pinned `<version>` tag and never rebuild the
Alpine image — those four tags exist only when someone runs `docker.sh` by hand.

The images themselves are self-contained build-and-run images: they install a
Rust toolchain, the wasm target and trunk 0.21.14, run
`cargo install --features server --root /usr --path .`, then delete the
toolchain, caches and source tree. Neither image sets `SKIP_TRUNK`, so unlike CI
they compile the wasm UI inside the image; Alpine adds `gcompat` because trunk
fetches glibc-linked `wasm-bindgen`/`wasm-opt` binaries that must run under musl,
and Arch disables pacman signature checking to keep the build scripted. Because
`CMD ["oxyromon"]` carries no subcommand, `docker run alucryd/oxyromon` alone is
a no-op — append `server` (or any other subcommand).

## Practical checklist

- Touching a query or a migration? Run
  `cargo sqlx prepare -- --features server` and commit `.sqlx/`, or CI and every
  image build fails.
- Touching `src/mcp/` or code shared with the server? Verify
  `cargo clippy --no-default-features --features use-rustls,mcp --all-targets`.
- Touching `src/database.rs` pool setup or any `cfg_if` around `server`/`mcp`?
  Also compile the bare configuration with `cargo clippy --no-default-features
  --all-targets`; CI never builds that branch.
- Touching a format crate's interop suite? Install `mame-tools`, `xdelta3` and
  `ffmpeg` (or point `CHDMAN`/`FFMPEG`/`MAXCSO` at local builds) before trusting
  a green run — without the reference binary these tests skip and say so only on
  stderr.
- Touching the frontend? CI rebuilds it once into `target/assets`; locally
  `cd frontend && trunk build --release` refreshes assets under `SKIP_TRUNK=true`.
- Writing to stdout from a code path the MCP server can reach? Don't — stdout is
  the JSON-RPC channel; use `log::*` or the `progress.rs` helpers.
- Debugging a container? Shell in with a subcommand
  (`docker run --rm -it alucryd/oxyromon server`) and check `/data` and `/roms`,
  which the image fixes by environment rather than by the host's platform paths.

See also [Feature Flags and Build Topology](../architecture/feature-flags-and-build-topology.md),
[Builds, Distribution and Releases](build-and-release.md),
[Settings and Configuration](../architecture/settings-and-configuration.md),
[Testing Conventions](../testing/testing-conventions.md), and
[External Tools](../integrations/external-tools.md).
