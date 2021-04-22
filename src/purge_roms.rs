use super::database::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("purge-roms")
        .about("Purges trashed, missing and orphan ROM files")
        .arg(
            Arg::with_name("MISSING")
                .short("m")
                .long("missing")
                .help("Deletes missing ROM files from the database")
                .required(false),
        )
        .arg(
            Arg::with_name("ORPHAN")
                .short("o")
                .long("orphan")
                .help("Deletes ROM files without an associated ROM from the database")
                .required(false),
        )
        .arg(
            Arg::with_name("TRASH")
                .short("t")
                .long("trash")
                .help("Physically deletes ROM files from the trash directories")
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

pub async fn main(matches: &ArgMatches<'_>, progress_bar: &ProgressBar) -> SimpleResult<()> {
    if matches.is_present("MISSING") {
        purge_missing_romfiles(&progress_bar).await?;
    }
    if matches.is_present("TRASH") {
        purge_trashed_romfiles(matches, &progress_bar).await?;
    }
    if matches.is_present("ORPHAN") {
        purge_orphan_romfiles(&progress_bar).await?;
    }
    Ok(())
}

async fn purge_missing_romfiles(progress_bar: &ProgressBar) -> SimpleResult<()> {
    progress_bar.println("Processing missing ROM files");

    let romfiles = find_romfiles(POOL.get().unwrap()).await;
    let mut count = 0;

    for romfile in romfiles {
        if !Path::new(&romfile.path).is_file().await {
            delete_romfile_by_id(POOL.get().unwrap(), romfile.id).await;
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
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println("Processing trashed ROM files");

    let romfiles = find_romfiles_in_trash(POOL.get().unwrap()).await;
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
                    remove_file(&romfile_path).await?;
                    delete_romfile_by_id(POOL.get().unwrap(), romfile.id).await;
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

async fn purge_orphan_romfiles(progress_bar: &ProgressBar) -> SimpleResult<()> {
    progress_bar.println("Processing orphan ROM files");
    delete_romfiles_without_rom(POOL.get().unwrap()).await;
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
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_purge_missing_roms() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&matches, &progress_bar)
            .await
            .unwrap();

        let romfiles = find_romfiles(POOL.get().unwrap()).await;
        remove_file(&Path::new(&romfiles[0].path)).await.unwrap();

        // when
        purge_missing_romfiles(&progress_bar).await.unwrap();

        // then
        let romfiles = find_romfiles(POOL.get().unwrap()).await;
        assert!(romfiles.is_empty());
        assert!(&system_directory
            .read_dir()
            .await
            .unwrap()
            .next()
            .await
            .is_none());
    }

    #[async_std::test]
    async fn test_purge_trashed_roms() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&matches, &progress_bar)
            .await
            .unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();
        let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path,
        )
        .await
        .unwrap();

        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&matches, &progress_bar)
            .await
            .unwrap();

        let matches =
            sort_roms::subcommand().get_matches_from(&["sort-roms", "-a", "-y", "-g", "JP"]);
        sort_roms::main(&matches, &progress_bar)
            .await
            .unwrap();

        // when
        let matches = subcommand().get_matches_from(&["purge-roms", "-y"]);

        purge_trashed_romfiles(&matches, &progress_bar)
            .await
            .unwrap();

        // then
        let romfiles = find_romfiles(POOL.get().unwrap()).await;
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
