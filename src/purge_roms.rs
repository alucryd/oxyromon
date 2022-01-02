use super::database::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{App, Arg, ArgMatches};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;

pub fn subcommand<'a>() -> App<'a> {
    App::new("purge-roms")
        .about("Purges trashed, missing and orphan ROM files")
        .arg(
            Arg::new("MISSING")
                .short('m')
                .long("missing")
                .help("Deletes missing ROM files from the database")
                .required(false),
        )
        .arg(
            Arg::new("ORPHAN")
                .short('o')
                .long("orphan")
                .help("Deletes ROM files without an associated ROM from the database")
                .required(false),
        )
        .arg(
            Arg::new("TRASH")
                .short('t')
                .long("trash")
                .help("Physically deletes ROM files from the trash directories")
                .required(false),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically says yes to prompts")
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
            for romfile in &romfiles {
                let romfile_path = Path::new(&romfile.path);
                if romfile_path.is_file().await {
                    remove_file(progress_bar, &romfile_path, false).await?;
                    delete_romfile_by_id(connection, romfile.id).await;
                    count += 1;
                }
            }

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
mod test {
    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::super::sort_roms;
    use super::*;
    use async_std::fs;
    use async_std::path::PathBuf;
    use async_std::prelude::*;
    use futures::stream::TryStreamExt;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_purge_missing_roms() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfiles = find_romfiles(&mut connection).await;
        remove_file(&progress_bar, &Path::new(&romfiles[0].path), false)
            .await
            .unwrap();

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        // when
        purge_missing_romfiles(&mut connection, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(&mut connection).await;
        assert!(romfiles.is_empty());
        let entries: Vec<fs::DirEntry> = system_directory
            .read_dir()
            .await
            .unwrap()
            .map_ok(|entry| entry)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries.get(0).unwrap().path(),
            get_trash_directory(&mut connection, &progress_bar, &system)
                .await
                .unwrap()
        );
    }

    #[async_std::test]
    async fn test_purge_trashed_roms() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let matches =
            sort_roms::subcommand().get_matches_from(&["sort-roms", "-a", "-y", "-g", "JP"]);
        sort_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["purge-roms", "-y"]);

        purge_trashed_romfiles(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(&mut connection).await;
        assert!(romfiles.is_empty());
        assert!(&system_directory
            .join("Trash")
            .read_dir()
            .await
            .unwrap()
            .next()
            .await
            .is_none());
    }
}
