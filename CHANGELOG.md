# 0.9.0
* Use transactions for increased performance
* Add an optional GraphQL API
* Support CORS
* Add a basic web UI

# 0.8.1
* Use a database connection pool
* Fix importing archives with invalid files

# 0.8.0
* Add a new download-dats subcommand
* Use dialoguer for prompts
* Support importing ISO compressed as CHD
* Support converting between ISO and CHD
* Support converting directly between supported formats (as opposed to having to revert to original beforehand)
* Optionally print statistics after each conversion

# 0.7.0
* Use shiratsu_naming to parse No-Intro names
* Drop releases, we don't need them
* Delete obsolete roms when importing updated dats and automatically reimport orphan romfiles
* Move failed imports to the trash directories
* Fix headered ROMs handling
* Add a unique constraint to the settings::key column
* Simplify discard settings, please refer to the new documentation
* Remove the ability to delete a setting
* Allow purge-roms to delete orphan romfiles

**WARNING** The internal region format is now TOSEC's, all dats need to be reimported for this change to take effect.

# 0.6.0
* Replace refinery with sqlx migrate
* Add a check-roms subcommand

# 0.5.0
* Filter ROMs by name in convert-roms

# 0.4.0
* Replace diesel with sqlx and refinery
* Use async_std
* Add a config subcommand and use the database to store the settings
* Add unit tests

# 0.3.0
* Add progress indicators and bars
* Add dependency on the indicatif crate

# 0.2.2
* Cleanup files on unsuccessful matches

# 0.2.1
* Fix documentation

# 0.2.0
* Deprecate environment variables in favor of db configuration
* New associated config subcommand
* Add dependency on the dirs crate

# 0.1.0
* Initial release