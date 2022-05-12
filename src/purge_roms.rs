use super::database::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;

pub fn subcommand<'a>() -> Command<'a> {
    Command::new("purge-roms")
        .about("Purge trashed, missing and orphan ROM files")
        .arg(
            Arg::new("MISSING")
                .short('m')
                .long("missing")
                .help("Delete missing ROM files from the database")
                .required(false),
        )
        .arg(
            Arg::new("ORPHAN")
                .short('o')
                .long("orphan")
                .help("Delete ROM files without an associated ROM from the database")
                .required(false),
        )
        .arg(
            Arg::new("TRASH")
                .short('t')
                .long("trash")
                .help("Physically delete ROM files from the trash directories")
                .required(false),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically say yes to prompts")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if matches.is_present("MISSING") {
        purge_missing_romfiles(connection, progress_bar).await?;
    }
    if matches.is_present("TRASH") {
        purge_trashed_romfiles(connection, matches, progress_bar).await?;
    }
    if matches.is_present("ORPHAN") {
        purge_orphan_romfiles(connection, progress_bar).await?;
    }
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(100);
    progress_bar.set_message("Computing system completion");
    update_games_mark_incomplete(connection).await;
    update_systems_mark_incomplete(connection).await;
    Ok(())
}

async fn purge_missing_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println("Processing missing ROM files");

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
            "Deleted {} missing ROM file(s) from the database",
            count
        ));
    }

    Ok(())
}

async fn purge_trashed_romfiles(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println("Processing trashed ROM files");

    let romfiles = find_romfiles_in_trash(connection).await;
    let mut count = 0;

    if !romfiles.is_empty() {
        progress_bar.println("Summary:");
        for romfile in &romfiles {
            progress_bar.println(&romfile.path);
        }

        if matches.is_present("YES") || confirm(true)? {
            let mut transaction = begin_transaction(connection).await;

            for romfile in &romfiles {
                let romfile_path = Path::new(&romfile.path);
                if romfile_path.is_file().await {
                    remove_file(progress_bar, &romfile_path, false).await?;
                    delete_romfile_by_id(&mut transaction, romfile.id).await;
                    count += 1;
                }
            }

            commit_transaction(transaction).await;

            if count > 0 {
                progress_bar.println(&format!("Deleted {} trashed ROM file(s)", count));
            }
        }
    }

    Ok(())
}

async fn purge_orphan_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println("Processing orphan ROM files");
    delete_romfiles_without_rom(connection).await;
    Ok(())
}

#[cfg(test)]
mod test_missing;
#[cfg(test)]
mod test_trashed;
