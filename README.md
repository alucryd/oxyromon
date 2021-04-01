![CI](https://github.com/alucryd/oxyromon/workflows/CI/badge.svg)
[![codecov](https://codecov.io/gh/alucryd/oxyromon/branch/master/graph/badge.svg)](https://codecov.io/gh/alucryd/oxyromon)
[![crates.io](https://img.shields.io/crates/v/oxyromon.svg)](https://crates.io/crates/oxyromon)

# oxyromon 0.6.1

### Rusty ROM OrgaNizer

OxyROMon is a cross-platform opinionated CLI ROM organizer written in Rust.
Like most ROM managers, it checks ROM files against known good databases.
It is designed with archiving in mind, as such it only supports original and lossless ROM formats.
Sorting can be done in regions mode, in so-called 1G1R mode, or both.

### Configuration

Configuration is done from the command line and settings are stored in the SQLite database.
The database itself is stored in `${data_dir}/oxyromon` as defined in the
[dirs](https://docs.rs/dirs/3.0.1/dirs/fn.data_dir.html) crate.

Available settings:
- `ROM_DIRECTORY`: Full path to your ROM directory, defaults to `${home_dir}/Emulation` as defined in the
[dirs](https://docs.rs/dirs/3.0.1/dirs/fn.home_dir.html) crate
- `TMP_DIRECTORY`: Full path to a temporary directory for file extraction, defaults to
[temp_dir](https://doc.rust-lang.org/std/env/fn.temp_dir.html)
- `DISCARD_BETA`: Discard beta ROMs
- `DISCARD_CASTLEVANIA_ANNIVERSARY_COLLECTION`: Discard Castlevania Anniversary Collection ROMs
- `DISCARD_CLASSIC_MINI`: Discard Classic Mini ROMs
- `DISCARD_DEBUG`: Discard debug ROMs
- `DISCARD_DEMO`: Discard demo ROMs
- `DISCARD_GOG`: Discard GOG ROMs
- `DISCARD_PROGRAM`: Discard program ROMs
- `DISCARD_PROTO`: Discard prototype ROMs
- `DISCARD_SAMPLE`: Discard sample ROMs
- `DISCARD_SEGA_CHANNEL`: Discard Sega Channel ROMs
- `DISCARD_SNES_MINI`: Discard SNES Mini ROMs
- `DISCARD_SONIC_CLASSIC_COLLECTION`: Discard Sonic Classic Collection ROMs
- `DISCARD_SWITCH_ONLINE`: Discard Switch Online ROMs
- `DISCARD_VIRTUAL_CONSOLE`: Discard Virtual Console ROMs
- `DISCARD_WII`: Discard Wii ROMs

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

- Support RVZ

## oxyromon

    USAGE:
        oxyromon [SUBCOMMAND]

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    SUBCOMMANDS:
        help            Prints this message or the help of the given subcommand(s)
        config          Queries and modifies the oxyromon settings
        import-dats     Parses and imports No-Intro and Redump DAT files into oxyromon
        import-roms     Validates and imports ROM files into oxyromon
        sort-roms       Sorts ROM files according to region and version preferences
        convert-roms    Converts ROM files between common formats
        check-roms      Checks ROM files integrity
        purge-roms      Purges deleted or moved ROM files

## oxyromon-config

Queries and configures the oxyromon settings

The settings can be queried, modified and deleted from the command line.

    USAGE:
        oxyromon config [FLAGS] [OPTIONS]

    FLAGS:
        -l, --list       Prints the whole configuration
        -h, --help       Prints help information
        -V, --version    Prints version information

    OPTIONS:
        -d, --delete <KEY>         Deletes a single setting
        -g, --get <KEY>            Prints a single setting
        -s, --set <KEY> <VALUE>    Configures a single setting

## oxyromon-import-dats

Parses and imports No-Intro and Redump DAT files into oxyromon

The standard Logiqx XML format is supported, this includes Parent-Clone DAT files.

Supported DAT providers:
* No-Intro
* Redump

Note: Some systems require a header definition to be placed alongside the DAT file.

    USAGE:
        oxyromon import-dats [FLAGS] <DATS>...

    FLAGS:
        -i, --info       Shows the DAT information and exit
        -h, --help       Prints help information
        -V, --version    Prints version information

    ARGS:
        <DATS>...    Sets the DAT files to import

## oxyromon-import-roms

Validates and imports ROM files into oxyromon

ROM files that match against the database will be placed in the base directory of the system they belong to. 
You will be prompted for the system you want to check your ROMs against.
Most files will be moved as-is, with the exception of archives containing multiple games which are extracted.

Supported ROM formats:
* All No-Intro and Redump supported formats
* 7Z and ZIP archives
* CHD (Compressed Hunks of Data)
* CSO (Compressed ISO)

Note: Importing a CHD requires the matching CUE file from Redump.

    USAGE:
        oxyromon import-roms <ROMS>...

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    ARGS:
        <ROMS>...    Sets the rom files to import

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
In 1G1R mode, only one game from a Parent-Clone game group will be placed in the 1G1R subdirectory, by order of
precedence.
In hybrid mode, the 1G1R rule applies, plus all remaining games from the selected regions will be placed in the base
directory.
In every mode, discarded games are placed in the Trash subdirectory.

1G1R and hybrid modes are still useful even without a Parent-Clone DAT file, it lets you separate games you will
actually play, while keeping original Japanese games for translation patches and other hacks.

The region format uses 2-letter codes according to [TOSEC's naming convention](https://www.tosecdev.org/tosec-naming-convention).

    USAGE:
        oxyromon sort-roms [FLAGS] [OPTIONS]

    FLAGS:
        -a, --all                        Sorts all systems
        -m, --missing                    Shows missing games
            --without-beta               Discards beta games
            --without-debug              Discards debug games
            --without-demo               Discards demo games
            --without-program            Discards program games
            --without-proto              Discards prototype games
            --without-sample             Discards sample games
            --with-beta                  Keeps beta games
            --with-debug                 Keeps debug games
            --with-demo                  Keeps demo games
            --with-program               Keeps program games
            --with-proto                 Keeps prototype games
            --with-sample                Keeps sample games
            --with-sega-channel          Keeps sega channel games
            --with-virtual-console       Keeps virtual console games
        -y, --yes                        Automatically says yes to prompts
        -h, --help                       Prints help information
        -V, --version                    Prints version information

    OPTIONS:
        -g, --1g1r <1G1R>...          Sets the 1G1R regions to keep (ordered)
        -r, --regions <REGIONS>...    Sets the regions to keep (unordered)
    
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

* All No-Intro and Redump supported formats <-> 7Z and ZIP archives
* CUE/BIN <-> CHD (Compressed Hunks of Data)
* ISO <-> CSO (Compressed ISO)

Note: CHD will be extracted to their original split CUE/BIN when applicable.

    USAGE:
        oxyromon convert-roms [FLAGS] [OPTIONS]

    FLAGS:
        -a, --all        Converts all systems/all ROMs
        -h, --help       Prints help information
        -V, --version    Prints version information

    OPTIONS:
        -f, --format <FORMAT>    Sets the destination format [possible values: 7Z, CHD, CSO, ORIGINAL, ZIP]
        -n, --name <NAME>        Selects ROMs by name

## oxyromon-check-roms

Checks ROM files integrity

This will scan every ROM file in each specified system and move corrupt files to their respective Trash directory.

    USAGE:
        oxyromon check-roms [FLAGS]

    FLAGS:
        -a, --all        Checks all systems
        -y, --yes        Automatically says yes to prompts
        -h, --help       Prints help information
        -V, --version    Prints version information

## oxyromon-purge-roms 

Purges trashed and missing ROM files

This will purge the database from every ROM file that has gone missing, as well as optionally delete all games in the
Trash subdirectories.

    USAGE:
        oxyromon purge-roms [FLAGS]

    FLAGS:
        -y, --yes            Automatically says yes to prompts
        -t, --empty-trash    Empties the ROM files trash directories
        -h, --help           Prints help information
        -V, --version        Prints version information
