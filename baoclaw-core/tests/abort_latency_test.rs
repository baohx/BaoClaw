//! Integration test: verify abort signal propagates within 50ms.

use std::time::Instant;
use tokio::sync::watch;

#[tokio::test]
async fn test_abort_wait_fires_under_50ms() {
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(baoclaw_core::engine::wait_for_abort(rx));
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    tx.send(true).unwrap();
    handle.await.unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 50,
        "wait_for_abort took {}ms, expected < 50ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_abort_does_not_fire_on_dropped_sender() {
    let (tx, rx) = watch::channel(false);
    drop(tx); // dropped with value=false — NOT an abort
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        baoclaw_core::engine::wait_for_abort(rx),
    )
    .await;
    assert!(result.is_err(), "should have timed out, not aborted");
}
