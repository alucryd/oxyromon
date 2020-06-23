use super::crud::*;
use super::prompt::*;
use clap::ArgMatches;
use diesel::pg::PgConnection;
use std::error::Error;
use std::fs;
use std::path::Path;

pub fn purge_roms(connection: &PgConnection, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    // delete roms in trash
    println!("Processing trash");
    println!("");

    let romfiles = find_romfiles_in_trash(&connection);

    if romfiles.len() > 0 {
        println!("Summary:");
        for romfile in &romfiles {
            println!("{}", &romfile.path);
        }

        if prompt_for_yes_no(matches) {
            for romfile in &romfiles {
                if Path::new(&romfile.path).is_file() {
                    fs::remove_file(&romfile.path)?;
                    delete_romfile_by_id(connection, &romfile.id);
                }
            }
        }
    }

    // deleted missing roms from database
    println!("Processing missing");
    println!("");

    let romfiles = find_romfiles(connection);
    let mut count = 0;

    for romfile in romfiles {
        if !Path::new(&romfile.path).is_file() {
            delete_romfile_by_id(connection, &romfile.id);
            count += 1;
        }
    }

    if count > 0 {
        println!("Deleted {} missing rom(s) from the database", count);
    }

    Ok(())
}
