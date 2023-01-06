# 0.14.0

- Add the ability to sort ROMs in alphabetical subfolders
- Add a catch-all region `ZZ` in `REGIONS_ALL` for the `sort-roms` hybrid mode
- Add a `GROUP_SUBSYSTEMS` toggle, if true merge variants of the same system in a single directory
- Support MAME DAT files that use non-standard `machine` instead of `game` tags
- Add various compression settings for 7Z, ZIP and RVZ
- Add a new `generate-playlists` subcommand

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
