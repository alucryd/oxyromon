use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::super::database::find_roms;
use super::super::import_dats;
use super::*;
use async_graphql::Result;
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::select;
use tokio::time::{Duration, sleep};

// (field name, optional filename, bytes). A None filename makes a text field.
fn multipart(fields: &[(&str, Option<&str>, &[u8])]) -> (Vec<u8>, String) {
    let boundary = "boundary_oxyromon_upload_test";
    let mut body = Vec::new();
    for (name, filename, data) in fields {
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        match filename {
            Some(f) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}

async fn post(client: &reqwest::Client, path: &str, body: Vec<u8>, content_type: String) -> u16 {
    client
        .post(format!("http://127.0.0.1:8012{path}"))
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn test() -> Result<()> {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let rom_id = find_roms(&mut connection).await.first().unwrap().id;

    let rom_bytes = std::fs::read(test_directory.join("Test Game (USA, Europe).rom")).unwrap();
    let patch_bytes = std::fs::read(test_directory.join("Test Game (USA, Europe).bps")).unwrap();

    // when
    let matches = subcommand().get_matches_from(["server", "--port", "8012"]);
    let server = async move {
        main(pool, &matches).await.unwrap();
    };

    let client = async move {
        sleep(Duration::from_millis(100)).await;
        let client = reqwest::Client::new();

        // upload_rom: no file and no url -> 400
        let (body, ct) = multipart(&[("junk", None, b"x")]);
        assert_eq!(post(&client, "/roms", body, ct).await, 400);

        // upload_rom: a file -> 202 (queued)
        let (body, ct) = multipart(&[("file", Some("Test Game (USA, Europe).rom"), &rom_bytes)]);
        assert_eq!(post(&client, "/roms", body, ct).await, 202);

        // upload_patch: file but no rom -> 400
        let (body, ct) = multipart(&[("file", Some("p.bps"), &patch_bytes)]);
        assert_eq!(post(&client, "/patches", body, ct).await, 400);

        // upload_patch: unknown rom -> 400
        let (body, ct) = multipart(&[
            ("file", Some("p.bps"), &patch_bytes),
            ("rom", None, b"999999"),
        ]);
        assert_eq!(post(&client, "/patches", body, ct).await, 400);

        // upload_patch: valid rom -> 202
        let (body, ct) = multipart(&[
            ("file", Some("p.bps"), &patch_bytes),
            ("rom", None, rom_id.to_string().as_bytes()),
        ]);
        assert_eq!(post(&client, "/patches", body, ct).await, 202);

        // upload_ird: no file -> 400
        let (body, ct) = multipart(&[("system", None, b"1")]);
        assert_eq!(post(&client, "/irds", body, ct).await, 400);

        // upload_ird: file but no system -> 400
        let (body, ct) = multipart(&[("file", Some("x.ird"), b"not an ird")]);
        assert_eq!(post(&client, "/irds", body, ct).await, 400);
    };

    select! {
        _ = server => {}
        _ = client => {}
    }

    Ok(())
}
