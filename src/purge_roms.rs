use super::SimpleResult;
use super::common::*;
use super::config::*;
use super::database::*;
use super::prompt::*;
use super::util::*;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use walkdir::WalkDir;

pub fn subcommand() -> Command {
    Command::new("purge-roms")
        .about("Purge trashed, missing, and orphan ROM files")
        .arg(
            Arg::new("MISSING")
                .short('m')
                .long("missing")
                .help("Delete missing ROM files from the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ORPHAN")
                .short('o')
                .long("orphan")
                .help("Delete ROM files without an associated ROM from the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("TRASH")
                .short('t')
                .long("trash")
                .help("Physically delete ROM files from the trash directories")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FOREIGN")
                .short('f')
                .long("foreign")
                .help("Physically delete ROM files unknown to the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically say yes to prompts")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let answer_yes = matches.get_flag("YES");
    if matches.get_flag("MISSING") {
        purge_missing_romfiles(connection, progress_bar).await?;
    }
    if matches.get_flag("TRASH") {
        purge_trashed_romfiles(connection, progress_bar, answer_yes).await?;
    }
    if matches.get_flag("ORPHAN") {
        purge_orphan_romfiles(connection, progress_bar, answer_yes).await?;
    }
    if matches.get_flag("FOREIGN") {
        purge_foreign_romfiles(connection, progress_bar, answer_yes).await?;
    }
    for system in find_systems(connection).await {
        compute_system_completion(connection, progress_bar, &system).await?;
    }
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
        if !romfile.as_common(connection).await?.path.is_file() {
            delete_romfile_by_id(connection, romfile.id).await;
            count += 1;
        }
    }

    if count > 0 {
        progress_bar.println(format!(
            "Deleted {} missing ROM file(s) from the database",
            count
        ));
    }

    Ok(())
}

async fn purge_trashed_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
) -> SimpleResult<()> {
    progress_bar.println("Processing trashed ROM files");

    let romfiles = find_romfiles_in_trash(connection).await;
    let mut count = 0;

    if !romfiles.is_empty() {
        progress_bar.println("Summary:");
        for romfile in &romfiles {
            progress_bar.println(&romfile.path);
        }

        if answer_yes || confirm(true)? {
            let mut transaction = begin_transaction(connection).await;

            for romfile in &romfiles {
                if romfile.as_common(&mut transaction).await?.path.is_file() {
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    delete_romfile_by_id(&mut transaction, romfile.id).await;
                    count += 1;
                }
            }

            commit_transaction(transaction).await;

            if count > 0 {
                progress_bar.println(format!("Deleted {} trashed ROM file(s)", count));
            }
        }
    }

    Ok(())
}

async fn purge_orphan_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
) -> SimpleResult<()> {
    progress_bar.println("Processing orphan ROM files");

    let romfiles = find_orphan_romfiles(connection).await;
    let mut count = 0;

    if !romfiles.is_empty() {
        progress_bar.println("Summary:");
        for romfile in &romfiles {
            progress_bar.println(&romfile.path);
        }

        if answer_yes || confirm(true)? {
            let mut transaction = begin_transaction(connection).await;

            for romfile in &romfiles {
                if romfile.as_common(&mut transaction).await?.path.is_file() {
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .delete(progress_bar, false)
                        .await?;
                    delete_romfile_by_id(&mut transaction, romfile.id).await;
                    count += 1;
                }
            }

            commit_transaction(transaction).await;

            if count > 0 {
                progress_bar.println(format!("Deleted {} trashed ROM file(s)", count));
            }
        }
    }

    Ok(())
}

async fn purge_foreign_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
) -> SimpleResult<()> {
    progress_bar.println("Processing foreign ROM files");
    let rom_directory = get_rom_directory(connection).await;
    let walker = WalkDir::new(&rom_directory).into_iter();
    let mut count = 0;
    for entry in walker.filter_map(|e| e.ok()) {
        if entry.path().is_file() {
            let relative_path = try_with!(
                entry.path().strip_prefix(&rom_directory),
                "Failed to retrieve relative path"
            );
            if find_romfile_by_path(connection, relative_path.as_os_str().to_str().unwrap())
                .await
                .is_none()
            {
                progress_bar.println(format!(
                    "Delete \"{}\"?",
                    relative_path.as_os_str().to_str().unwrap()
                ));
                if answer_yes || confirm(true)? {
                    remove_file(progress_bar, &entry.path(), false).await?;
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        progress_bar.println(format!(
            "Deleted {} foreign ROM file(s) from the ROM directory",
            count
        ));
    }
    Ok(())
}

#[cfg(test)]
mod test_foreign;
#[cfg(test)]
mod test_missing;
#[cfg(test)]
mod test_orphans;
#[cfg(test)]
mod test_trashed;
