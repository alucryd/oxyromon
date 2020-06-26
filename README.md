# oxyromon 0.1.0

Rusty ROM OrgaNizer

    USAGE:
        oxyromon [SUBCOMMAND]

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    SUBCOMMANDS:
        convert-roms    Converts ROM files between common formats
        help            Prints this message or the help of the given subcommand(s)
        import-dats     Parses and imports No-Intro and Redump DAT files into oxyromon
        import-roms     Validates and imports ROM files into oxyromon
        purge-roms      Purges deleted or moved ROM files
        sort-roms       Sorts ROM files according to region and version preferences

## oxyromon-import-dats

Parses and imports No-Intro and Redump DAT files into oxyromon

    USAGE:
        oxyromon import-dats [FLAGS] <DATS>...

    FLAGS:
        -i, --info       Show the DAT information and exit
        -h, --help       Prints help information
        -V, --version    Prints version information

    ARGS:
        <DATS>...    Sets the DAT files to import

## oxyromon-import-roms

Validates and imports ROM files into oxyromon

    USAGE:
        oxyromon import-roms <ROMS>...

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    ARGS:
        <ROMS>...    Sets the rom files to import

## oxyromon-convert-roms

Converts ROM files between common formats

    USAGE:
        oxyromon convert-roms [OPTIONS]

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    OPTIONS:
        -f, --format <FORMAT>    Sets the destination format [possible values: 7Z, CHD, ORIGINAL, ZIP]


## oxyromon-sort-roms 

Sorts ROM files according to region and version preferences

    USAGE:
        oxyromon sort-roms [FLAGS] [OPTIONS]

    FLAGS:
        -a, --all                   Sorts all systems
        -m, --missing               Shows missing games
            --no-beta               Discards beta games
            --no-debug              Discards debug games
            --no-demo               Discards demo games
            --no-program            Discards program games
            --no-proto              Discards prototype games
            --no-sample             Discards sample games
            --no-sega-channel       Discards sega channel games
            --no-virtual-console    Discards virtual console games
        -y, --yes                   Automatically says yes to prompts
        -h, --help                  Prints help information
        -V, --version               Prints version information

    OPTIONS:
        -g, --1g1r <1G1R>...          Sets the 1G1R regions to keep (ordered)
        -r, --regions <REGIONS>...    Sets the regions to keep (unordered)

## oxyromon-purge-roms 

Purges trashed and missing ROM files

    USAGE:
        oxyromon purge-roms [FLAGS]

    FLAGS:
        -y, --yes            Automatically says yes to prompts
        -t, --empty-trash    Empties the ROM files trash directories
        -h, --help           Prints help information
        -V, --version        Prints version information
