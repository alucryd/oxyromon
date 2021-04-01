# 0.6.1
* Add a unique constraint to the settings::key column
* Use shiratsu_naming to parse No-Intro names
* Delete obsolete roms and releases when importing updated dats and automatically reimport orphan romfiles

**WARNING** The internal region format is now TOSEC's, all dats need to be reimported for the change to take effect.

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