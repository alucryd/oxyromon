use super::database::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{App, Arg, ArgMatches, SubCommand};
use sqlx::SqliteConnection;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("purge-roms")
        .about("Purges trashed and missing ROM files")
        .arg(
            Arg::with_name("EMPTY_TRASH")
                .short("t")
                .long("empty-trash")
                .help("Empties the ROM files trash directories")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        )
}

pub async fn main(connection: &mut SqliteConnection, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    // delete roms in trash
    if matches.is_present("EMPTY-TRASH") {
        progress_bar.set_message("Processing trashed ROM files");

        let romfiles = find_romfiles_in_trash(connection).await;

        if romfiles.len() > 0 {
            progress_bar.println("Summary:");
            for romfile in &romfiles {
                progress_bar.println(&romfile.path);
            }

            if prompt_for_yes_no(matches).await {
                for romfile in &romfiles {
                    let romfile_path = Path::new(&romfile.path).to_path_buf();
                    if romfile_path.is_file().await {
                        remove_file(&romfile_path).await?;
                        delete_romfile_by_id(connection, romfile.id).await;
                    }
                }
            }
        }
    }

    // deleted missing roms from database
    progress_bar.set_message("Processing missing ROM files");

    let romfiles = find_romfiles(connection).await;
    let mut count = 0;

    for romfile in romfiles {
        if !Path::new(&romfile.path).is_file().await {
            delete_romfile_by_id(connection, romfile.id).await;
            count += 1;
        }
    }

    if count > 0 {
        progress_bar.println(&format!(
            "Deleted {} missing rom(s) from the database",
            count
        ));
    }

    Ok(())
}
