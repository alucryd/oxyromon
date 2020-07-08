use super::crud::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::ArgMatches;
use diesel::SqliteConnection;
use std::path::Path;

pub fn purge_roms(connection: &SqliteConnection, matches: &ArgMatches) -> SimpleResult<()> {
    // delete roms in trash
    if matches.is_present("EMPTY-TRASH") {
        println!("Processing trashed ROM files");
        println!("");

        let romfiles = find_romfiles_in_trash(&connection);

        if romfiles.len() > 0 {
            println!("Summary:");
            for romfile in &romfiles {
                println!("{}", &romfile.path);
            }

            if prompt_for_yes_no(matches) {
                for romfile in &romfiles {
                    let romfile_path = Path::new(&romfile.path).to_path_buf();
                    if romfile_path.is_file() {
                        remove_file(&romfile_path)?;
                        delete_romfile_by_id(connection, romfile.id);
                    }
                }
            }
        }
    }

    // deleted missing roms from database
    println!("Processing missing ROM files");
    println!("");

    let romfiles = find_romfiles(connection);
    let mut count = 0;

    for romfile in romfiles {
        if !Path::new(&romfile.path).is_file() {
            delete_romfile_by_id(connection, romfile.id);
            count += 1;
        }
    }

    if count > 0 {
        println!("Deleted {} missing rom(s) from the database", count);
    }

    Ok(())
}
