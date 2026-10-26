---
type: "Reference"
title: "External Tools"
description: "oxyromon runs no external program at runtime: every tool it once shelled out to (chdman, ctrtool, maxcso, nsz, gdidrop, Flips, xdelta3, 7-Zip, dolphin-tool, wit) is replaced by an in-tree crate or a Rust dependency. Reference tools survive only in the format crates' tests, which skip without them."
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-38b8a6db58a8dd7d870046a0
    resource: repo://crates/chd-rs/tests/ld.rs
  - id: openwiki-source-dace695e1efd12a88e32dd04
    resource: repo://crates/chd-rs/tests/write.rs
  - id: openwiki-source-039ef28c41d4fc7a1b163edc
    resource: repo://crates/cia-rs/tests/common/mod.rs
  - id: openwiki-source-1877f90a07c967103e553a55
    resource: repo://crates/wux-rs/README.md
  - id: openwiki-source-7c41df92324ca8ddc11ad14b
    resource: repo://crates/xdelta-rs/tests/interop.rs
  - id: openwiki-source-0c6adbcf77da61d03e900821
    resource: repo://crates/xso-rs/tests/common/mod.rs
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-db15f8427180aeec55b3a076
    resource: repo://src/cia.rs
  - id: openwiki-source-2a737474d86fc75cc9d9694f
    resource: repo://src/config.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-b1a773ad2966952330a5f92d
    resource: repo://src/import_roms/test_cia.rs
  - id: openwiki-source-7fb657fead83b71b9836ddac
    resource: repo://src/info.rs
  - id: openwiki-source-f6b27667092492e425164ff8
    resource: repo://src/mcp/read.rs
  - id: openwiki-source-d3402490d16a663b822e8a00
    resource: repo://src/nsz.rs
  - id: openwiki-source-f314ed3b08dd849d591fdb0f
    resource: repo://src/query.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-a849c4f3cbc8a6dbd38563a0
    resource: repo://src/wux.rs
generated: { by: "claude-code", at: "2026-10-25T14:55:31.619Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T20:09:06.498Z
---


# External Tools

oxyromon needs no external program at runtime. The README says so directly:
every format is handled in-process ([README.md](../../README.md#L20)). The last
subprocess, `ctrtool` for CIA, was replaced by the `cia-rs` crate, and with it
went `src/ctrtool.rs`, the `util::tool_version` probe and the
`Required tool not found` path in `import-roms`. AGENTS.md now states the rule
outright: format modules do not spawn processes, and every format is handled by a
crate in-process.

## What replaced each tool

| Former tool | Replacement | Formats |
| --- | --- | --- |
| `chdman` | `chd-rs` ([CHD Format and chd-rs](../concepts/chd-format-and-chd-rs.md)) | CHD |
| `ctrtool` | `cia-rs` | CIA |
| `maxcso` | `xso-rs` | CSO, ZSO |
| `nsz` | `nsz-rs` | NSZ |
| `gdidrop` | `gdi-rs` | GDI export |
| `flips` | `xps-rs` | IPS, BPS |
| `xdelta3` | `xdelta-rs` | XDELTA |
| 7-Zip | `sevenz-rust2` + `zip` | 7Z, ZIP |
| `dolphin-tool`, `wit` | `nod` | RVZ, WBFS |

`wux-rs` has no row: oxyromon never ran a tool for the Wii U, so the crate follows
wudd and Cemu rather than replacing a subprocess. Its tests use a synthetic disc and
the independent `zarust` ZArchive reader, not an upstream binary.

The crates under `crates/` are oxyromon's own ports, each published on its own
and shipping a standalone CLI; see [Format Crates in crates/](format-crates.md).
`sevenz-rust2`, `zip` and `nod` are ordinary crates.io dependencies.

## What this means at runtime

- **Nothing to install.** No setting names a tool path, nothing reads `${PATH}`
  for a tool, and the Docker image installs no format tool. Every format works in
  every build.
- **Nothing to probe.** `import-roms` dispatches a `.cia` straight into
  `import_cia`, which parses and unpacks it through `src/cia.rs` with byte
  progress from `run_blocking`. A malformed CIA is reported as an error naming
  the file instead of a panic.
- **Nothing can be missing.** `info`, the GraphQL `dependencies` query and the MCP
  `status` payload list `sevenz-rust2`, `chd-rs`, `cia-rs`, `nod`, `nsz-rs`,
  `wux-rs`, `xdelta-rs`, `xps-rs` and `xso-rs`, and every one reports `built-in`.
  Two add a note about keys: `nsz-rs` appends `prod.keys not found` when the Switch
  key file is absent, and `wux-rs` appends `no Wii U keys imported` until
  `import-keys` has run.
- **Temp space still matters.** Intermediate files, CIA unpacks included, go to a
  `TempDir` from `util::create_tmp_directory`, created inside the `TMP_DIRECTORY`
  setting rather than the OS default. The caller holds it for the whole
  operation, because dropping it deletes the directory.

## Reference tools in tests

The upstream tools survive only as oracles for the format crates' tests. Each
test resolves its tool, and skips with a message rather than failing when the
tool is absent, so a contributor without them still gets a green run.

| Crate | Reference | How it is found |
| --- | --- | --- |
| `chd-rs` | `chdman`, plus `ffmpeg` for LaserDisc AVIs | `$CHDMAN` / `$FFMPEG`, then `$PATH` |
| `cia-rs` | `ctrtool` | `$CTRTOOL`, then `$PATH` |
| `xso-rs` | `maxcso` | `$MAXCSO`, then `$PATH` |
| `xps-rs` | `flips` | `flips --version` on `$PATH` |
| `xdelta-rs` | `xdelta3` | `xdelta3 -V` on `$PATH` |

CI installs `mame-tools`, `xdelta3` and `ffmpeg` in its one install step, after an
`apt-get update` with retries, and the workflow comments that all three are only
for the crates' tests. `ctrtool`, `maxcso` and `flips` are not installed, so those
interop suites skip on the runner. The CIA import test in oxyromon itself no
longer depends on any of this: `src/import_roms/test_cia.rs` runs the real
pipeline in-process and has no skip guard.

## Adding a format

There is no seam for a new external tool, by design. A new format goes into a
crate under `crates/` with a synchronous, file-to-file API and a
`progress: &mut dyn FnMut(u64)` callback, driven from `src/` through
`run_blocking`. If the format has an upstream tool, it becomes the reference the
crate's tests compare against, found the same way as above and skipped when
absent. See
[Format Modules and Their Crate Backends](../concepts/format-modules-and-format-crates.md).
