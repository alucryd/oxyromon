use super::common::*;
use super::config::HashAlgorithm;
use super::database::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;
use tokio::fs;
use tokio::io::{copy, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

pub fn subcommand() -> Command {
    Command::new("benchmark").about("Benchmark oxyromon").arg(
        Arg::new("CHUNK_SIZE")
            .short('c')
            .long("chunk-size")
            .help("Set the chunk size in KB for read and writes (Default: 256)")
            .required(false)
            .num_args(1)
            .default_value("256"),
    )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let rom_directory = find_setting_by_key(connection, "ROM_DIRECTORY")
        .await
        .unwrap()
        .value
        .unwrap();
    let tmp_directory = find_setting_by_key(connection, "TMP_DIRECTORY")
        .await
        .unwrap()
        .value
        .unwrap();

    let rom_file_path = Path::new(&rom_directory).join(".oxyromon");
    let tmp_file_path = Path::new(&tmp_directory).join(".oxyromon");

    let original_romfile_tmpdir = OriginalRomfile {
        path: tmp_file_path.to_path_buf(),
    };

    let mb_count = 1024;
    // TODO: make this into a setting
    let chunk_size = matches
        .get_one::<String>("CHUNK_SIZE")
        .unwrap()
        .parse::<usize>()
        .unwrap();

    // rom write speed
    progress_bar.set_message("Measuring ROM directory write speed");
    let reader = BufReader::with_capacity(
        chunk_size * 1024,
        fs::File::open("/dev/random").await.unwrap(),
    );
    let mut writer = BufWriter::with_capacity(
        chunk_size * 1024,
        fs::File::create(&rom_file_path).await.unwrap(),
    );
    let start = Instant::now();
    copy(&mut reader.take(1024 * 1024 * mb_count), &mut writer)
        .await
        .unwrap();
    writer.flush().await.unwrap();
    let duration = start.elapsed();

    progress_bar.println(format!(
        "ROM Directory Write Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // rom read speed
    progress_bar.set_message("Measuring ROM directory read speed");
    let reader = BufReader::with_capacity(
        chunk_size * 1024,
        fs::File::open(&rom_file_path).await.unwrap(),
    );
    let mut writer = BufWriter::with_capacity(
        chunk_size * 1024,
        fs::File::create("/dev/null").await.unwrap(),
    );
    let start = Instant::now();
    copy(&mut reader.take(1024 * 1024 * mb_count), &mut writer)
        .await
        .unwrap();
    writer.flush().await.unwrap();
    let duration = start.elapsed();

    progress_bar.println(format!(
        "ROM Directory Read Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // tmp write speed
    progress_bar.set_message("Measuring TMP directory write speed");
    let reader = BufReader::with_capacity(
        chunk_size * 1024,
        fs::File::open("/dev/random").await.unwrap(),
    );
    let mut writer = BufWriter::with_capacity(
        chunk_size * 1024,
        fs::File::create(&tmp_file_path).await.unwrap(),
    );
    let start = Instant::now();
    copy(&mut reader.take(1024 * 1024 * mb_count), &mut writer)
        .await
        .unwrap();
    writer.flush().await.unwrap();
    let duration = start.elapsed();

    progress_bar.println(format!(
        "TMP Directory Write Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // tmp read speed
    progress_bar.set_message("Measuring TMP directory read speed");
    let reader = BufReader::with_capacity(
        chunk_size * 1024,
        fs::File::open(&tmp_file_path).await.unwrap(),
    );
    let mut writer = BufWriter::with_capacity(
        chunk_size * 1024,
        fs::File::create("/dev/null").await.unwrap(),
    );

    let start = Instant::now();
    copy(&mut reader.take(1024 * 1024 * mb_count), &mut writer)
        .await
        .unwrap();
    writer.flush().await.unwrap();
    let duration = start.elapsed();

    progress_bar.println(format!(
        "TMP Directory Read Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // crc speed
    let start = Instant::now();
    original_romfile_tmpdir
        .get_hash(
            connection,
            progress_bar,
            &None,
            1,
            1,
            &HashAlgorithm::Crc,
        )
        .await?;
    let duration = start.elapsed();

    progress_bar.println(format!(
        "CRC Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // md5 speed
    let start = Instant::now();
    original_romfile_tmpdir
        .get_hash(
            connection,
            progress_bar,
            &None,
            1,
            1,
            &HashAlgorithm::Md5,
        )
        .await?;
    let duration = start.elapsed();

    progress_bar.println(format!(
        "MD5 Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    // sha1 speed
    let start = Instant::now();
    original_romfile_tmpdir
        .get_hash(
            connection,
            progress_bar,
            &None,
            1,
            1,
            &HashAlgorithm::Sha1,
        )
        .await?;
    let duration = start.elapsed();

    progress_bar.println(format!(
        "SHA1 Speed: {:.2}Mb/s",
        mb_count as f64 / duration.as_secs_f64()
    ));

    remove_file(progress_bar, &rom_file_path, true).await?;
    remove_file(progress_bar, &tmp_file_path, true).await?;

    Ok(())
}
