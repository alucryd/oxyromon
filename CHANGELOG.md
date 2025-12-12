# 0.21.0

## Features

- Added sorting options for arcade systems
- Added `-e/--extension` flag to `import-dats` for setting custom extensions on original ROMs in the system
- Added support for exporting Dreamcast Redump images to GDI format in `export-roms` (based on gdidrop, credits to feyris-tan)
- Added `-d/--delete` flag to `import-roms` for hard deletion of unmatched and duplicate ROMs (alternative to `--trash`)
- Added `-s/--save` flag to `download-dats` for optionally saving downloaded DAT files to a specified directory
- Added purge system buttons in the frontend
- Added automatic detection of CHD parents in `import-roms`, provided they are in the same directory
- Added a romfiles card to the frontend

## Changes

- Retired the hash algorithm override in `import-roms`
- Repurposed the `-a` flag in `import-roms` to import files as-is (now supports non-original formats in DAT files)
- Improved ROM matching in unattended mode using similarity scoring
- Rewrote the frontend using Svelte/Flowbite

## Fixes

- Fixed an infinite loop when importing DAT files containing invalid parent references
- Fixed playlist names for games having stuff after the disc number
- Fix JB folder game completion

# 0.20.2

## Features

- Added a new `wanted` game state in the web UI (replaces `incomplete`, which now means games with some but not all ROMs)
- Added a `-f` flag to `rebuild-roms` to force rebuilding with the same strategy

## Fixes

- Fixed a regression when importing multi-track CHDs containing tracks identical to other games
- Fixed parsing of archives containing files with `=` signs in their names
- Fixed several issues affecting MAME DAT imports
- Fixed regressions in `rebuild-roms`

# 0.20.1

## Changes

- Arcade CHDs are now left untouched in `convert-roms` and `rebuild-roms`

## Fixes

- Fixed an import issue with Progetto MAME DATs
- Fixed a regression preventing directory imports
- Fixed a crash in `sort-roms` when ROMs have no extension

# 0.20.0

## Features

- Added `-x` flag to `import-roms` to extract top-level archives before importing their contents
- Added new `import-patches` subcommand
- Added new `create-dats` subcommand
- Added support for CHD HD and LD formats
- Added support for `disk` tags in MAME CHD DATs
- Added support for DATs with no size information (such as MAME CHD DATs)
- Now allows ordering CHD compression algorithms
- Now allows importing multi-track CHDs without a CUE file (requires chdman 0.265+)
- Now supports importing MAME CHDs based on the CHD SHA1 contained in the DAT (not the actual data SHA1)

## Improvements

- Replaced `isoinfo` with the native `cdfs` crate for parsing IRD ISO headers
- Made the ROM directory portable by storing relative paths internally
- Enhanced CHD processing with the new `splitbin` feature in chdman 0.265+ (fixes Dreamcast CHD imports)
- Implemented mimetype inference using file magic where possible
- Database is now optimized upon exit
- CHD data SHA1 is now parsed and matched first where applicable when importing CHDs
- All hash algorithms are now iterated over when matching and checking ROMs in all subcommands
- MAME DATs are now auto-detected as arcade systems
- `GROUP_SUBSYSTEMS` setting now applies to custom system names as well

## Changes

- Removed the `-a` flag from the `import-dats` subcommand
- Removed the `HASH_ALGORITHM` setting

## Fixes

- Fixed special character handling in 7z

# 0.19.0

## Features

- Added `-n` flag to `import-dats` to override the system name
- Added `-r` flag to `convert-roms` to recompress files that already match the target format
- Added `-u` flag to `config` to unset nullable settings
- Added `-p` flag to `convert-roms` to prompt for an optional CHD parent when converting to CHD
- Added configuration options for chdman
- Added support for CHD parents (enable via `CHD_PARENTS` setting)
- Added option to scrub RVZ in `export-roms`

## Improvements

- Enhanced ROM matching algorithm in `import-roms` to reduce prompts when multiple matches are found
- CHD parent prompt is now sorted by distance
- IRD game matching prompt improved when IRD game name is all caps and Redump name is not
- Now uses `createcd/createdvd` and `extractcd/extractdvd` appropriately for CDs and DVDs
- Added support for both `7zz` and `7z` on all systems (in that order)

## Changes

- Changed `-s` in various subcommands to accept SQL wildcards
- Changed `-n` to `-g` (for game) in various subcommands and made it accept SQL wildcards
- Changed `-g` to `-o` (for one game one ROM) in various subcommands

## Fixes

- Fixed reimporting orphan archives containing multiple files or CHD ROMs in `import-dats` and `download-dats`
- Fixed converting archives to another archive format
- Fixed an issue where converting an archive back to original would only delete the archive

# 0.18.1

## Changes

- Bumped minimum required chdman version to 0.264 for Dreamcast support

## Fixes

- Fixed Dreamcast system being skipped even with the correct chdman version

# 0.18.0

## Features

- Added `info` subcommand to display basic system and dependency information
- Added support for ZSO format
- Added optional dependency on bchunk
- Added optional post-conversion check
- Added `-u` flag to `import-roms` to skip user prompts
- Added `-s` flag to `convert-roms` to select systems by name
- Added `-f` flag to `purge-roms` to track and delete foreign files in the ROM directory
- Added new `export-roms` subcommand
- Added support for WBFS in `export-roms`

## Improvements

- Replaced `async-std` with `tokio`
- Graceful exit in most places when missing an external program
- Uses `7zz` instead of `7z` on macOS
- Playlists are now only generated for complete sets of games
- Game filtering by name is now allowed in `check-roms`
- Introduced traits to make parts of the code reusable
- ZIP files are now accepted as input for `import-dats`
- Updated `-s` flag behavior in `import-roms` to match `convert-roms`

## Changes

- Files are no longer silently deleted in `check-roms`

## Fixes

- Fixed importing a couple more bogus DAT files

# 0.17.0

## Features

- Added support for importing CIA files (@leo60228)

## Improvements

- Temporary files are now cleaned immediately in `convert-roms`
- Non-Redump DATs are now included in grouped subsystems

## Changes

- Empty directories are no longer created

## Fixes

- Fixed paths displayed on Windows

# 0.16.0

## Features

- Added language whitelist setting
- Added support for NSZ format

## Improvements

- PSN content now uses original names
- Archives are now always named after the game name
- Most settings are now exposed in the web UI
- Ignored status now takes precedence over complete status in the web UI
- Enhanced flag matching to allow discarding specific flag combinations
- `purge-roms` now physically deletes orphan ROMs

# 0.15.0

## Features

- Added flag to force import of existing ROMs

## Improvements

- Archive names and directory names within archives are now used when importing arcade games
- File names within archives are also used when importing arcade games
- Reworked `import-roms` to only ask for a system when desired or necessary
- Reworked `rebuild-roms` for significantly improved performance
- Invalid ROMs are no longer automatically trashed (now behind a flag)
- Now uses native Rust implementations by default (openssl-sys → rustls, libz-sys → miniz_oxide)

## Fixes

- Fixed completion computation for arcade systems

# 0.14.1

## Fixes

- Fixed game filtering in the web UI with the new 1G1R system

# 0.14.0

## Features

- Added ability to sort ROMs in alphabetical subfolders
- Added catch-all region `ZZ` in `REGIONS_ALL` for `sort-roms` hybrid mode
- Added `GROUP_SUBSYSTEMS` toggle to merge variants of the same system in a single directory
- Added compression settings for 7Z, ZIP and RVZ formats
- Added new `generate-playlists` subcommand
- Added `REGIONS_ONE_STRICT` option to switch between strict and lenient 1G1R election
- Added `PREFER_FLAGS` setting to boost specific flags in the 1G1R election process
- Added `PREFER_PARENTS`, `PREFER_REGIONS` and `PREFER_VERSIONS` settings to influence 1G1R election

## Improvements

- Now supports MAME DAT files using non-standard `machine` tags instead of `game` tags
- Displays appropriate message when a ROM has already been imported

## Fixes

- Fixed pure 1G1R sorting with parent-clone groups that have no ROMs
- Fixed 1G1R sorting when revisions are clones

# 0.13.0

## Features

- Added ability to create solid 7z archives

## Improvements

- Greatly improved performance of `purge-systems`
- ROM original extension is now removed from archives (fixes RetroArch save naming inconsistencies)
- Games with unparseable names are now skipped in `import-dats`

## Fixes

- Fixed the `sort-roms` prompt to proceed
- Fixed successive import of invalid ROMs with the same name in `import-roms`

# 0.12.0

## Features

- Added `purge-systems` subcommand (WIP)

## Changes

- Changed `--missing` to `--wanted` in `sort-roms` to avoid confusion with the `purge-roms` flag

# 0.11.1

## Fixes

- Fixed `import-roms` CLI flags
- Fixed `import-roms` 7Z CRC parsing

# 0.11.0

## Features

- Added support for PS3 IRD files with new `import-irds` subcommand
- Added support for PS3 updates and DLCs
- Added ability to select checksum algorithm in `import-roms` (useful for JB folders with MD5-only)
- Added new `benchmark` subcommand to measure checksum algorithm performance (Linux only)
- Put CHD, CSO and RVZ support behind feature flags

## Improvements

- Now supports moving files across different filesystems

## Fixes

- Fixed parsing issue with DAT files containing duplicate `clrmamepro` fields

# 0.10.1

## Fixes

- Fixed RVZ conversion

# 0.10.0

## Features

- Added support for Atari 7800 header definition
- Added initial support for arcade systems
- Added new `rebuild-roms` subcommand for arcade ROM sets
- Added support for RVZ via dolphin-tool
- Embedded some No-Intro header definitions for use as fallbacks

## Improvements

- Greatly improved completion calculation in `sort-roms`
- First eligible clone is now treated as parent in 1G1R when parent is missing

## Fixes

- Fixed version sorting in 1G1R when parent is missing or unwanted

# 0.9.0

## Features

- Added optional GraphQL API and basic web UI (behind `server` feature)

## Improvements

- Database performance increased through use of transactions
- ROM files' actual size is now stored in database
- System and game completion status is now stored in database
- Game sorting information is now stored in database (all regions, one region, ignored)

# 0.8.1

## Improvements

- Now uses database connection pool

## Fixes

- Fixed importing archives with invalid files

# 0.8.0

## Features

- Added new `download-dats` subcommand
- Added support for importing ISO compressed as CHD
- Added support for converting between ISO and CHD formats
- Added support for direct conversion between supported formats (no need to revert to original first)
- Added option to print statistics after each conversion

## Improvements

- Now uses dialoguer for prompts

# 0.7.0

## Features

- Added `purge-roms` capability to delete orphan ROM files

## Improvements

- Now uses shiratsu_naming to parse No-Intro names
- Obsolete ROMs are now deleted when importing updated DATs and orphan ROM files are automatically reimported
- Failed imports are now moved to trash directories
- Discard settings have been simplified (refer to new documentation)
- Added unique constraint to settings::key column

## Changes

- Dropped releases (no longer needed)
- Removed ability to delete settings

## Fixes

- Fixed headered ROM handling

**WARNING** The internal region format is now TOSEC's, all DATs need to be reimported for this change to take effect.

# 0.6.0

## Improvements

- Replaced refinery with sqlx migrate

## Features

- Added `check-roms` subcommand

# 0.5.0

## Features

- Added ROM filtering by name in `convert-roms`

# 0.4.0

## Improvements

- Replaced diesel with sqlx and refinery
- Now uses async_std

## Features

- Added `config` subcommand with database-stored settings
- Added unit tests

# 0.3.0

## Features

- Added progress indicators and bars
- Added dependency on indicatif crate

# 0.2.2

## Fixes

- Fixed cleanup of files on unsuccessful matches

# 0.2.1

## Fixes

- Fixed documentation

# 0.2.0

## Features

- Added new `config` subcommand
- Added dependency on dirs crate

## Changes

- Deprecated environment variables in favor of database configuration

# 0.1.0

- Initial release
