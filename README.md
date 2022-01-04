![CI](https://github.com/alucryd/oxyromon/workflows/CI/badge.svg)
[![codecov](https://codecov.io/gh/alucryd/oxyromon/branch/master/graph/badge.svg)](https://codecov.io/gh/alucryd/oxyromon)
[![crates.io](https://img.shields.io/crates/v/oxyromon.svg)](https://crates.io/crates/oxyromon)

# oxyromon 0.10.0

### Rusty ROM OrgaNizer

OxyROMon is a cross-platform opinionated CLI ROM organizer written in Rust.
Like most ROM managers, it checks ROM files against known good databases.
It is designed with archiving in mind, as such it only supports original and lossless ROM formats.
Sorting can be done in regions mode, in so-called 1G1R mode, or both.

### Compilation

The CLI has no specific requirement, you can just:

    cargo build --release

For the web UI, you will also need yarn:

    yarn install
    yarn run build
    cargo build --release --all-features

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

### Directory Layout

    ${ROM_DIRECTORY}
        ...
        ⮡ ${SYSTEM_NAME} # Base directory for each system, allowed regions will be stored here
            ⮡ 1G1R # Sub directory for 1G1R games
            ⮡ Trash # Sub directory for trashed games
        ...

### External programs

These should be in your `${PATH}` for extra features.

- 7z: 7Z and ZIP support
- chdman: CHD support
- maxcso: CSO support

### TODO

- Automatically create m3u files for multi-discs games
- Add actions to the web UI
- Add an optional check of the ROMs after conversion
- Support RVZ when dolphin adds it to its CLI (or NKit, whichever comes first)
- Find a way to automatically download No-Intro DAT files (just made harder by asking users to click on a color...)
- Support all merge options for arcade systems

## oxyromon

    USAGE:
        oxyromon [SUBCOMMAND]

    OPTIONS:
        -h, --help       Print help information
        -V, --version    Print version information

    SUBCOMMANDS:
        help             Print this message or the help of the given subcommand(s)
        config           Queries and modifies the oxyromon settings
        import-dats      Parses and imports No-Intro and Redump DAT files into oxyromon
        download-dats    Downloads No-Intro and Redump DAT files and imports them into oxyromon
        import-roms      Validates and imports ROM files into oxyromon
        sort-roms        Sorts ROM files according to region and version preferences
        convert-roms     Converts ROM files between common formats
        check-roms       Checks ROM files integrity
        purge-roms       Purges trashed, missing and orphan ROM files
        server           Launches the backend server

## oxyromon-config

Queries and configures the oxyromon settings

The settings can be queried, modified and deleted from the command line.

    USAGE:
        oxyromon config [OPTIONS]

    OPTIONS:
        -a, --add <KEY> <VALUE>       Adds an entry to a list
        -g, --get <KEY>               Prints a single setting
        -h, --help                    Print help information
        -l, --list                    Prints the whole configuration
        -r, --remove <KEY> <VALUE>    Removes an entry from a list
        -s, --set <KEY> <VALUE>       Configures a single setting

## oxyromon-import-dats

Parses and imports No-Intro and Redump DAT files into oxyromon

The standard Logiqx XML format is supported, this includes Parent-Clone DAT files.

Supported DAT providers:

- No-Intro
- Redump

Note: Some systems require a header definition to be placed alongside the DAT file.

    USAGE:
        oxyromon import-dats [OPTIONS] <DATS>...

    ARGS:
        <DATS>...    Sets the DAT files to import

    OPTIONS:
        -a, --arcade         Toggles arcade mode
        -f, --force          Forces import of outdated DAT files
        -h, --help           Print help information
        -i, --info           Shows the DAT information and exit
        -s, --skip-header    Skips parsing the header even if the system has one

## oxyromon-download-dats

Downloads No-Intro and Redump DAT files and imports them into oxyromon

Redump ofers direct downloads, but no summary, whereas No-Intro offers a summary
but no direct downloads. For now the No-intro counterpart will only tell you if
an update is available, but the Redump one is able to download brand new dats
and update those you've already imported.

Supported DAT providers:

- Redump (Download and update)
- No-Intro (Update check only)

  USAGE:
  oxyromon download-dats [OPTIONS]

  OPTIONS:
  -a, --all Imports all systems
  -f, --force Forces import of outdated DAT files
  -h, --help Print help information
  -n, --nointro Downloads No-Intro DAT files
  -r, --redump Downloads Redump DAT files
  -u, --update Checks for system updates

## oxyromon-import-roms

Validates and imports ROM files into oxyromon

ROM files that match against the database will be placed in the base directory of the system they belong to.
You will be prompted for the system you want to check your ROMs against.
Most files will be moved as-is, with the exception of archives containing multiple games which are extracted.

Supported ROM formats:

- All No-Intro and Redump supported formats
- 7Z and ZIP archives
- CHD (Compressed Hunks of Data)
- CSO (Compressed ISO)

Note: Importing a CHD containing multiple partitions requires the matching CUE file from Redump.

    USAGE:
        oxyromon import-roms [OPTIONS] <ROMS>...

    ARGS:
        <ROMS>...    Sets the ROM files to import

    OPTIONS:
        -h, --help               Print help information
        -s, --system <SYSTEM>    Sets the system number to use

## oxyromon-sort-roms

Sorts ROM files according to region and version preferences

Sorting can be done using several strategies.
You can also choose to discard certain types of games.
Optionally you can print a list of games you may be missing, you hoarder, you.

Supported modes:

- Regions mode
- 1G1R mode
- Hybrid mode

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

    USAGE:
        oxyromon sort-roms [OPTIONS]

    OPTIONS:
        -a, --all                         Sorts all systems
        -g, --1g1r <REGIONS_ONE>...       Sets the 1G1R regions to keep (ordered)
        -h, --help                        Print help information
        -m, --missing                     Shows missing games
        -r, --regions <REGIONS_ALL>...    Sets the regions to keep (unordered)
        -y, --yes                         Automatically says yes to prompts

    EXAMPLE:
        oxyromon sort-roms -g US EU -r US EU JP

## oxyromon-convert-roms

Converts ROM files between common formats

ROMs can be converted back and forth at most one format away from their original format.
That means you can convert an ISO to a CSO, but not a CSO to a 7Z archive.
Invoking this command will convert all eligible roms for some or all systems.
You may optionally filter ROMs by name, the matching string is not case sensitive and doesn't need to be the full ROM
name.

Supported ROM formats:

- All No-Intro and Redump supported formats <-> 7Z and ZIP archives
- CUE/BIN <-> CHD (Compressed Hunks of Data)
- ISO <-> CHD (Compressed Hunks of Data)
- ISO <-> CSO (Compressed ISO)

Note: CHD will be extracted to their original split CUE/BIN when applicable.

    USAGE:
        oxyromon convert-roms [OPTIONS]

    OPTIONS:
        -a, --all                Converts all systems/all ROMs
        -f, --format <FORMAT>    Sets the destination format [possible values: 7Z, CHD, CSO, ORIGINAL,
                                ZIP]
        -h, --help               Print help information
        -n, --name <NAME>        Selects ROMs by name
        -s, --statistics         Prints statistics for each conversion

## oxyromon-check-roms

Checks ROM files integrity

This will scan every ROM file in each specified system and move corrupt files to their respective Trash directory.
File sizes can also be computed again, useful for ROM files imported in v0.8.1 or below.

    USAGE:
        oxyromon check-roms [OPTIONS]

    OPTIONS:
        -a, --all       Checks all systems
        -h, --help      Print help information
        -r, --repair    Repairs arcade ROM files when possible
        -s, --size      Recalculates ROM file sizes

## oxyromon-purge-roms

Purges trashed, missing and orphan ROM files

This will optionally purge the database from every ROM file that has gone missing or that is not currently associated
with a ROM, as well as physically delete all files in the `Trash` subdirectories.

    USAGE:
        oxyromon purge-roms [OPTIONS]

    OPTIONS:
        -h, --help       Print help information
        -m, --missing    Deletes missing ROM files from the database
        -o, --orphan     Deletes ROM files without an associated ROM from the database
        -t, --trash      Physically deletes ROM files from the trash directories
        -y, --yes        Automatically says yes to prompts

## oxyromon-server

Launches the backend server

The server exposes a GraphQL API endpoint at `/graphql`. An associated Svelte.js web UI is also exposed at `/`.

    USAGE:
        oxyromon server [OPTIONS]

    OPTIONS:
        -a, --address <ADDRESS>    Specifies the server address [default: 127.0.0.1]
        -h, --help                 Print help information
        -p, --port <PORT>          Specifies the server port [default: 8000]
