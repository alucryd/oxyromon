use super::database::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
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

    if matches.is_present("EMPTY_TRASH") {
        purge_trashed_roms(connection, matches, &progress_bar).await?;
    }

    purge_missing_roms(connection, &progress_bar).await?;

    Ok(())
}

async fn purge_missing_roms(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
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
            "Deleted {} missing ROM file(s) from the database",
            count
        ));
    }

    Ok(())
}

async fn purge_trashed_roms(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.set_message("Processing trashed ROM files");

    let romfiles = find_romfiles_in_trash(connection).await;
    let mut count = 0;

    if romfiles.len() > 0 {
        progress_bar.println("Summary:");
        for romfile in &romfiles {
            progress_bar.println(&romfile.path);
        }

        if prompt_for_yes_no(matches, &progress_bar).await {
            for romfile in &romfiles {
                let romfile_path = Path::new(&romfile.path).to_path_buf();
                if romfile_path.is_file().await {
                    remove_file(&romfile_path).await?;
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

#[cfg(test)]
mod test {
    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::embedded;
    use super::super::import_dats::import_dat;
    use super::super::import_roms::import_rom;
    use super::super::sort_roms::sort_system;
    use super::*;
    use async_std::fs;
    use async_std::path::{Path, PathBuf};
    use async_std::prelude::*;
    use async_std::sync::Mutex;
    use refinery::config::{Config, ConfigDbType};
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_purge_missing_roms() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();
        let romfiles = find_romfiles(&mut connection).await;
        remove_file(&PathBuf::from(&romfiles[0].path))
            .await
            .unwrap();

        // when
        purge_missing_roms(&mut connection, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(&mut connection).await;
        assert!(romfiles.is_empty());
        assert!(&system_path.read_dir().await.unwrap().next().await.is_none());
    }

    #[async_std::test]
    async fn test_purge_trashed_roms() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let rom_path = tmp_path.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &rom_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        import_rom(
            &mut connection,
            &system_path,
            &system,
            &None,
            &rom_path,
            &progress_bar,
        )
        .await
        .unwrap();

        let matches = subcommand().get_matches_from(vec!["sort-roms", "-y"]);
        let all_regions = vec!["JPN"];
        let one_regions = vec![];

        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &None,
            &progress_bar,
        )
        .await
        .unwrap();

        // when
        let matches = subcommand().get_matches_from(vec!["purge-roms", "-y"]);

        purge_trashed_roms(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(&mut connection).await;
        assert!(romfiles.is_empty());
        assert!(&system_path
            .join("Trash")
            .read_dir()
            .await
            .unwrap()
            .next()
            .await
            .is_none());
    }
}
