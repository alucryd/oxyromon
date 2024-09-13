![CI](https://github.com/alucryd/oxyromon/workflows/CI/badge.svg)
[![codecov](https://codecov.io/gh/alucryd/oxyromon/branch/master/graph/badge.svg)](https://codecov.io/gh/alucryd/oxyromon)
[![crates.io](https://img.shields.io/crates/v/oxyromon.svg)](https://crates.io/crates/oxyromon)

<img 
    style="display: block; 
           margin-left: auto;
           margin-right: auto;
           width: 20%;"
    src="https://github.com/alucryd/oxyromon/raw/master/resources/logo.svg" 
    alt="logo">
</img>

<h1 style="text-align: center;">oxyROMon 0.20.0</h1>

### Rusty ROM OrgaNizer

oxyROMon is a cross-platform opinionated CLI ROM organizer written in Rust.
Like most ROM managers, it checks ROM files against known good databases.
It is designed with archiving in mind, so it only supports original and lossless ROM formats.
It can, however, export in various popular lossy formats, leaving the lossless ROM files untouched.
Sorting can be done in regions mode, in so-called 1G1R mode, or both.
Console, computer, and arcade (WIP) systems are supported using Logiqx DAT files.
The first two require No-Intro or Redump DAT files, while the latter uses MAME or FBNeo DAT files.

<img 
    style="display: block; 
           margin-left: auto;
           margin-right: auto;
           width: 100%;"
    src="https://github.com/alucryd/oxyromon/raw/master/resources/screenshot.png" 
    alt="screenshot">
</img>

### Quick start

To create and manage a new system, you need a Logiqx DAT file.
Cartridge-based consoles and older computers can be downloaded from [Dat-o-Matic](https://datomatic.no-intro.org/).
CD-based ones can be downloaded from [Redump](http://redump.org/).
Alternatively, the `download-dats` subcommand can download and import Redump DATs for you because they offer direct links.
MAME DATs can be found on [Progetto-Snaps](https://www.progettosnaps.net/index.php). FBNeo DATs are harder to find; [libretro](https://git.libretro.com/libretro/FBNeo/-/tree/master/dats) has some.

Manually downloaded DATs are then imported using the `import-dats` subcommand.
Once a system has been created, you can import ROMs using the `import-roms` subcommand.
Imported ROMs that check out will be placed in the main folder of their respective system.
Then, the ROMs can be sorted using the `sort-roms` subcommand according to your configuration.
Please add at least one region in the `REGIONS_ALL` or `REGIONS_ONE` list beforehand.
See all configuration options below.

You can also convert ROMs between various formats using the `convert-roms` subcommand, check them later on with the `check-roms` subcommand, or purge them with the `purge-roms` subcommand to empty `Trash` folders or detect and forget manually deleted ROMs.

### Installation

You can grab pre-compiled Linux, Mac, and Windows binaries from the [release page](https://github.com/alucryd/oxyromon/releases).

A docker image is also available on [Docker Hub](https://hub.docker.com/r/alucryd/oxyromon).

If you use Arch Linux, there's a package in the [official repos](https://archlinux.org/packages/extra/x86_64/oxyromon/).

It is also possible to install from crates.io using `cargo install oxyromon`.

Finally, if you're feeling adventurous, you can always build from the source, as shown in the instructions below.

### Compilation

The CLI has no specific requirement, and you can just:

    cargo build --release

For the web UI, you will also need yarn:

    cargo build --release --features server

The build uses rustls by default, but you can also opt for OpenSSL:

    cargo build --no-default-features --features use-native-tls

### Features

| feature        | description                                    | default |
| -------------- | ---------------------------------------------- | ------- |
| server         | build the server subcommand                    |         |
| enable-asm     | enable ASM variants of the MD5 and SHA1 hashes | x       |
| use-native-tls | use the system OpenSSL library                 |         |
| use-rustls     | use rustls                                     | x       |

### Configuration

Configuration is done from the command line, and settings are stored in the SQLite database.
The database itself is stored in `${data_dir}/oxyromon` as defined in the [dirs](https://docs.rs/dirs/3.0.1/dirs/fn.data_dir.html) crate.
This may be overwritten using the `OXYROMON_DATA_DIR` environment variable.

Available settings:

- `ROM_DIRECTORY`: Full path to your ROM directory, defaults to `${home_dir}/Emulation` as defined in the
  [dirs](https://docs.rs/dirs/3.0.1/dirs/fn.home_dir.html) crate
- `TMP_DIRECTORY`: Full path to a temporary directory for file extraction, defaults to
  [temp_dir](https://doc.rust-lang.org/std/env/fn.temp_dir.html)
- `PREFER_PARENTS`: Favor parents in the 1G1R election process, defaults to `true`
- `PREFER_REGIONS`: Favor ROMs targeting more or fewer regions in the 1G1R election process, defaults to `none`, valid choices: `none`, `broad`, `narrow`
- `PREFER_VERSIONS`: Favor newer or earlier versions of ROMs in the 1G1R election process, defaults to `new`, valid choices: `none`, `new`, `old`
- `PREFER_FLAGS`: List of ROM flags to favor in the 1G1R election process (eg: `Rumble Version`)
- `DISCARD_FLAGS`: List of ROM flags to discard (eg: `Virtual Console`)
- `DISCARD_RELEASES`: List of ROM releases to discard (eg: `Beta`)
- `LANGUAGES`: List of languages you want to keep, applies only to ROMs that do specify them (eg: `En,Ja`)
- `REGIONS_ALL`: Unordered list of regions for which you want to keep all ROM files (eg: `US,EU,JP`)
- `REGIONS_ONE`: Ordered list of regions for which you want to keep a single ROM file (eg: `US,EU`)
- `REGIONS_ALL_SUBFOLDERS`: Sort ROMs in subfolders, defaults to `none`, valid choices: `none`, `alpha`
- `REGIONS_ONE_SUBFOLDERS`: Sort 1G1R ROMs in subfolders, defaults to `none`, valid choices: `none`, `alpha`
- `REGIONS_ONE_STRICT`: `true` will elect ROMs regardless of them being available, `false` will only elect available ROMs, defaults to `false`
- `GROUP_SUBSYSTEMS`: Group all system variants in a single directory, defaults to `true`
- `CHD_CD_HUNK_SIZE`: The CHD hunk size in bytes for CDs, defaults to auto, valid range: `16-1048576`
- `CHD_CD_COMPRESSION_ALGORITHMS`: The CHD compression algorithms for CDs, up to 4 can be specified, defaults to auto, valid choices: `none`, `cdfl`, `cdlz`, `cdzl`, `cdzs`
- `CHD_DVD_HUNK_SIZE`: The CHD hunk size in bytes for DVDs, defaults to auto, valid range: `16-1048576`
- `CHD_DVD_COMPRESSION_ALGORITHMS`: The CHD compression algorithms for DVDs, up to 4 can be specified, defaults to auto, valid choices: `none`, `flac`, `huff`, `lzma`, `zlib`, `zstd`
- `CHD_PARENTS`: Enables the CHD parents feature, needs playlists to have been generated, defaults to `false`
- `RVZ_BLOCK_SIZE`: The RVZ block size in KiB, defaults to `128`, valid range: `32-2048`
- `RVZ_COMPRESSION_ALGORITHM`: The RVZ compression algorithm, defaults to `zstd`, valid choices: `none`, `zstd`, `bzip`, `lzma`, `lzma2`
- `RVZ_COMPRESSION_LEVEL`: The RVZ compression level, defaults to `5`, valid ranges: `1-22` for zstd, `1-9` for the other algorithms
- `RVZ_SCRUB`: Enables RVZ scrubbing, applies only to `export-roms`, defaults to `false`
- `SEVENZIP_COMPRESSION_LEVEL`: The 7Z compression level, defaults to `9`, valid range: `1-9`
- `SEVENZIP_SOLID_COMPRESSION`: Toggles 7Z solid compression, defaults to `false`
- `ZIP_COMPRESSION_LEVEL`: The ZIP compression level, defaults to `9`, valid range: `1-9`

Note: `TMP_DIRECTORY` should have at least 8GB of free space to extract those big DVDs.

Example configuration:

```
oxyromon config -l

DISCARD_FLAGS = Aftermarket,Debug
DISCARD_RELEASES = Beta,Proto,Sample,Demo,Hack,Bootleg,Homebrew
GROUP_SUBSYSTEMS = true
HASH_ALGORITHM = crc
PREFER_FLAGS =
PREFER_PARENTS = true
PREFER_REGIONS = none
PREFER_VERSIONS = new
REGIONS_ALL = US,EU,JP
REGIONS_ALL_SUBFOLDERS = none
REGIONS_ONE = US,EU
REGIONS_ONE_STRICT = false
REGIONS_ONE_SUBFOLDERS = none
ROM_DIRECTORY = /home/alucryd/Emulation
RVZ_COMPRESSION_ALGORITHM = zstd
RVZ_COMPRESSION_LEVEL = 5
SEVENZIP_COMPRESSION_LEVEL = 9
SEVENZIP_SOLID_COMPRESSION = false
TMP_DIRECTORY = /tmp
ZIP_COMPRESSION_LEVEL = 9
```

### Directory Layout

    ${ROM_DIRECTORY}
        ...
        ⮡ ${SYSTEM_NAME} # Base directory for each system, allowed regions will be stored here
            ⮡ 1G1R # Sub directory for 1G1R games
            ⮡ Trash # Sub directory for trashed games
        ...

`${SYSTEM_NAME}` is influenced by the `GROUP_SUBSYSTEMS` setting

### External programs

These should be in your `${PATH}` for extra features.

- [7z](https://www.7-zip.org/download.html): 7Z and ZIP support
- [bchunk](https://github.com/extramaster/bchunk): CUE/BIN to ISO support
- [chdman](https://www.mamedev.org/release.html): CHD support
- [ctrtool](https://github.com/3DSGuy/Project_CTR/releases): CIA support
- [dolphin-tool](https://dolphin-emu.org/download/): RVZ support
- [flips](https://github.com/Alcaro/Flips): BPS and IPS support
- [maxcso](https://github.com/unknownbrackets/maxcso/releases): CSO/ZSO support
- [nsz](https://github.com/nicoboss/nsz): NSZ support
- [wit](https://wit.wiimm.de/): WBFS support
- [xdelta3](https://github.com/jmacd/xdelta): XDELTA support

### TODO

- Add actions to the web UI
- Find a way to automatically download No-Intro DAT files
- Support merged sets for arcade systems
- Craft some unit tests for arcade systems
- Craft some unit tests for NSZ
- Craft some unit tests for IRD and PS3 in general
- Support rebuilding PS3 ISOs using IRD files, if possible, and requested
- Add a metadata scraper in the retroarch format

## oxyromon

    Usage: oxyromon [COMMAND]

    Commands:
        info                Print system information
        config              Query and modify the oxyromon settings
        import-dats         Parse and import Logiqx DAT files into oxyromon
        download-dats       Download No-Intro and Redump DAT files and import them into oxyromon
        import-irds         Parse and import PlayStation 3 IRD files into oxyromon
        import-patches      Import patch files into oxyromon
        import-roms         Validate and import ROM files or directories into oxyromon
        sort-roms           Sort ROM files according to region and version preferences
        convert-roms        Convert ROM files between common formats
        export-roms         Export ROM files to common formats
        rebuild-roms        Rebuild arcade ROM sets according to the selected strategy
        check-roms          Check ROM files' integrity
        purge-roms          Purge trashed, missing, and orphan ROM files
        purge-systems       Purge systems
        generate-playlists  Generate M3U playlists for multi-disc games
        benchmark           Benchmark oxyromon
        server              Launch the backend server
        help                Print this message or the help of the given subcommand(s)

    Options:
        -h, --help     Print help information
        -V, --version  Print version information

## oxyromon-config

Query and modify the oxyromon settings

The settings can be queried, modified, and deleted from the command line.

    Usage: oxyromon config [OPTIONS]

    Options:
        -l, --list                  Print the whole configuration
        -g, --get <KEY>             Print a single setting
        -s, --set <KEY> <VALUE>     Set a single setting
        -u, --unset <KEY>           Unset a single setting
        -a, --add <KEY> <VALUE>     Add an entry to a list
        -r, --remove <KEY> <VALUE>  Remove an entry from a list
        -h, --help                  Print help information

## oxyromon-info

Print system information

Prints the program version, installed dependencies and their version (when possible), as well as some basic system statistics.

    Usage: oxyromon info

    Options:
    -h, --help  Print help

## oxyromon-import-dats

Parse and import Logiqx DAT files into oxyromon

The standard Logiqx XML format is supported, this includes Parent-Clone DAT files.
ZIP files such as the No-Intro Love Pack can be imported directly without extracting them first.

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

Redump offers direct downloads, but no summary, whereas No-Intro offers a summary
but no direct downloads. For now, the No-intro counterpart will only tell you if
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
Games will be considered complete, as far as oxyromon goes, even if you don't have the `PS3_CONTENT`, `PS3_EXTRA`, and `PS3_UPDATE` directories.

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
In most cases the system is auto-detected, however, you will still be prompted for the system you want when importing JB folders. You can also force specific systems by name to narrow the search. The name doesn't have to be the full name and is case-insensitive.
Systems that use a header definition require the `-s` flag to be passed to match ROM files that contain a header. This currently affects Nintendo Entertainment System (Headerless), Famicom Disc System, Atari 7800, and Atari Lynx.
Most files are moved as-is, with the exception of archives containing multiple games which are extracted.

Supported console ROM formats:

- All No-Intro and Redump supported formats
- 7Z and ZIP archives
- CHD (Compressed Hunks of Data)
- CIA (Installable 3DS title)
- CSO (Compressed ISO)
- NSZ (Compressed NSP)
- RVZ (Modern Dolphin format)
- ZSO (LZ4 Compressed ISO)
- JB folders (Extracted PS3 ISO)

Supported arcade ROM formats:

- ZIP archives
- Uncompressed folders

Note: Importing a CHD containing multiple partitions requires the matching CUE file from Redump.

    Usage: oxyromon import-roms [OPTIONS] <ROMS>...

    Arguments:
        <ROMS>...  Set the ROM files or directories to import

    Options:
        -s, --system <SYSTEM>  Select systems by name
        -t, --trash            Trash invalid ROM files
        -f, --force            Force import of existing ROM files
        -a, --hash <HASH>      Set the hash algorithm [possible values: crc, md5, sha1]
        -u, --unattended       Skip ROM files that require human intervention
        -x, --extract          Extract top-level archives before importing their contents
        -h, --help             Print help

## oxyromon-import-patches

Import patch files into oxyromon

Supported formats are BPS, IPS and XDELTA. Patches are named after the ROM files following the RetroArch softpatching naming convention.

    Usage: oxyromon import-patches [OPTIONS] <PATCHES>...

    Arguments:
    <PATCHES>...  Set the patch files to import

    Options:
    -n, --name   Customize patch names
    -f, --force  Force import of already imported patch files
    -h, --help   Print help

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
Regions are set via the `REGIONS_ALL` setting and can be overridden via the CLI `-r` flag.

In 1G1R mode, only one game from a Parent-Clone game group will be placed in the 1G1R subdirectory, by order of
precedence.
Regions are set via the `REGIONS_ONE` setting and can be overridden via the CLI `-o` flag.

In hybrid mode, the 1G1R rule applies, plus all remaining games from the selected regions will be placed in the base
directory.

1G1R and hybrid modes have an additional knob you can turn via `REGIONS_ONE_STRICT`.
Setting it to false will elect the first available ROM you possess following your region preferences.
Setting to true will elect ROMs following region preferences regardless of whether you actually posses the ROM or not.

In every mode, discarded games are placed in the `Trash` subdirectory.

1G1R and hybrid modes are still useful even without a Parent-Clone DAT file, it lets you separate games you will
actually play, while keeping original Japanese games for translation patches and other hacks.

The region format uses 2-letter codes according to [TOSEC's naming convention](https://www.tosecdev.org/tosec-naming-convention). The `Unknown` region, represented by `ZZ`, is used as a catch-all region in `REGIONS_ALL` for the hybrid mode.

    Usage: oxyromon sort-roms [OPTIONS]

    Options:
        -r, --regions <REGIONS_ALL>...
                Set the regions to keep (unordered)
            --subfolders <REGIONS_ALL_SUBFOLDERS>
                Set the subfolders scheme for games [possible values: NONE, ALPHA]
        -o, --1g1r <REGIONS_ONE>...
                Set the 1G1R regions to keep (ordered)
            --1g1r-subfolders <REGIONS_ONE_SUBFOLDERS>
                Set the subfolders scheme for 1G1R games [possible values: NONE, ALPHA]
        -w, --wanted
                Show wanted games
        -a, --all
                Sort all systems
        -y, --yes
                Automatically say yes to prompts
        -h, --help
                Print help information

## oxyromon-rebuild-roms

Rebuild arcade ROM sets according to the selected strategy

ROM sets can be rebuilt using popular merging strategies.

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

ROMs can be converted back and forth between reversible formats and their original formats.
Invoking this command will convert all eligible roms for some or all systems.
You may optionally filter games by name, the matching string is case-insensitive and can use SQL wildcards.
Systems can also be selected the same way so as to avoid being prompted for them. Both systems and game flags can be passed multiple times.

Supported ROM formats:

- All No-Intro and Redump supported formats <-> 7Z and ZIP archives
- CUE/BIN <-> CHD (Compressed Hunks of Data)
- ISO <-> CHD (Compressed Hunks of Data)
- ISO <-> CSO (Compressed ISO)
- ISO <-> ZSO (LZ4 Compressed ISO)
- ISO <-> RVZ (Modern Dolphin format)

Note: CHD will be extracted to their original split CUE/BIN where applicable.

Warning: CHD for Dreamcast requires at least chdman 0.264

    Usage: oxyromon convert-roms [OPTIONS]

    Options:
        -f, --format <FORMAT>  Set the destination format [possible values: ORIGINAL, 7Z, ZIP, CHD, CSO, RVZ, ZSO]
        -g, --game <GAME>      Select games by name
        -s, --system <SYSTEM>  Select systems by name
        -a, --all              Convert all systems/games
        -r, --recompress       Force conversion even if already in the selected format
        -d, --diff             Print size differences
        -c, --check            Check ROM files after conversion
        -p, --parents          Prompt for CHD parents
        -h, --help             Print help information

## oxyromon-export-roms

Export ROM files to common formats

Similar to `convert-roms`, however, this one leaves your original ROM files untouched, thus allowing the use of lossy formats. It is designed to export all or a subset of ROM files for use with external systems like original consoles via an EverDrive or an ODE.

Note: ISO is a variant of ORIGINAL specifically designed for OPL on PlayStation 2, it allows converting CUE/BIN CD games to ISO using bchunk.

    Usage: oxyromon export-roms [OPTIONS] --directory <DIRECTORY>

    Options:
        -f, --format <FORMAT>        Set the destination format [possible values: ORIGINAL, 7Z, ZIP, ISO, CHD, CSO, NSZ, RVZ, WBFS, ZSO]
        -g, --game <Game>            Select games by name
        -s, --system <SYSTEM>        Select systems by name
        -d, --directory <DIRECTORY>  Set the output directory
        -o, --1g1r                   Export 1G1R games only
        -h, --help                   Print help

## oxyromon-check-roms

Check ROM files' integrity

This will scan every ROM file in each specified system and move corrupt files to their respective Trash directory.
File sizes can also be computed again, useful for ROM files imported in v0.8.1 or below.

    Usage: oxyromon check-roms [OPTIONS]

    Options:
        -a, --all   Check all systems
        -g, --game <GAME>  Select games by name
        -s, --size  Recalculate ROM file sizes
        -h, --help  Print help information

## oxyromon-purge-roms

Purge trashed, missing, and orphan ROM files

This will optionally purge the database from every ROM file that has gone missing or that is not currently associated
with a ROM, as well as physically deleting all files in the `Trash` subdirectories.

    Usage: oxyromon purge-roms [OPTIONS]

    Options:
        -m, --missing  Delete missing ROM files from the database
        -o, --orphan   Delete ROM files without an associated ROM from the database
        -t, --trash    Physically delete ROM files from the trash directories
        -f, --foreign  Physically delete ROM files unknown to the database
        -y, --yes      Automatically say yes to prompts
        -h, --help     Print help

## oxyromon-purge-systems

Purge systems

This will wipe the system and all its ROMs from the database. All ROMs will be placed in the `Trash` folder, it is up to you to physically delete them afterward.

    Usage: oxyromon purge-systems

    Options:
        -h, --help  Print help information

## oxyromon-generate-playlists

Generate M3U playlists for multi-disc games

This will generate playlists to be able to swap discs from within RetroArch. Limited to Redump only.
The playlist information is also used to determine parents if you enable the CHD parents feature.

Note: `sort-roms` will move them accordingly but if you use `convert-roms` you will need to run this command again at the moment.

    Usage: oxyromon generate-playlists [OPTIONS]

    Options:
        -a, --all   Generate playlists for all systems
        -h, --help  Print help information

## oxyromon-import-irds

Parse and import PlayStation 3 IRD files into oxyromon

One of the most common ways PlayStation 3 games are dumped is as JB folders, IRD files are used to describe and validate the contents of these folders, not unlike what a DAT file does.

Note: You still need to import a PS3 DAT file from Redump or elsewhere beforehand. Please make sure it has `PlayStation 3` in the name if you don't go with Redump.

    Usage: oxyromon import-irds [OPTIONS] <IRDS>...

    Arguments:
        <IRDS>...  Set the IRD files to import

    Options:
        -i, --info   Show the IRD information and exit
        -f, --force  Force import of already imported IRD files
        -h, --help   Print help information

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
