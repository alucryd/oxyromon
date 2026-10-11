---
type: "Reference"
title: "Test Fixtures and End-to-End Suites"
openwiki_generated: true
sources:
  - id: openwiki-source-90dbbac45e714e041e170795
    resource: repo://.github/workflows/continuous_integration.yml
  - id: openwiki-source-ea70eb6c045047448e446296
    resource: repo://.gitignore
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-de387659b991f4f153cbd6d1
    resource: repo://src/check_roms/test_original_crc_mismatch.rs
  - id: openwiki-source-c307f8b7435bf1eb8e07109f
    resource: repo://src/common.rs
  - id: openwiki-source-885c8766d237f8340462593b
    resource: repo://src/download_dats/test_nointro.rs
  - id: openwiki-source-904cc3411b962b115194d55e
    resource: repo://src/download_dats/test_redump.rs
  - id: openwiki-source-6f95130cb448e88dd97ab560
    resource: repo://src/import_dats.rs
  - id: openwiki-source-8485a3ae73bdd8c6923a0bf4
    resource: repo://src/import_dats/test_dat_headered_skipped_header.rs
  - id: openwiki-source-799a986fffbd6d0361994608
    resource: repo://src/import_dats/test_dat_mame_mixed.rs
  - id: openwiki-source-e851f29de7bc17b747354e83
    resource: repo://src/import_dats/test_dat_outdated_should_do_nothing.rs
  - id: openwiki-source-b64c77b451fbf8078aeef243
    resource: repo://src/import_dats/test_zip_nested.rs
  - id: openwiki-source-5822ac752aee6ed90feaf40f
    resource: repo://src/import_patches/test_ips.rs
  - id: openwiki-source-9add64e166d8ef19bd58c5af
    resource: repo://src/import_roms.rs
  - id: openwiki-source-29cce0b2b6a22a67a00d0a31
    resource: repo://src/import_roms/test_multiple_tracks_chd.rs
  - id: openwiki-source-e39f531dcbca745e3e2e92a4
    resource: repo://src/import_roms/test_original.rs
  - id: openwiki-source-c7e9fe6c31daad3e16dda346
    resource: repo://src/import_roms/test_zip_single_file.rs
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
  - id: openwiki-source-0bf78c91e0736d89627ed389
    resource: repo://src/rebuild_roms/test_split_to_non_merged.rs
  - id: openwiki-source-66dda2cecfc81c486446cc8e
    resource: repo://src/util.rs
  - id: openwiki-source-1cf069de5c999c3ff1245ed6
    resource: repo://src/xps/test_patch.rs
  - id: openwiki-source-790aa1ecc03c536bf0e060a6
    resource: repo://tests/compute_sums.sh
  - id: openwiki-source-13df766a80853b510f45e6c3
    resource: repo://tests/crates.rs
  - id: openwiki-source-03238ed8280795dfd0fd3fae
    resource: repo://tests/header.xml
  - id: openwiki-source-9a02c1f55f47430403921057
    resource: repo://tests/mcp_stdio.rs
  - id: openwiki-source-0cc1fa6981e9c7b322a64061
    resource: repo://tests/profile.xml
  - id: openwiki-source-a3a96d35ac80c8ccafbd677c
    resource: repo://tests/Test%20System%20(20000000).dat
  - id: openwiki-source-792ef1983e45ffb1bb16bee7
    resource: repo://tests/Test%20System%20(20200721).dat
  - id: openwiki-source-b6fbe21cd25e7e095cc8c367
    resource: repo://tests/Test%20System%20(20200721)%20(MAME).dat
  - id: openwiki-source-af9ca7a4de06de516c2bef85
    resource: repo://tests/Test%20System%20(20200721)%20(MAME%20Rebuild).dat
  - id: openwiki-source-73e67b23cdfda6c237fcc93e
    resource: repo://tests/Test%20System%20(20200721)%20(Parent-Clone).dat
  - id: openwiki-source-26fd153a05de7ae5f18590a8
    resource: repo://tests/Test%20System%20(20200722)%20(Renamed%20Game).dat
  - id: openwiki-source-10fc26277faaa63bfb24569a
    resource: repo://tests/Test%20System%20(20200723)%20(Changed%20Hashes).dat
  - id: openwiki-source-b9b2d39b54efe492fae94cf7
    resource: repo://tests/Test%20System%20(20210401).dat
  - id: openwiki-source-4a2a57a16282d42c7f9b2379
    resource: repo://tests/Test%20System%20(20210402)%20(Headered).dat
  - id: openwiki-source-cd4b714cdfc6111831a71ca9
    resource: repo://tests/Test%20System%20(20240626)%20(Multiple%20Orphan%20Roms).dat
  - id: openwiki-source-f6fde163dbfa2ff1c0a7baab
    resource: repo://tests/Test%20System%20(20240908)%20(Patches).dat
  - id: openwiki-source-e9594f23f8233266878bea4e
    resource: repo://tests/Test%20System%20(20241228)%20(MAME%20CHD).dat
  - id: openwiki-source-d055e6a57bb9c1c3b426914e
    resource: repo://tests/Test%20System%20(20241230)%20(MAME%20Mixed).dat
generated: { by: "openwiki/0.7.1", at: "2026-10-09T16:11:37.544Z" }
---


# Test Fixtures and End-to-End Suites

`tests/` is not an integration-test directory in the ordinary Rust sense. It is
**a fixture corpus** — 27 Logiqx DAT files and the ROM, disc-image, archive,
patch and header files they describe — consumed by the ~300 `src/<module>/test_*.rs`
modules that live beside the code under test (see
[Testing Conventions](testing-conventions.md)). Only two files in it are test
targets of the root package: `tests/crates.rs` and `tests/mcp_stdio.rs`, and
both are the special cases — one cross-crate parity guard, one end-to-end suite
that spawns the real binary.

## The corpus is read-only; imports move files

The rule that shapes every test's setup: **an import moves, it does not copy**.
`import_roms` relocates the romfile into the system directory with
`rename_file`, which falls back to copy-then-remove only when the rename crosses
filesystems
([src/util.rs](../../src/util.rs#L123-L151)). Handing `tests/` a committed
fixture path directly would delete it from the checkout. So every test copies the
fixture into a scratch directory first and imports that copy
([src/import_roms/test_original.rs](../../src/import_roms/test_original.rs#L19-L37)):

```rust
let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
fs::copy(test_directory.join("Test Game (USA, Europe).rom"), &romfile_path).await.unwrap();
```

Only fixtures that are **read**, never imported, are referenced in place:
`tests/Test Game (USA).ird` for `parse_ird`, `tests/profile.xml` served by a
wiremock server, the DAT zips fed to `download_dats`, and the DATs themselves.

The scratch `TempDir`s are created **inside `tests/`**, not in `/tmp` —
`TempDir::new_in(test_directory)` — so fixtures and their imported results stay
on one filesystem and the rename path is exercised rather than the cross-device
fallback. `.gitignore` carries `tests/.tmp*` for the leftovers
([.gitignore](../../.gitignore#L10-L20)).

## DAT naming: system, version date, scenario suffix

A fixture DAT is named `Test System (<YYYYMMDD>)[ (<Scenario>)]….dat`. The two
identities in play are decoupled, and confusing them is the main way to break a
suite while "renaming a fixture":

- **The file name** is only how a test finds the file. It is a string passed to
  `import_dats::subcommand().get_matches_from(["import-dats", "tests/…dat"])`,
  never parsed by the product code.
- **The `<header><name>`** is the database identity. Systems are matched with
  `find_system_by_name(&system_xml.name)`
  ([src/import_dats.rs](../../src/import_dats.rs#L453-L490)), so the scenario
  suffix belongs in `<name>` **only when the scenario needs its own system**.
- **The `<header><version>`** is the update gate — see the next section.

That yields two naming families. Scenario DATs that are *distinct systems* carry
the suffix in the header too, and the tests assert it: `Test System
(20200721) (Parent-Clone).dat` → `"Test System (Parent-Clone)"`, `Test System
(20230105) (Multiple Discs).dat` → `"Test System (Multiple Discs)"`, `Test System
(20230618) (RVZ).dat` → `"Test System (RVZ)"`. DATs that exercise a **catalog
update of the same system** keep `<name>Test System</name>` and advance only the
date, so the suffix lives in the file name alone: `Test System (20210401).dat`,
`Test System (20200722) (Renamed Game).dat`, `Test System (20200723) (Changed
Hashes).dat`, `Test System (20240626) (Multiple Orphan Roms).dat`
([src/import_dats/test_dat_updated.rs](../../src/import_dats/test_dat_updated.rs#L26-L128)).

The file-name date and `<version>` normally agree, and the deliberate exceptions
are load-bearing: `Test System (20240908) (Patches).dat` carries version
`20230112` and the header name `Test System (Parent-Clone) (Revisions)`;
`Test System (20241010) (Special Characters).dat` carries `20210401`; and `Test
System (20241230) (MAME Mixed).dat` deliberately reuses **both** the header name
and the version of `Test System (20241228) (MAME CHD).dat`, which is why its test
asserts `"Test System (MAME CHD)"` and not anything about "Mixed"
([src/import_dats/test_dat_mame_mixed.rs](../../src/import_dats/test_dat_mame_mixed.rs#L23-L48)).

### The version gate

`import_dat` only touches an existing system when `is_update(old, new)` says so,
and `is_update` is a plain string `Ordering` compare on the version: **less**
prints "Skipping older version" and returns false, **equal** prints "Already up
to date" and returns false, **greater** updates
([src/util.rs](../../src/util.rs#L393-L420)). `--force` bypasses it. This is why
`Test System (20000000).dat` exists at all: it is a *deliberately ancient*
catalog, imported second, used to prove nothing changed
([src/import_dats/test_dat_outdated_should_do_nothing.rs](../../src/import_dats/test_dat_outdated_should_do_nothing.rs#L23-L62)).
Dates are lexicographic, not calendar: any `YYYYMMDD` string works, and a new
scenario DAT that must update an existing system simply needs a version string
that sorts higher.

## A DAT row is a checksum contract with a file on disk

The pairing is exact and unforgiving. Each `<rom>` element declares `name`,
`size`, `crc`, `md5` and `sha1`
([src/model.rs](../../src/model.rs#L267-L284)):

```xml
<rom name="Test Game (USA, Europe).rom" size="256" crc="cc721e14"
    md5="c1fe7a7ed317bd069b0cb7bdbc40be01" sha1="4b3e49f0f22c7ce5f1eb7c30a2bbd6fb4fdc6f3d"
    status="verified" />
```

- `name` must equal a file in `tests/` **byte for byte**, including double
  extensions (`Test Game (USA, Europe).rom.zip`, `.iso.7z`) and the `(Track
  01).bin` spacing. This is what the special-characters scenario probes with its
  `-Test Game@ (USA, Europe).rom` entry.
- `size` and one hash must match the file's bytes, because import matches a file
  by size and hash before it ever looks at a name. `get_hash_algorithm` picks
  **CRC first, then MD5, then SHA1**, whichever the row carries, and
  `compare_hash_and_size` fails with `Size mismatch` or `Checksum mismatch`
  ([src/common.rs](../../src/common.rs#L540-L573)).

For containers, the DAT declares the **entries inside**, not the container: the
base DAT lists `Test Game (USA, Europe).rom` and the test feeds it
`Test Game (USA, Europe).rom.zip`; the CUE/BIN game is declared as a `.cue` plus
two `.bin` tracks and a CHD holding the same three entries is matched to it
([src/import_roms/test_multiple_tracks_chd.rs](../../src/import_roms/test_multiple_tracks_chd.rs#L37-L100)).
MAME arcade DATs break the pattern in one specific way: `<disk>` rows carry only
a `sha1` and no size, because MAME's own CHD DATs have none, and the importer has
a dedicated retry that looks the file up by CHD SHA1 with size 0
([src/import_roms.rs](../../src/import_roms.rs#L1214-L1235)).

```mermaid
flowchart TD
  A["scenario DAT row in tests/"] --> B["declares rom name, size, crc, md5, sha1"]
  B --> C{"file of that exact name in tests/"}
  C -- "archive or CHD" --> D["name is the entry inside the container"]
  C -- "flat file" --> E["name is the fixture itself"]
  D --> F["importer hashes the decoded entry"]
  E --> G["importer hashes the file"]
  F --> H{"size and the DAT's strongest hash agree"}
  G --> H
  H -- "yes" --> I["rom matched, file moved into the system directory"]
  H -- "no" --> J["no match, or check-roms sends it to Trash"]
```

*Caption: a DAT row only works if a fixture file with that exact name exists in `tests/` and its size and hash agree with what the row declares.*

## Checksums for new fixtures: `tests/compute_sums.sh`

Do not compute these by hand or with a home-grown pipeline. `tests/compute_sums.sh`
prints, per file, the `ls -l` size, the CRC via `rhash -C --simple`, the MD5 and
the SHA1 — exactly the four fields a `<rom>` row needs
([tests/compute_sums.sh](../../tests/compute_sums.sh#L1-L8)):

```sh
#!/bin/sh

for f in "$*"; do
    ls -l "$f"
    rhash -C --simple "$f"
    md5sum "$f"
    sha1sum "$f"
done
```

`rhash` must be installed for the CRC line. Run it over every file a new scenario
adds, paste the values into the DAT, and keep the fixture's name equal to the row
it produced. Conversely, **regenerating a fixture invalidates its DAT rows**: the
hashes in the DAT, not the file, are what the assertions ultimately encode.

### Reusing bytes across scenarios

Because the identity is the checksum, one file serves many catalogs:
`Test Game (USA, Europe).rom` (`cc721e14`) is declared verbatim by the base DAT,
the Parent-Clone DAT, the MAME DAT, the Headered DAT and the Patches DAT, and
`Test Game (Asia).rom` (`1c3321f3`) carries the same CRC as `Test BIOS.rom` in
the MAME Rebuild DAT. Arcade scenarios exploit that freedom: the `rebuild_roms`
tests copy a flat fixture to the name the `<machine>` catalog declares
(`Test Game (USA, Europe).rom` → `Test Parent.rom`) rather than committing a
duplicate file ([src/rebuild_roms/test_split_to_non_merged.rs](../../src/rebuild_roms/test_split_to_non_merged.rs#L9-L35)).

## Staging failures without corrupting fixtures

Two techniques, and neither commits a broken ROM:

- **A DAT with the wrong numbers.** `Test System (20210401).dat` and the ancient
  `Test System (20000000).dat` carry a mutated `Test Game (Asia)` CRC
  (`1c3321f4`, one nibble off) so its previously matched romfile is orphaned;
  `Test System (20200723) (Changed Hashes).dat` uses the loud `crc="deadbeef"`
  on `Test Game (USA, Europe).rom`;
  `Test System (20240626) (Multiple Orphan Roms Mismatch).dat` mutates both
  `(Track 0n).bin` hashes. Renaming a game while keeping its rom rows — `Test
  System (20240626) (Multiple Orphan Roms).dat`, `Test System (20200722) (Renamed
  Game).dat` — orphans the same files from the other direction.
- **Corrupt after import.** `check_roms` writes over the already-imported file in
  the ROM directory and asserts the file lands under `/Trash/`
  ([src/check_roms/test_original_crc_mismatch.rs](../../src/check_roms/test_original_crc_mismatch.rs#L50-L72)).

## Non-DAT fixtures

| File | Role |
| --- | --- |
| `header.xml` | A `clrmamepro` header detector (`NES` magic at offset 10) referenced by `<clrmamepro header="header.xml" />` in the Headered DATs; resolved next to the DAT, else from the embedded `data/` assets ([src/import_dats.rs](../../src/import_dats.rs#L283-L304)) |
| `profile.xml` | A no-intro profile served by a wiremock `GET /profile.xml` in the DAT-download test ([src/download_dats/test_nointro.rs](../../src/download_dats/test_nointro.rs#L35-L50)) |
| `Test System (20200721).zip` | The redump datfile payload a mocked `GET /datfile/…/` returns ([src/download_dats/test_redump.rs](../../src/download_dats/test_redump.rs#L28-L70)) |
| `Test System (20210401).zip` | A DAT **nested in a subdirectory**, exercising `import-dats` on an archive |
| `Test Game (USA, Europe).ips` / `.bps` / `.xdelta` with `(patched).rom` | The known-answer triple: apply the patch, compare to the committed expected output ([src/xps/test_patch.rs](../../src/xps/test_patch.rs#L38-L45)) |

## `tests/crates.rs`: the shared CLI shell must not drift

`tests/crates.rs` is a workspace-level guard, not a behaviour test. The five
format crates each ship a CLI whose `ui.rs` — progress bar, `-o DIR`, the
`✔`/`✖` per-input lines — is a **verbatim copy**. The test reads
`crates/xso-rs/src/bin/xsors/ui.rs` as the reference and asserts the copies in
`gdirs`, `nszrs`, `xdeltars` and `xpsrs` are byte-identical
([tests/crates.rs](../../tests/crates.rs#L1-L16)).

It has no feature gate and no fixtures, so it runs on every
`cargo test --workspace`, including the CI coverage pass. Editing one `ui.rs`
without the other four turns CI red with `"<path> differs from xsors's ui.rs"`;
the fix is always to change all five together, which is the convention
`AGENTS.md` records for the crates
([Format Crates](../integrations/format-crates.md)).

## `tests/mcp_stdio.rs`: driving the real binary over JSON-RPC

This suite has no SDK client. Its header states the reason: raw JSON-RPC against
the spawned binary means *"the wire format is what a real client will find"*
([tests/mcp_stdio.rs](../../tests/mcp_stdio.rs#L1-L10)). It is `#![cfg(feature
= "mcp")]`, so without the feature it compiles to nothing.

`Client::spawn(args)` launches `env!("CARGO_BIN_EXE_oxyromon")` — cargo's
build-time path to the `oxyromon` binary target — with `mcp` plus any extra
arguments, and:

- sets **`OXYROMON_DATA_DIRECTORY` to a `TempDir`** held in the `Client` struct,
  so `main.rs` creates `oxyromon.db` in an isolated directory and each test has a
  virgin library instead of the developer's
  ([src/main.rs](../../src/main.rs#L118-L132));
- sets `OXYROMON_LOG_LEVEL=off` and `Stdio::null()` for stderr — nothing but the
  protocol channel is read;
- pipes stdin and stdout, and `initialize` runs before the client is handed back.

Requests are single-line JSON written with `writeln!` to stdin; responses are
read one line at a time and **matched by `id`, with any non-matching message
discarded**. That loop is what lets server-initiated notifications (progress,
etc.) interleave with a call without desynchronising the test
([tests/mcp_stdio.rs](../../tests/mcp_stdio.rs#L45-L67)).

```mermaid
sequenceDiagram
  participant T as mcp_stdio test
  participant B as oxyromon mcp (child)
  participant D as temp data directory
  T->>B: spawn with OXYROMON_DATA_DIRECTORY, OXYROMON_LOG_LEVEL=off
  B->>D: create oxyromon.db, init ROM and tmp settings
  T->>B: initialize 2025-06-18 with an id
  B-->>T: result with serverInfo name oxyromon
  T->>B: notifications initialized (no id)
  T->>B: tools list, tools call, prompts list, resources list
  B-->>T: one JSON line per response, matched by id
  T->>B: drop stdin
  B-->>T: process exits with a success status
```

*Caption: the harness lifecycle — isolated data directory, id-matched newline-delimited JSON-RPC, and a clean EOF-triggered shutdown.*

`close()` drops stdin and asserts `status.success()`: **stdin EOF must terminate
the server cleanly**. That is the observable half of the
`tokio::select! { _ = server.waiting() … }` shape in `mcp::main`
([src/mcp.rs](../../src/mcp.rs#L116-L134)) and the only automated proof that
shutdown does not panic on the way through `optimize_database`. It also implicitly
guards the project's hard rule that **stdout carries nothing but protocol
messages** — a stray `println!` anywhere on the reached path yields a line that
fails `serde_json::from_str` and the test aborts with "a JSON-RPC message"
([MCP Server](../server/mcp-server.md)).

The behaviours pinned by the suite:

| Test | Contract |
| --- | --- |
| `handshake_exposes_the_full_surface` | `initialize` names the server `oxyromon`; `tools/list` contains all 19 tools; `prompts/list` is exactly `audit_system`, `onboard_system` **in that order**; `resources/list` is exactly `oxyromon://status`, `oxyromon://systems`, `oxyromon://settings` |
| `status_reports_the_empty_library` | `structuredContent.version` equals `CARGO_PKG_VERSION`, `read_only` false, `systems` 0 |
| `unknown_ids_are_tool_errors_with_pointers` | `get_system` with an unknown id is a **tool-level** `isError` whose text points at `list_systems` |
| `import_dats_then_read_the_catalog_back` | Importing `tests/Test System (20200721).dat` through the tool yields one system with `games_total` 6, `search_games "test game"` count 6, `list_games limit 2` returning 2 of 6 |
| `destructive_tools_need_confirmation` | `purge_roms` / `purge_systems` / `purge_irds` with `confirm: false` error and mention `confirm` |
| `setting_validation_mirrors_the_cli` | `PREFER_REGIONS=broad` accepted, `mars` rejected with "valid", an unknown key rejected, and the accepted value read back through `get_settings` |
| `read_only_hides_and_refuses_the_write_tools` | With `--read-only` the mutating tools are absent from `tools/list` **and** a direct `tools/call` returns a JSON-RPC `error` object, not a tool result |

The read-only distinction matters: hidden routes are rejected at the protocol
layer (`message["error"]`), while a refused-but-routable call is an `isError`
tool result. Note also the fixture coupling — `import_dats_then_read_the_catalog_back`
hard-codes the game count of the base DAT, so adding or removing a `<game>` from
`tests/Test System (20200721).dat` breaks an MCP test as well as the ~130
in-module tests that import it.

## Running them

```sh
cargo test --workspace --features oxyromon/server   # fixtures, crates.rs, mcp_stdio.rs
cargo test --features oxyromon/server mcp_stdio     # just the stdio suite
./test.sh                                           # coverage over two feature sets
```

`server` implies `mcp`, so CI's single
`cargo llvm-cov --workspace --features oxyromon/server` step runs both suites
([CI Pipelines and Runtime Environment](../operations/ci-and-environment.md)).
The stdio suite is also compiled — linted, at least — under
`cargo clippy --no-default-features --features use-rustls,mcp --all-targets`,
which is the combination where MCP ships without the web server. Without either
feature `mcp_stdio.rs` contributes no tests at all, so a green run there is not
evidence the server was exercised.

## Adding a scenario

1. Name the DAT `Test System (<date>) (<Scenario>).dat`, choosing the date so it
   sorts after any DAT the same system is expected to update from.
2. Decide the `<header><name>`: keep it equal to an existing system to test an
   update, give it the scenario suffix to test a separate system, and match the
   test's assertion to whichever you chose.
3. Add or reuse ROM files, then run `tests/compute_sums.sh` on each and paste the
   size, CRC, MD5 and SHA1 into its `<rom>` row. For a container, declare the
   entries inside it.
4. Copy the fixture into the test's temp directory before importing it — never
   import from `tests/`.
5. Create scratch directories with `TempDir::new_in(test_directory)` so renames
   stay on one filesystem.

See also [Format Crate Test Strategy](format-crate-tests.md) for how the crates
under `crates/` prove themselves against upstream tools, and
[Testing Conventions](testing-conventions.md) for the shared
`MUTEX`/`NamedTempFile` scaffolding every fixture-consuming test uses.
