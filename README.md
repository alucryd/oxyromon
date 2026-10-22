![CI](https://github.com/alucryd/oxyromon/workflows/CI/badge.svg)
[![codecov](https://codecov.io/gh/alucryd/oxyromon/branch/master/graph/badge.svg)](https://codecov.io/gh/alucryd/oxyromon)
[![crates.io](https://img.shields.io/crates/v/oxyromon.svg)](https://crates.io/crates/oxyromon)
[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/alucryd)

<p align="center">
  <img src="https://github.com/alucryd/oxyromon/raw/master/resources/logo.svg" alt="logo" width="40%">
</p>

<h1 align="center">oxyROMon</h1>

<h2 align="center">Rusty ROM OrgaNizer</h2>

oxyROMon is a cross-platform, opinionated ROM organizer written in Rust.
It checks ROM files against known-good Logiqx DAT files, imports them into a tidy directory tree, and sorts them by region, in one-game-one-ROM (1G1R) mode, or both.

It is designed with archiving in mind, so it only keeps original and lossless formats.
It can still export to popular lossy formats, leaving your lossless files untouched.
Console and computer systems use No-Intro or Redump DATs, and arcade systems (work in progress) use MAME or FBNeo DATs.

Every format is handled in-process, so oxyROMon needs no external tool.

oxyROMon comes in three flavors, all driving the same code and the same database:

- **A command-line tool**, the core of the project.
- **A web UI**, served by `oxyromon server`, also available as a desktop app.
- **A Model Context Protocol (MCP) server**, so an AI assistant can explore and maintain your library.

<p align="center">
  <img src="https://github.com/alucryd/oxyromon/raw/master/resources/cli.png" alt="the command-line interface" width="100%">
</p>

## Quick start

A system is created from a Logiqx DAT file:

- Cartridge-based consoles and older computers: [Dat-o-Matic](https://datomatic.no-intro.org/) (No-Intro).
- Disc-based consoles: [Redump](http://redump.org/), or let `download-dats` fetch them for you.
- MAME: [Progetto-Snaps](https://www.progettosnaps.net/index.php).
- FBNeo: harder to find, [libretro](https://git.libretro.com/libretro/FBNeo/-/tree/master/dats) has some.

Then the usual loop is:

```sh
oxyromon config -s ROM_DIRECTORY /path/to/Emulation  # where your library lives
oxyromon config -s REGIONS_ALL US,EU,JP              # regions to keep
oxyromon config -s REGIONS_ONE US,EU                 # 1G1R regions, by preference
oxyromon import-dats "Nintendo - Game Boy (20260101-000000).dat"
oxyromon import-roms ~/Downloads/roms
oxyromon sort-roms --system "Game Boy"
```

ROM files that match the database are moved into their system's directory, the rest are left alone.
`sort-roms` then distributes them according to your settings, and needs at least one region in `REGIONS_ALL` or `REGIONS_ONE`.

From there, `convert-roms` switches formats, `check-roms` verifies integrity, and `purge-roms` empties the `Trash` directories and forgets files you deleted by hand.

## Installation

- **Binaries** for Linux, macOS and Windows are on the [release page](https://github.com/alucryd/oxyromon/releases), along with the desktop app and the format crates' command-line tools.
- **Arch Linux** has a package in the [official repositories](https://archlinux.org/packages/extra/x86_64/oxyromon/).
- **Docker** images are on [Docker Hub](https://hub.docker.com/r/alucryd/oxyromon), Alpine-based, with `/data` holding the database and `/roms` the library.
- **crates.io**: `cargo install oxyromon` builds the command-line tool.

## Building

The command-line tool has no special requirement beyond a C toolchain, which the RVZ backend needs, and CMake, which builds the FLAC codec for the CHD backend:

```sh
cargo build --release
```

The web UI is a [Leptos](https://leptos.dev) app compiled to WebAssembly and embedded into the binary.
It needs the [Trunk](https://trunkrs.dev) bundler and the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
cargo build --release --features server
```

Set `SKIP_TRUNK=true` to reuse web assets you already built.

The desktop app lives in `desktop/`.
It wraps the web UI in a [Tauri](https://tauri.app) window and starts a bundled `oxyromon server` behind it, so it shares the command-line tool's database and settings.
It needs everything the web UI needs, the Tauri CLI, and your platform's webview development packages.
On Linux, those are `webkit2gtk-4.1`, `gtk3`, `librsvg` and `libayatana-appindicator`.

```sh
cargo install --locked tauri-cli --version "^2"
cd desktop
cargo tauri build  # or `cargo tauri dev` while working on it
```

The repository also holds oxyROMon's format crates, under `crates/`.
Each is a library and a standalone command-line tool:

```sh
cargo build --release -p chd-rs -p gdi-rs -p nsz-rs -p xdelta-rs -p xps-rs -p xso-rs
```

### Features

| Feature          | Description                                                  | Default |
| ---------------- | ------------------------------------------------------------ | ------- |
| `mcp`            | The MCP server over stdio                                    |         |
| `server`         | The web server and UI, which also serves MCP over HTTP      |         |
| `use-rustls`     | TLS through rustls                                           | ✔       |
| `use-native-tls` | TLS through the system's OpenSSL                             |         |

To use OpenSSL instead of rustls:

```sh
cargo build --release --no-default-features --features use-native-tls
```

## Configuration

Settings are stored in the database and managed with the `config` subcommand.
They are global by default.
Pass `-n/--system` to override one for a single system, which then falls back to the global value when unset.

The database lives in the platform's data directory, under `oxyromon`.
These environment variables change where things go:

| Variable                  | Purpose                                                      |
| ------------------------- | ------------------------------------------------------------ |
| `OXYROMON_DATA_DIRECTORY` | The directory holding the database                           |
| `OXYROMON_ROM_DIRECTORY`  | The initial `ROM_DIRECTORY`, used only until it is set       |
| `OXYROMON_TMP_DIRECTORY`  | The initial `TMP_DIRECTORY`, used only until it is set       |
| `OXYROMON_LOG_LEVEL`      | Log verbosity, in [env_logger](https://docs.rs/env_logger) syntax |

### Directories

| Setting         | Description                                         | Default             |
| --------------- | --------------------------------------------------- | ------------------- |
| `ROM_DIRECTORY` | Your ROM library                                    | `~/Emulation`       |
| `TMP_DIRECTORY` | Scratch space for conversions, at least 8 GB for DVDs | The system's temp directory |

### Sorting

| Setting                  | Description                                                                                  | Default |
| ------------------------ | -------------------------------------------------------------------------------------------- | ------- |
| `REGIONS_ALL`            | Regions for which to keep every game, unordered (eg: `US,EU,JP`)                              |         |
| `REGIONS_ONE`            | Regions for 1G1R, by order of preference (eg: `US,EU`)                                       |         |
| `REGIONS_ALL_SUBFOLDERS` | Subfolders for games: `none` or `alpha`                                                      | `none`  |
| `REGIONS_ONE_SUBFOLDERS` | Subfolders for 1G1R games: `none` or `alpha`                                                 | `none`  |
| `REGIONS_ONE_STRICT`     | Elect 1G1R games whether you own them or not                                                 | `false` |
| `REGIONS_ALL_ARCADE`     | Arcade set types for the main directory: `bios`, `parent`, `clone`                           |         |
| `REGIONS_ONE_ARCADE`     | Arcade set types for the 1G1R directory: `bios`, `parent`, `clone`                           |         |
| `PREFER_PARENTS`         | Favor parents in the 1G1R election                                                           | `true`  |
| `PREFER_REGIONS`         | Favor games released in more (`broad`) or fewer (`narrow`) regions, or neither (`none`)       | `none`  |
| `PREFER_VERSIONS`        | Favor `new` or `old` revisions, or neither (`none`)                                          | `new`   |
| `PREFER_FLAGS`           | Flags to favor in the 1G1R election (eg: `Rumble Version`)                                   |         |
| `DISCARD_FLAGS`          | Flags to discard (eg: `Virtual Console`)                                                     |         |
| `DISCARD_RELEASES`       | Releases to discard (eg: `Beta,Proto`)                                                       |         |
| `LANGUAGES`              | Languages to keep, for games that list theirs (eg: `En,Ja`)                                  |         |
| `GROUP_SUBSYSTEMS`       | Put every variant of a system in the same directory                                          | `true`  |

### Formats

| Setting                           | Description                                                                                              | Default   |
| --------------------------------- | -------------------------------------------------------------------------------------------------------- | --------- |
| `PREFER_FORMAT`                   | Format imported ROMs are converted to: `ORIGINAL`, `7Z`, `CHD`, `CSO`, `NSZ`, `RVZ`, `ZIP`, `ZSO`; arcade systems take only `ORIGINAL` and `ZIP` |           |
| `CHD_PARENTS`                     | Use the other discs of a game as CHD parents, which needs playlists to have been generated              | `false`   |
| `CHD_CD_HUNK_SIZE`                | CHD hunk size in bytes for CDs, `16` to `1048576`                                                        | auto      |
| `CHD_CD_COMPRESSION_ALGORITHMS`   | Up to 4 CHD codecs for CDs: `none`, `cdfl`, `cdlz`, `cdzl`, `cdzs`                                       | auto      |
| `CHD_DVD_HUNK_SIZE`               | CHD hunk size in bytes for DVDs, `16` to `1048576`                                                       | auto      |
| `CHD_DVD_COMPRESSION_ALGORITHMS`  | Up to 4 CHD codecs for DVDs: `none`, `flac`, `huff`, `lzma`, `zlib`, `zstd`                              | auto      |
| `CHD_HD_HUNK_SIZE`                | CHD hunk size in bytes for hard disks, `16` to `1048576`                                                 | auto      |
| `CHD_HD_COMPRESSION_ALGORITHMS`   | Up to 4 CHD codecs for hard disks: `none`, `flac`, `huff`, `lzma`, `zlib`, `zstd`                        | auto      |
| `CHD_LD_HUNK_SIZE`                | CHD hunk size in bytes for LaserDiscs, `16` to `1048576`                                                 | auto      |
| `CHD_LD_COMPRESSION_ALGORITHMS`   | CHD codecs for LaserDiscs: `none`, `avhu`                                                                | auto      |
| `RVZ_BLOCK_SIZE`                  | RVZ block size in KiB, `32` to `2048`                                                                    | `128`     |
| `RVZ_COMPRESSION_ALGORITHM`       | RVZ codec: `none`, `zstd`, `bzip2`, `lzma`, `lzma2`                                                      | `zstd`    |
| `RVZ_COMPRESSION_LEVEL`           | RVZ level, `1` to `22` for zstd and `1` to `9` otherwise                                                 | `5`       |
| `RVZ_SCRUB`                       | Scrub RVZ exports; currently ignored with a warning, as the RVZ backend cannot scrub                     | `false`   |
| `SEVENZIP_COMPRESSION_ALGORITHM`  | 7Z codec: `lzma2` or `zstd`                                                                              | `lzma2`   |
| `SEVENZIP_COMPRESSION_LEVEL`      | 7Z LZMA2 level, `1` to `9`                                                                               | `9`       |
| `SEVENZIP_ZSTD_COMPRESSION_LEVEL` | 7Z Zstandard level, `1` to `22`                                                                          | `19`      |
| `SEVENZIP_SOLID_COMPRESSION`      | Solid 7Z archives                                                                                        | `false`   |
| `ZIP_COMPRESSION_ALGORITHM`       | ZIP codec: `deflate` or `zstd`                                                                           | `deflate` |
| `ZIP_COMPRESSION_LEVEL`           | ZIP Deflate level, `1` to `9`                                                                            | `9`       |
| `ZIP_ZSTD_COMPRESSION_LEVEL`      | ZIP Zstandard level, `1` to `22`                                                                         | `19`      |

Zstandard archives are opt-in.
They compress much faster than LZMA2 at a comparable size, but almost nothing else reads them.
7-Zip, emulators and Windows Explorer cannot open a Zstandard `.7z` or `.zip`, so keep them for archival rather than for a library you play from.

## Directory layout

```text
${ROM_DIRECTORY}
├── ${SYSTEM_NAME}  # each system's games, from the regions you keep
│   ├── 1G1R        # its 1G1R games
│   └── Trash       # its discarded games
└── Trash           # files import-roms -t matched to no system
```

`GROUP_SUBSYSTEMS` controls `${SYSTEM_NAME}`: when on, every variant of a system shares one directory.

## Supported formats

| Format                  | Import and check | Convert | Export | Backend                                                      |
| ----------------------- | :--------------: | :-----: | :----: | ------------------------------------------------------------ |
| Original files          | ✔                | ✔       | ✔      |                                                              |
| 7Z, ZIP                 | ✔                | ✔       | ✔      | [sevenz-rust2](https://crates.io/crates/sevenz-rust2), [zip](https://crates.io/crates/zip) |
| CHD                     | ✔                | ✔       | ✔      | chd-rs                                                       |
| CSO, ZSO                | ✔                | ✔       | ✔      | xso-rs                                                       |
| NSZ                     | ✔                | ✔       | ✔      | nsz-rs                                                       |
| RVZ                     | ✔                | ✔       | ✔      | [nod](https://crates.io/crates/nod)                          |
| WBFS                    |                  |         | ✔      | nod                                                          |
| GDI                     |                  |         | ✔      | gdi-rs                                                       |
| ISO (from CUE/BIN)      |                  |         | ✔      |                                                              |
| CIA                     | ✔                |         |        | cia-rs                                                       |
| JB folders (PS3)        | ✔                |         |        | IRD files                                                    |
| BPS, IPS, XDELTA patches | ✔               |         |        | xps-rs, xdelta-rs                                            |

The crates without a link are oxyROMon's own, in `crates/`.
Each began as a port of the tool oxyROMon used to run: chdman, ctrtool, maxcso, nsz, gdidrop, Flips and xdelta3.

Your Switch keys, at `~/.switch/prod.keys`, are only needed to compress NSPs that contain NCAs.
Importing, checking and decompressing NSZs never need them.

RVZ files are interchangeable with Dolphin's in both directions.

## Web UI and desktop app

`oxyromon server` serves the web UI at `http://127.0.0.1:8000` by default, and the desktop app shows the same UI in its own window.
From there you can browse systems, games and ROM files, see each system's completion, download a ROM file, edit global and per-system settings, purge a system, import DAT files, uploaded or fetched from Redump, and import ROM files.

<p align="center">
  <img src="https://github.com/alucryd/oxyromon/raw/master/resources/web.png" alt="the web UI" width="100%">
</p>

The server has no authentication.
It can also import ROM files from a URL, which the server itself downloads, so it reaches anything the server can reach.
Keep it on the default loopback address unless you trust your network.

## MCP server

The MCP server lets an AI assistant explore and maintain your library with the same actions as the command-line tool:

- **Read-only tools:** `status`, `list_systems`, `get_system`, `list_games`, `get_game`, `search_games`, `get_settings`, `downloadable_systems` and `parse_game_name`.
- **Mutating tools:** `import_dats`, `download_dats`, `import_roms`, `sort_roms`, `check_roms`, `convert_roms` and `set_setting`.
- **Destructive tools:** `purge_roms`, `purge_systems` and `purge_irds`, which refuse to run without `confirm: true`.

Mutating tools run to completion within the call, report progress as they go, and can be cancelled.
The server also exposes resources (`oxyromon://status`, `oxyromon://systems`, `oxyromon://systems/{id}` and `oxyromon://settings`) and two guided prompts, `onboard_system` and `audit_system`.

`oxyromon mcp` speaks over stdio, and `--read-only` hides every mutating tool:

```json
{
  "mcpServers": {
    "oxyromon": {
      "command": "oxyromon",
      "args": ["mcp"]
    }
  }
}
```

`oxyromon server` also serves it over HTTP at `/mcp`:

```json
{
  "mcpServers": {
    "oxyromon": {
      "url": "http://127.0.0.1:8000/mcp"
    }
  }
}
```

## Commands

```text
Usage: oxyromon [COMMAND]

Commands:
  info                Print system information
  config              Query and modify the oxyromon settings
  create-dats         Create DAT files from directories
  import-dats         Parse and import Logiqx DAT files into oxyromon
  download-dats       Download No-Intro and Redump DAT files and import them into oxyromon
  import-irds         Parse and import PlayStation 3 IRD files into oxyromon
  import-patches      Import patch files into oxyromon
  import-roms         Validate and import ROM files or directories into oxyromon
  purge-irds          Unassociate games from IRD files
  sort-roms           Sort ROM files according to region and version preferences
  convert-roms        Convert ROM files between common formats
  export-roms         Export ROM files to common formats
  rebuild-roms        Rebuild arcade ROM sets according to the selected strategy
  check-roms          Check ROM files' integrity
  purge-roms          Purge trashed, missing, and orphan ROM files
  purge-systems       Purge systems
  generate-playlists  Generate M3U playlists for multi-disc games
  benchmark           Benchmark oxyromon
  mcp                 Serve the Model Context Protocol on stdin/stdout
  server              Launch the backend server
  help                Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

`mcp` needs the `mcp` feature, and `server` the `server` feature.

### info

Prints the version, every format backend and its version, and some library statistics.

```text
Usage: oxyromon info

Options:
  -h, --help  Print help
```

### config

Lists, reads, sets and unsets settings, and adds or removes entries of list settings.
Every operation is global unless `-n/--system` scopes it to one system.

```text
Usage: oxyromon config [OPTIONS]

Options:
  -n, --system <NAME>         Select a system by name (supports % globs)
  -l, --list                  Print the whole configuration
  -g, --get <KEY>             Print a single setting
  -s, --set <KEY> <VALUE>     Set a single setting
  -u, --unset <KEY>           Unset a single setting
  -a, --add <KEY> <VALUE>     Add an entry to a list
  -r, --remove <KEY> <VALUE>  Remove an entry from a list
  -h, --help                  Print help
```

### create-dats

Builds a DAT file from a directory, which oxyROMon or any other tool can then import.
Name and description default to the directory's name, and the version to the current date and time, No-Intro style.
Each file becomes a game named after the file without its extension, with all three checksums.

Files are hashed as they are and subdirectories are ignored.
Use original, uncompressed files if you plan to import the DAT and then convert or export its ROMs.

```text
Usage: oxyromon create-dats [OPTIONS] <DIRECTORIES>...

Arguments:
  <DIRECTORIES>...  Set the directories to process

Options:
  -n, --name <NAME>                Customize the DAT name
  -d, --description <DESCRIPTION>  Customize the DAT description
  -v, --version <VERSION>          Customize the DAT version
  -a, --author <AUTHOR>            Customize the DAT author
  -u, --url <URL>                  Customize the DAT URL
  -o, --output <OUTPUT>            Customize the output directory
  -h, --help                       Print help
```

### import-dats

Imports Logiqx DAT files from No-Intro, Redump or MAME, including Parent-Clone DATs.
ZIP files such as No-Intro's daily packs are read directly, including DATs in nested directories.
The output of MAME's or HBMAME's `-listxml` is read directly too: having no header, its system is named after the file, which `-n` overrides, and versioned after the build.
With `-u`, only systems already in the database are updated, which keeps a collection in sync with those packs.

An update keeps a system's custom name and extension unless new ones are given.
`-f` sets each to what is given, so one left out reverts to the DAT's.
When an update moves ROM files, to a renamed game or to `Trash`, the directories they leave empty are deleted, except the ROM, system and `Trash` directories.

Some systems need a header definition next to their DAT file.
oxyROMon falls back to its own when it is missing.

A DAT that lists machines, like MAME's, is imported as an arcade system.
FBNeo's arcade DATs list games, so import them with `-a` to make them arcade.
An update keeps a system arcade, and `-f` without `-a` reverts it.

MAME ships separate DATs for ROMs, CHDs and more.
Turn `GROUP_SUBSYSTEMS` on and name them `MAME (ROMs)`, `MAME (CHDs)` and so on, so they share a `MAME` directory that MAME reads as is.

```text
Usage: oxyromon import-dats [OPTIONS] <DATS>...

Arguments:
  <DATS>...  Set the DAT files to import

Options:
  -i, --info                   Show the DAT information and exit
  -u, --update                 Only import DAT files for systems already in the database
  -s, --skip-header            Skip parsing the header even if the system has one
  -f, --force                  Force import of outdated DAT files
  -n, --name <NAME>            Customize the system name
  -e, --extension <EXTENSION>  Customize the system extension
  -a, --arcade                 Import as arcade systems, for DATs that list games instead of machines
  -h, --help                   Print help
```

### download-dats

Redump offers direct downloads, so this can download and import new Redump systems and update those you have.
A DAT no newer than the imported one changes nothing, unless `-f` forces it.
A system that fails to download is reported and skipped, and the run then fails, naming it.
No-Intro offers no direct downloads, so for No-Intro it only reports which systems have an update.
As with `import-dats`, an update keeps custom names and extensions, and `-f` reverts them.

```text
Usage: oxyromon download-dats [OPTIONS]

Options:
  -n, --nointro           Download No-Intro DAT files
  -r, --redump            Download Redump DAT files
  -u, --update            Check for system updates
  -a, --all               Import all systems
  -f, --force             Force import of outdated DAT files
  -s, --save <DIRECTORY>  Save downloaded DAT files to the specified directory
  -h, --help              Print help
```

### import-irds

Imports PlayStation 3 IRD files, which validate JB folders, the extracted form PS3 games are usually dumped in.
A game counts as complete without its `PS3_CONTENT`, `PS3_EXTRA` and `PS3_UPDATE` directories.
When an IRD no longer matches a file already in its JB folder, that file is imported again or moved to `Trash`.

Import a PS3 DAT first.
If it doesn't come from Redump, make sure its name contains `PlayStation 3`.
Only IRD version 9, the latest, is supported.

```text
Usage: oxyromon import-irds [OPTIONS] <IRDS>...

Arguments:
  <IRDS>...  Set the IRD files to import

Options:
  -i, --info             Show the IRD information and exit
  -f, --force            Force import of already imported IRD files
      --system <SYSTEM>  Select the system to import into, matching the game and ROM automatically
  -h, --help             Print help
```

### purge-irds

Removes the ROMs an IRD import created, moving their files to the system's `Trash` directory.
The game is no longer a JB folder afterwards, so a different IRD can be imported.

```text
Usage: oxyromon purge-irds [OPTIONS] [GAMES]...

Arguments:
  [GAMES]...  Set the game names to purge

Options:
      --system <SYSTEM>  Select the system to purge from, skipping the prompt and purging every IRD game when no names are given
  -h, --help             Print help
```

### import-patches

Imports BPS, IPS and XDELTA patches, named after their ROM files following RetroArch's softpatching convention.

```text
Usage: oxyromon import-patches [OPTIONS] <PATCHES>...

Arguments:
  <PATCHES>...  Set the patch files to import

Options:
  -n, --name       Customize patch names
  -f, --force      Force import of already imported patch files
      --rom <ROM>  Select the target ROM by id, skipping the prompts
  -h, --help       Print help
```

### import-roms

Hashes files and directories and moves those that match the database into their system's directory.
The system is usually detected, but you can narrow the search with `-s`, by a case-insensitive partial name.
`-s` is also needed to match headered ROMs, for the Nintendo Entertainment System (Headerless), Famicom Disk System, Atari 7800 and Atari Lynx.

Files are moved as they are, except archives holding several games, which are extracted.
When the system has a `PREFER_FORMAT`, imported ROMs are then converted to it, unless you pass `--as-is`.
Files needing a choice, such as JB folders or ambiguous matches, prompt by default, and `-u` skips them or picks the first match instead.

Consoles accept every No-Intro and Redump format, 7Z, ZIP, CHD, CIA, CSO, NSZ, RVZ, ZSO and JB folders.
Arcade systems accept uncompressed files, ZIP and CHD.

```text
Usage: oxyromon import-roms [OPTIONS] <ROMS>...

Arguments:
  <ROMS>...  Set the ROM files or directories to import

Options:
  -s, --system <SYSTEM>    Select systems by name
  -t, --trash              Trash invalid ROM files
  -d, --delete             Delete invalid ROM files (hard delete)
  -f, --force              Force import of existing ROM files
  -u, --unattended <MODE>  Handle ROM files that require human intervention (skip: skip, first: auto-select first match) [default: none] [possible values: none, skip, first]
  -x, --extract            Extract top-level archives before importing their contents
  -a, --as-is              Import ROM files as-is without converting them
  -h, --help               Print help
```

### sort-roms

Moves games between the system directory, `1G1R` and `Trash`, according to your settings.

- **Regions mode** keeps every game from the `REGIONS_ALL` regions in the system directory.
- **1G1R mode** keeps one game per Parent-Clone group in `1G1R`, elected by `REGIONS_ONE` order and the `PREFER_*` settings.
- **Hybrid mode** does both: the 1G1R election, plus every remaining game from the `REGIONS_ALL` regions.

By default, the election only considers games you own.
`REGIONS_ONE_STRICT` makes it follow your preferences regardless, so a missing game shows as wanted instead of being replaced.
1G1R is useful even without a Parent-Clone DAT: it separates the games you play from, say, Japanese originals kept for translation patches.

Arcade systems sort by set type instead.
`REGIONS_ALL_ARCADE` and `REGIONS_ONE_ARCADE` say which of `bios`, `parent` and `clone` go to each directory.
With both empty, everything stays in the system directory, and otherwise the types left out are discarded.

Discarded games always go to `Trash`.
Regions use TOSEC's [two-letter codes](https://www.tosecdev.org/tosec-naming-convention), and `ZZ`, the unknown region, works as a catch-all in `REGIONS_ALL`.

```text
Usage: oxyromon sort-roms [OPTIONS]

Options:
  -r, --regions <REGIONS_ALL>...
          Set the regions to keep (unordered)
      --subfolders <REGIONS_ALL_SUBFOLDERS>
          Set the subfolders scheme for games [possible values: none, alpha]
  -o, --1g1r <REGIONS_ONE>...
          Set the 1G1R regions to keep (ordered)
      --1g1r-subfolders <REGIONS_ONE_SUBFOLDERS>
          Set the subfolders scheme for 1G1R games [possible values: none, alpha]
  -w, --wanted
          Show wanted games
  -a, --all
          Sort all systems
  -s, --system <SYSTEM>
          Select systems by name
  -y, --yes
          Automatically say yes to prompts
  -h, --help
          Print help
```

### convert-roms

Converts ROM files between lossless formats and back to their originals:

- Any original format to and from 7Z and ZIP.
- CUE/BIN and ISO to and from CHD, with CHDs restored to their original split CUE/BIN.
- ISO to and from CSO, ZSO and RVZ.
- NSP to and from NSZ.

Games and systems are selected by case-insensitive name, with SQL wildcards, and both flags can be repeated.
`--save` also stores the format as each processed system's `PREFER_FORMAT`, so future imports follow it.

```text
Usage: oxyromon convert-roms [OPTIONS]

Options:
  -f, --format <FORMAT>  Set the destination format [possible values: ORIGINAL, 7Z, CHD, CSO, NSZ, RVZ, ZIP, ZSO]
  -g, --game <GAME>      Select games by name
  -s, --system <SYSTEM>  Select systems by name
  -a, --all              Convert all systems/games
  -r, --recompress       Force conversion even if already in the selected format
  -d, --diff             Print size differences
  -c, --check            Check ROM files after conversion
  -p, --parents          Prompt for CHD parents
  -S, --save             Persist the format as the systems' preferred format (PREFER_FORMAT)
  -h, --help             Print help
```

### export-roms

Like `convert-roms`, but writes copies to another directory and leaves your library untouched, so lossy formats are allowed.
It is meant for real hardware, through an EverDrive or an optical drive emulator.

`ISO` targets OPL on the PlayStation 2.
It turns CUE/BIN CD games into an ISO of the first track's 2048-byte data sectors, as bchunk does, keeping only that track for multi-track games.

```text
Usage: oxyromon export-roms [OPTIONS] --directory <DIRECTORY>

Options:
  -f, --format <FORMAT>        Set the destination format [possible values: ORIGINAL, 7Z, CHD, CSO, GDI, ISO, NSZ, RVZ, WBFS, ZIP, ZSO]
  -g, --game <GAME>            Select games by name
  -s, --system <SYSTEM>        Select systems by name
  -d, --directory <DIRECTORY>  Set the output directory
  -o, --1g1r                   Export 1G1R games only
  -h, --help                   Print help
```

### rebuild-roms

Rebuilds arcade sets with another merging strategy:

- **Split:** each set holds only its own files.
- **Non-merged:** each set also holds its parent's files.
- **Full non-merged:** each set also holds its parent's files and the BIOS files it needs.
- **Merged:** a parent's ZIP also holds its clones' files, each stored once. A clone's file whose name the parent already uses with other contents goes under `<clone>/`, as ClrMamePro does. Sets must be ZIPs: convert loose sets with `convert-roms` first.

```text
Usage: oxyromon rebuild-roms [OPTIONS]

Options:
  -m, --merging <MERGING>  Set the arcade merging strategy [possible values: SPLIT, NON_MERGED, FULL_NON_MERGED, MERGED]
  -f, --force              Force rebuild even if merging strategy is unchanged
  -a, --all                Rebuild all arcade systems
  -y, --yes                Automatically say yes to prompts
  -h, --help               Print help
```

### check-roms

Hashes every ROM file of the selected systems again and moves corrupt ones to `Trash`.
`--size` also recomputes stored file sizes.

```text
Usage: oxyromon check-roms [OPTIONS]

Options:
  -a, --all              Check all systems
  -g, --game <GAME>      Select games by name
  -s, --size             Recalculate ROM file sizes
      --system <SYSTEM>  Select systems by name
  -h, --help             Print help
```

### purge-roms

Forgets ROM files that went missing or that no ROM references, and deletes the files in every `Trash` directory, the ROM directory's own included.
`--foreign` deletes files in the ROM directory that the database doesn't know.
Directories left empty are deleted, except the ROM, system and `Trash` directories.

```text
Usage: oxyromon purge-roms [OPTIONS]

Options:
  -m, --missing  Delete missing ROM files from the database
  -o, --orphan   Delete ROM files without an associated ROM from the database
  -t, --trash    Physically delete ROM files from the trash directories
  -f, --foreign  Physically delete ROM files unknown to the database
  -y, --yes      Automatically say yes to prompts
  -h, --help     Print help
```

### purge-systems

Removes systems and their ROMs from the database and moves their files to `Trash`, for you to delete afterwards.

```text
Usage: oxyromon purge-systems [OPTIONS]

Options:
  -e, --empty            Only list empty systems for selection
  -s, --system <SYSTEM>  Select systems by name
  -h, --help             Print help
```

### generate-playlists

Writes M3U playlists for multi-disc Redump games, so RetroArch can swap discs.
The playlists also tell `CHD_PARENTS` which discs belong together.
`sort-roms` moves playlists along with their games, but run this again after `convert-roms`.

```text
Usage: oxyromon generate-playlists [OPTIONS]

Options:
  -a, --all   Generate playlists for all systems
  -h, --help  Print help
```

### benchmark

Measures read and write speeds in the ROM and temporary directories, and ranks the checksum algorithms.
CRC is usually fastest, then SHA1, then MD5, though it depends on your hardware.

```text
Usage: oxyromon benchmark [OPTIONS]

Options:
  -c, --chunk-size <CHUNK_SIZE>  Set the chunk size in KB for read and writes (Default: 256) [default: 256]
  -h, --help                     Print help
```

### mcp

Serves MCP over stdio, as described in [MCP server](#mcp-server).
Nothing else may write to standard output while it runs.

```text
Usage: oxyromon mcp [OPTIONS]

Options:
      --read-only  Expose only non-mutating tools
  -h, --help       Print help
```

### server

Serves the web UI at `/`, the GraphQL API at `/graphql`, and MCP at `/mcp`.

```text
Usage: oxyromon server [OPTIONS]

Options:
  -a, --address <ADDRESS>  Specify the server address [default: 127.0.0.1]
  -p, --port <PORT>        Specify the server port [default: 8000]
  -h, --help               Print help
```

## TODO

- Add more actions to the web UI
- Find a way to automatically download No-Intro DAT files
- Support merged sets for arcade systems
- Write tests for arcade systems, for NSZ with a compressible NCA, and for IRD and PS3 in general
- Support rebuilding PS3 ISOs from IRD files, if possible and requested
- Add a metadata scraper in the RetroArch format

## Contributing

[AGENTS.md](AGENTS.md) holds the project's conventions, and `openwiki/` a generated wiki of how the code works.
