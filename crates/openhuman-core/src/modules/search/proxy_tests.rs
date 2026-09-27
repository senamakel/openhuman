use super::*;

#[tokio::test]
async fn configured_calls_keep_the_lock_through_invocation() {
    use std::sync::{Arc, Mutex};
    let active = Arc::new(Mutex::new(String::new()));
    let (a_entered_tx, a_entered_rx) = tokio::sync::oneshot::channel();
    let (release_a_tx, release_a_rx) = tokio::sync::oneshot::channel();
    let a_active = active.clone();
    let a = tokio::spawn(async move {
        with_module_lock(|| async move {
            *a_active.lock().unwrap() = "A".into();
            a_entered_tx.send(()).unwrap();
            release_a_rx.await.unwrap();
            assert_eq!(*a_active.lock().unwrap(), "A");
            Ok::<_, String>(())
        })
        .await
        .unwrap();
    });
    a_entered_rx.await.unwrap();
    let b_active = active.clone();
    let (b_started_tx, b_started_rx) = tokio::sync::oneshot::channel();
    let b = tokio::spawn(async move {
        b_started_tx.send(()).unwrap();
        with_module_lock(|| async move {
            *b_active.lock().unwrap() = "B".into();
            Ok::<_, String>(())
        })
        .await
        .unwrap();
    });
    b_started_rx.await.unwrap();
    tokio::task::yield_now().await;
    assert_eq!(*active.lock().unwrap(), "A");
    release_a_tx.send(()).unwrap();
    a.await.unwrap();
    b.await.unwrap();
    assert_eq!(*active.lock().unwrap(), "B");
}
