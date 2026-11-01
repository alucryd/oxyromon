use super::actions::lock_writes;
use super::*;
use crate::config::MUTEX;

/// A tool waiting behind a long action can be cancelled, by its client or by
/// the server stopping, and gets the lock once it is free.
#[tokio::test]
async fn test() {
    let _guard = MUTEX.lock().await;
    let (client, server) = (CancellationToken::new(), CancellationToken::new());

    let held = WRITE_LOCK.lock().await;
    let waiting = lock_writes(&client, &server);
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut waiting)
            .await
            .is_err()
    );

    client.cancel();
    assert_eq!(
        waiting.await.err().as_deref(),
        Some("cancelled by the client")
    );

    let client = CancellationToken::new();
    server.cancel();
    assert_eq!(
        lock_writes(&client, &server).await.err().as_deref(),
        Some("server shutting down")
    );

    drop(held);
    assert!(
        lock_writes(&client, &CancellationToken::new())
            .await
            .is_ok()
    );
}
