use super::*;
use crate::config::MUTEX;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::sync::oneshot;
use tokio::time::timeout;

/// A web action announces itself, then waits for the action holding the write
/// lock, whichever surface started that one, before it runs.
#[tokio::test]
async fn test() {
    let _guard = MUTEX.lock().await;
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let (sse_tx, mut sse_rx) = broadcast::channel(16);
    let (ran_tx, mut ran_rx) = oneshot::channel();

    let held = WRITE_LOCK.lock().await;
    spawn_cli_action(
        pool,
        sse_tx,
        "test",
        String::from("started"),
        String::from("done"),
        "failed",
        move |_| {
            Box::pin(async move {
                ran_tx.send(()).unwrap();
                Ok(())
            })
        },
    );

    sse_rx.recv().await.unwrap();
    assert!(
        timeout(Duration::from_millis(100), &mut ran_rx)
            .await
            .is_err()
    );

    drop(held);
    timeout(Duration::from_secs(5), ran_rx)
        .await
        .unwrap()
        .unwrap();
}
