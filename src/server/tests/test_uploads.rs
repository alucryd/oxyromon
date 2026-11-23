use crate::database::find_roms;
use super::*;
use crate::test_support::TestEnv;
use async_graphql::Result;
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
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let rom_id = find_roms(&mut env.connection).await.first().unwrap().id;

    let rom_bytes = std::fs::read(env.fixture_source("Test Game (USA, Europe).rom")).unwrap();
    let patch_bytes = std::fs::read(env.fixture_source("Test Game (USA, Europe).bps")).unwrap();

    // when
    let matches = subcommand().get_matches_from(["server", "--port", "8012"]);
    let pool = env.pool.clone();
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
