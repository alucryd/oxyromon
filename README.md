![CI](https://github.com/alucryd/oxyromon/workflows/CI/badge.svg)
[![codecov](https://codecov.io/gh/alucryd/oxyromon/branch/master/graph/badge.svg)](https://codecov.io/gh/alucryd/oxyromon)
[![crates.io](https://img.shields.io/crates/v/oxyromon.svg)](https://crates.io/crates/oxyromon)

# oxyROMon 0.12.0

### Rusty ROM OrgaNizer

oxyROMon is a cross-platform opinionated CLI ROM organizer written in Rust.
Like most ROM managers, it checks ROM files against known good databases.
It is designed with archiving in mind, as such it only supports original and lossless ROM formats.
Sorting can be done in regions mode, in so-called 1G1R mode, or both.
Both console and arcade (WIP) systems are supported using Logiqx DAT files.
The former requires No-Intro or Redump DAT files, the latter can use MAME or FBNeo DAT files.

### Quick start

To create and manage a new system, you need a Logiqx DAT file.
Cartridge based consoles and computers can be downloaded from [Dat-o-Matic](https://datomatic.no-intro.org/).
CD based ones can be downloaded from [Redump](http://redump.org/).
Alternatively the `download-dats` subcommand can download and import Redump DATs for you because they offer direct links.
Arcade DATs are a bit harder to find, [libretro](https://git.libretro.com/libretro/FBNeo/-/tree/master/dats) has some.

Manually downloaded DATs are then imported using the `import-dats` subcommand.
Once a system has been created, you can start importing ROMs using the `import-roms` subcommand.
Imported ROMs that check out will be placed in the main folder of their respective system.
They can then be sorted using the `sort-roms` subcommand according to your configuration.
Please add at least one region in the `REGIONS_ALL` or `REGIONS_ONE` list beforehand.
See configuration below.

You can also convert ROMs between various formats using the `convert-roms` subcommand, check them later on with the `check-roms` subcommand, or purge them with the `purge-roms` subcommand to empty `Trash` folders or find manually deleted ROMs.

### Compilation

The CLI has no specific requirement, you can just:

    cargo build --release

For the web UI, you will also need yarn:

    yarn install
    yarn build
    cargo build --release --features server

The build uses native TLS by default, but you can also opt for rustls:

    cargo build --no-default-features --features use-rustls

### Features

| feature        | description                                                   | default |
| -------------- | ------------------------------------------------------------- | ------- |
| use-native-tls | use the system OpenSSL library                                | x       |
| use-rustls     | use rustls where possible, and fallback to a vendored OpenSSL |         |
| enable-asm     | enable ASM variants of the MD5 and SHA1 hashes                | x       |
| chd            | CHD support                                                   | x       |
| cso            | CSO support                                                   | x       |
| ird            | IRD support                                                   | x       |
| rvz            | RVZ support                                                   | x       |
| benchmark      | build the benchmark subcommand                                |         |
| server         | build the server subcommand                                   |         |

### Configuration

Configuration is done from the command line and settings are stored in the SQLite database.
The database itself is stored in `${data_dir}/oxyromon` as defined in the
[dirs](https://docs.rs/dirs/3.0.1/dirs/fn.data_dir.html) crate.

Available settings:

- `ROM_DIRECTORY`: Full path to your ROM directory, defaults to `${home_dir}/Emulation` as defined in the
  [dirs](https://docs.rs/dirs/3.0.1/dirs/fn.home_dir.html) crate
- `TMP_DIRECTORY`: Full path to a temporary directory for file extraction, defaults to
  [temp_dir](https://doc.rust-lang.org/std/env/fn.temp_dir.html)
- `DISCARD_FLAGS`: List of ROM flags to discard (eg: `Virtual Console`)
- `DISCARD_RELEASES`: List of ROM releases to discard (eg: `Beta`)
- `REGIONS_ALL`: Unordered list of regions for which you want to keep all ROM files
- `REGIONS_ONE`: Ordered list of regions for which you want to keep a single ROM file

Note: `TMP_DIRECTORY` should have at least 8GB of free space to extract those big DVDs.

Example configuration:

```
oxyromon config -l

DISCARD_FLAGS = Aftermarket,Debug
DISCARD_RELEASES = Beta,Proto,Sample,Demo,Hack,Bootleg,Homebrew
HASH_ALGORITHM = CRC
REGIONS_ALL = US,EU,JP
REGIONS_ONE = US,EU
ROM_DIRECTORY = /home/alucryd/Emulation
TMP_DIRECTORY = /tmp
```

### Directory Layout

    ${ROM_DIRECTORY}
        ...
        ⮡ ${SYSTEM_NAME} # Base directory for each system, allowed regions will be stored here
            ⮡ 1G1R # Sub directory for 1G1R games
            ⮡ Trash # Sub directory for trashed games
        ...

### External programs

These should be in your `${PATH}` for extra features.

- [7z](https://www.7-zip.org/download.html): 7Z and ZIP support
- [chdman](https://www.mamedev.org/release.html): CHD support (optional)
- [dolphin-tool](https://dolphin-emu.org/download/): RVZ support (optional)
- [isoinfo](https://sourceforge.net/projects/cdrtools/): IRD support (optional)
- [maxcso](https://github.com/unknownbrackets/maxcso/releases): CSO support (optional)

### TODO

- Remove the ROM original extension from archives, it makes RetroArch name saves differently between imported and non imported ROMs
- Automatically create m3u files for multi-discs games
- Add actions to the web UI
- Add an optional check of the ROMs after conversion
- Find a way to automatically download No-Intro DAT files (just made harder by asking users to click on a color...)
- Support merged sets for arcade systems
- Infer arcade games based on the archive name for duplicate ROMs
- Craft some unit tests for arcade systems
- Craft some unit tests for RVZ
- Craft some unit tests for IRD and PS3 in general
- Support rebuilding PS3 ISOs using IRD files, if possible and requested
- Add a metadata scraper in the retroarch format

## oxyromon

    Usage: oxyromon [COMMAND]

    Commands:
    config         Query and modify the oxyromon settings
    import-dats    Parse and import Logiqx DAT files into oxyromon
    download-dats  Download No-Intro and Redump DAT files and import them into oxyromon
    import-roms    Validate and import ROM files or directories into oxyromon
    sort-roms      Sort ROM files according to region and version preferences
    convert-roms   Convert ROM files between common formats
    rebuild-roms   Rebuild arcade ROM sets according to the selected strategy
    check-roms     Check ROM files integrity
    purge-roms     Purge trashed, missing and orphan ROM files
    purge-systems  Purge systems
    import-irds    Parse and import PlayStation 3 IRD files into oxyromon
    benchmark      Benchmark oxyromon
    server         Launch the backend server
    help           Print this message or the help of the given subcommand(s)

    Options:
    -h, --help     Print help information
    -V, --version  Print version information

## oxyromon-config

Query and modify the oxyromon settings

The settings can be queried, modified and deleted from the command line.

    Usage: oxyromon config [OPTIONS]

    Options:
    -l, --list                  Print the whole configuration
    -g, --get <KEY>             Print a single setting
    -s, --set <KEY> <VALUE>     Configure a single setting
    -a, --add <KEY> <VALUE>     Add an entry to a list
    -r, --remove <KEY> <VALUE>  Remove an entry from a list
    -h, --help                  Print help information

## oxyromon-import-dats

Parse and import Logiqx DAT files into oxyromon

The standard Logiqx XML format is supported, this includes Parent-Clone DAT files.

Supported console DAT providers:

- No-Intro
- Redump

Supported arcade DAT providers:

- MAME
- FBNeo

Note: Some systems require a header definition to be placed alongside the DAT file.
If not provided, oxyromon will use its own fallback header definition.

    Usage: oxyromon import-dats [OPTIONS] <DATS>...

    Arguments:
    <DATS>...  Set the DAT files to import

    Options:
    -i, --info         Show the DAT information and exit
    -s, --skip-header  Skip parsing the header even if the system has one
    -f, --force        Force import of outdated DAT files
    -a, --arcade       Enable arcade mode
    -h, --help         Print help information

## oxyromon-download-dats

Download No-Intro and Redump DAT files and import them into oxyromon

Redump ofers direct downloads, but no summary, whereas No-Intro offers a summary
but no direct downloads. For now the No-intro counterpart will only tell you if
an update is available, but the Redump one is able to download brand new dats
and update those you've already imported.

Supported DAT providers:

- Redump (Download and update)
- No-Intro (Update check only)
<!-- -->
    Usage: oxyromon download-dats [OPTIONS]

    Options:
    -n, --nointro  Download No-Intro DAT files
    -r, --redump   Download Redump DAT files
    -u, --update   Check for system updates
    -a, --all      Import all systems
    -f, --force    Force import of outdated DAT files
    -h, --help     Print help information

## oxyromon-import-irds

Parse and import PlayStation 3 IRD files into oxyromon

IRD files allow validation of extracted PS3 ISOs, a.k.a. JB folders.
Games will be considered complete, as far as oxyromon goes, even if you don't have the `PS3_CONTENT`, `PS3_EXTRA` and `PS3_UPDATE` directories.

Note: Currently supports IRD version 9 only. Should cover most online sources as it is the latest version.

    Usage: oxyromon import-irds [OPTIONS] <IRDS>...

    Arguments:
    <IRDS>...  Set the IRD files to import

    Options:
    -i, --info   Show the IRD information and exit
    -f, --force  Force import of already imported IRD files
    -h, --help   Print help information

## oxyromon-import-roms

Validate and import ROM files or directories into oxyromon

ROM files that match against the database will be placed in the base directory of the system they belong to.
You will be prompted for the system you want to check your ROMs against.
Most files will be moved as-is, with the exception of archives containing multiple games which are extracted.

Supported console ROM formats:

- All No-Intro and Redump supported formats
- 7Z and ZIP archives
- CHD (Compressed Hunks of Data)
- CSO (Compressed ISO)
- RVZ (Modern Dolphin format)
- JB folders (Extracted PS3 ISO)

Supported arcade ROM formats:

- ZIP archives
- Uncompressed folders

Note: Importing a CHD containing multiple partitions requires the matching CUE file from Redump.

    Usage: oxyromon import-roms [OPTIONS] <ROMS>...

    Arguments:
    <ROMS>...  Set the ROM files or directories to import

    Options:
    -s, --system <SYSTEM>  Set the system number to use
    -a, --hash <HASH>      Set the hash algorithm [possible values: CRC, MD5, SHA1]
    -h, --help             Print help information

## oxyromon-sort-roms

Sort ROM files according to region and version preferences

Sorting can be done using several strategies.
You can also choose to discard certain types of games.
Optionally you can print a list of games you may be missing, you hoarder, you.

Supported console modes:

- Regions mode
- 1G1R mode
- Hybrid mode

Supported arcade modes:

- None (yet?)

In regions mode, games belonging to at least one of the specified regions will be placed in the base directory of the
system.
Regions are set via the `REGIONS_ALL` setting, and can overriden via the CLI `-g` flag.

In 1G1R mode, only one game from a Parent-Clone game group will be placed in the 1G1R subdirectory, by order of
precedence.
Regions are set via the `REGIONS_ONE` setting, and can overriden via the CLI `-r` flag.

In hybrid mode, the 1G1R rule applies, plus all remaining games from the selected regions will be placed in the base
directory.

In every mode, discarded games are placed in the `Trash` subdirectory.

1G1R and hybrid modes are still useful even without a Parent-Clone DAT file, it lets you separate games you will
actually play, while keeping original Japanese games for translation patches and other hacks.

The region format uses 2-letter codes according to [TOSEC's naming convention](https://www.tosecdev.org/tosec-naming-convention).

    Usage: oxyromon sort-roms [OPTIONS]

    Options:
    -r, --regions <REGIONS_ALL>...  Set the regions to keep (unordered)
    -g, --1g1r <REGIONS_ONE>...     Set the 1G1R regions to keep (ordered)
    -m, --missing                   Show missing games
    -a, --all                       Sort all systems
    -y, --yes                       Automatically say yes to prompts
    -h, --help                      Print help information

    Example: oxyromon sort-roms -g US EU -r US EU JP

## oxyromon-rebuild-roms

Rebuild arcade ROM sets according to the selected strategy

ROM sets can be rebuilt using the popular merging strategies.

Supported merging strategies:

- Split (each parent and clone set contains only its own ROM files)
- Non-Merged (each parent and clone set contains its ROM files and its parent's files)
- Full Non-Merged (each parent and clone set contains its ROM files, its parent's files, and the required BIOS files)
- ~~Merged (parent and clones are stored together, alongside the required BIOS files)~~
<!-- -->
    Usage: oxyromon rebuild-roms [OPTIONS]

    Options:
    -m, --merging <MERGING>  Set the arcade merging strategy [possible values: SPLIT, NON_MERGED, FULL_NON_MERGED]
    -a, --all                Rebuild all arcade systems
    -y, --yes                Automatically say yes to prompts
    -h, --help               Print help information

## oxyromon-convert-roms

Convert ROM files between common formats

ROMs can be converted back and forth between common formats and their original formats.
Invoking this command will convert all eligible roms for some or all systems.
You may optionally filter games by name, the matching string is not case sensitive and doesn't need to be the full game name.

Supported ROM formats:

- All No-Intro and Redump supported formats <-> 7Z and ZIP archives
- CUE/BIN <-> CHD (Compressed Hunks of Data)
- ISO <-> CHD (Compressed Hunks of Data)
- ISO <-> CSO (Compressed ISO)
- ISO <-> RVZ (Modern Dolphin format)

Note: CHD will be extracted to their original split CUE/BIN where applicable.

    Usage: oxyromon convert-roms [OPTIONS]

    Options:
    -f, --format <FORMAT>  Set the destination format [possible values: ORIGINAL, 7Z, ZIP, CHD, CSO, RVZ]
    -n, --name <NAME>      Select games by name
    -a, --all              Convert all systems/games
    -s, --statistics       Print statistics for each conversion
    -h, --help             Print help information

## oxyromon-check-roms

Check ROM files integrity

This will scan every ROM file in each specified system and move corrupt files to their respective Trash directory.
File sizes can also be computed again, useful for ROM files imported in v0.8.1 or below.

    USAGE:
        oxyromon check-roms [OPTIONS]

    OPTIONS:
        -a, --all     Check all systems
        -h, --help    Print help information
        -s, --size    Recalculate ROM file sizes

## oxyromon-purge-roms

Purge trashed, missing and orphan ROM files

This will optionally purge the database from every ROM file that has gone missing or that is not currently associated
with a ROM, as well as physically delete all files in the `Trash` subdirectories.

    Usage: oxyromon purge-roms [OPTIONS]

    Options:
    -m, --missing  Delete missing ROM files from the database
    -o, --orphan   Delete ROM files without an associated ROM from the database
    -t, --trash    Physically delete ROM files from the trash directories
    -y, --yes      Automatically say yes to prompts
    -h, --help     Print help information

## oxyromon-purge-systems

Purge systems

This will wipe the system and all its ROMs from the database. All ROMs will be placed in the `Trash` folder, it is up to you to physically delete them afterwards.

    Usage: oxyromon purge-systems

    Options:
    -h, --help  Print help information

## oxyromon-server

Launch the backend server

The server exposes a GraphQL API endpoint at `/graphql`. An associated Svelte.js web UI is also exposed at `/`.

    Usage: oxyromon server [OPTIONS]

    Options:
    -a, --address <ADDRESS>  Specify the server address [default: 127.0.0.1]
    -p, --port <PORT>        Specify the server port [default: 8000]
    -h, --help               Print help information

## oxyromon-benchmark

Benchmark oxyromon

Gives some idea about the various read/write performance of the ROM and TMP directories.
It will also rank checksum algorithms, typically CRC should be the fastest, followed by SHA1, and then MD5.
Your mileage may vary depending on your architecture.

    Usage: oxyromon benchmark [OPTIONS]

    Options:
    -c, --chunk-size <CHUNK_SIZE>  Set the chunk size in KB for read and writes (Default: 256) [default: 256]
    -h, --help                     Print help information
