# 0.19.0
- Traits! Makes parts of the code actually reusable
- Add an optional post-conversion check
- Fix importing a couple more bogus DAT files
- Accept ZIP files as input for `import-dats`
- Add a `-u` flag to `import-roms` to skip user prompts
- Add a `-s` flag to `convert-roms` to select systems by name 
- Update the `-s` flag in `import-roms` to behave the same as in `convert-roms`
- Add a `-f` flag in `purge-roms` to track and delete foreign files in the ROM directory
- Add a new `export-roms` subcommand
- Add support for WBFS in `export-roms`

# 0.18.0

- Replace `async-std` with `tokio`
- Add an `info` subcommand to display basic system and dependencies information
- Only generate playlists for complete sets of games
- Exit gracefully in most places when missing an external program
- Use `7zz` instead of `7z` on mac
- Don't silently delete files in `check-roms`
- Add support for ZSO
- Add optional dependency on bchunk
- Allow filtering games by name in `check-roms`

# 0.17.0

- Clean temporary files immediately in `convert-roms`
- Include Non-Redump DATs in the grouped subsystems
- Don't create empty directories
- Fix paths displayed on Windows
- Add support for importing CIA files (@leo60228)

# 0.16.0

- Use original names for PSN stuff
- Add a language whitelist setting
- Always name archives after the game name
- Add support for NSZ
- Expose most settings in the web UI
- Make ignored status take precedence over complete status in the web UI
- Additionally match the whole flag string to allow discarding specific flag combinations
- Make `purge-roms` physically delete orphan ROMs

# 0.15.0

- Use archive name and directory names in the archive when importing arcade games
- Also use the file name in the archive when importing arcade games
- Rework import-roms to only ask for a system when desired or necessary
- Rework rebuild-roms to make it a lot faster
- Fix completion computation for arcade systems
- Don't automatically trash invalid roms, put the mechanic behind a flag
- Add a flag to force import existing roms
- Use native rust implementations by default (openssl-sys -> rustls, libz-sys -> miniz_oxide)

# 0.14.1

- Fix game filtering in the web UI with the new 1G1R system

# 0.14.0

- Add the ability to sort ROMs in alphabetical subfolders
- Add a catch-all region `ZZ` in `REGIONS_ALL` for the `sort-roms` hybrid mode
- Add a `GROUP_SUBSYSTEMS` toggle, if true merge variants of the same system in a single directory
- Support MAME DAT files that use non-standard `machine` instead of `game` tags
- Add various compression settings for 7Z, ZIP and RVZ
- Add a new `generate-playlists` subcommand
- Fix pure 1G1R sorting with parent-clone groups that have no ROMs
- Add a `REGIONS_ONE_STRICT` option to switch between strict and lenient 1G1R election
- Fix 1G1R sorting when revisions are clones
- Display an appropriate message when a ROM has already been imported
- Add a `PREFER_FLAGS` to give a boost to specific flags in the 1G1R election process
- Add `PREFER_PARENTS`, `PREFER_REGIONS` and `PREFER_VERSIONS` settings to influence the 1G1R election process

# 0.13.0

- Remove the ROM original extension from archives, it makes RetroArch name its saves differently between imported and non imported games
- Fix the `sort-roms` prompt to proceed
- Allow creating solid 7z archives
- Skip games with unparseable names in `import-dats`
- Fix successive import of invalid ROMs with the same name in `import-roms`
- Greatly speed up `purge-systems`

# 0.12.0

- Change `--missing` to `--wanted` in `sort-roms` to avoid confusion with the same `purge-roms` flag
- Add a `purge-systems` subcommand (WIP)

# 0.11.1

- Fix import-roms CLI flags
- Fix import-roms 7Z CRC parsing

# 0.11.0

- Put CHD, CSO and RVZ support behind features
- Add support for PS3 IRD files, along with a new import-irds subcommand
- Add support for PS3 updates and DLCs
- Allow selecting the checksum algorithm in import-roms (useful for JB folders which only provide MD5)
- Add a new benchmark subcommand (Linux only for now) to measure the performance of checksum algorithms
- Support moving files across different filesystems
- Fix a parsing issue on DAT files with duplicate `clrmamepro` fields

# 0.10.1

- Fix RVZ conversion

# 0.10.0

- Fix version sorting in 1G1R when parent is missing or unwanted
- Treat the first elligible clone as parent in 1G1R when parent is missing
- Greatly improve completion calculation in sort-roms
- Add support for the Atari 7800 header definition
- Embed some no-intro header definitions and use them as fallback
- Add initial support for arcade systems
- Add a new rebuild-roms subcommand for arcade ROM sets
- Add support for RVZ via dolphin-tool

# 0.9.0

- Use transactions for increased database performance
- Add an optional GraphQL API and a basic web UI, behind the `server` feature
- Store ROM files' actual size in database
- Store system and game completion status in database
- Store game sorting in database (all regions, one region, ignored)

# 0.8.1

- Use a database connection pool
- Fix importing archives with invalid files

# 0.8.0

- Add a new download-dats subcommand
- Use dialoguer for prompts
- Support importing ISO compressed as CHD
- Support converting between ISO and CHD
- Support converting directly between supported formats (as opposed to having to revert to original beforehand)
- Optionally print statistics after each conversion

# 0.7.0

- Use shiratsu_naming to parse No-Intro names
- Drop releases, we don't need them
- Delete obsolete roms when importing updated dats and automatically reimport orphan romfiles
- Move failed imports to the trash directories
- Fix headered ROMs handling
- Add a unique constraint to the settings::key column
- Simplify discard settings, please refer to the new documentation
- Remove the ability to delete a setting
- Allow purge-roms to delete orphan romfiles

**WARNING** The internal region format is now TOSEC's, all dats need to be reimported for this change to take effect.

# 0.6.0

- Replace refinery with sqlx migrate
- Add a check-roms subcommand

# 0.5.0

- Filter ROMs by name in convert-roms

# 0.4.0

- Replace diesel with sqlx and refinery
- Use async_std
- Add a config subcommand and use the database to store the settings
- Add unit tests

# 0.3.0

- Add progress indicators and bars
- Add dependency on the indicatif crate

# 0.2.2

- Cleanup files on unsuccessful matches

# 0.2.1

- Fix documentation

# 0.2.0

- Deprecate environment variables in favor of db configuration
- New associated config subcommand
- Add dependency on the dirs crate

# 0.1.0

- Initial release
