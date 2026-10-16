use super::*;

fn reservation() -> ChannelRuntimeReservation {
    let handle = "0123456789abcdef0123456789abcdef".to_string();
    ChannelRuntimeReservation {
        object_path: format!("{OBJECT_PATH}/runtime_{handle}"),
        host_object_path: format!("{HOST_OBJECT_PATH}/runtime_{handle}"),
        handle,
    }
}

#[test]
fn private_paths_must_belong_to_the_reserved_runtime() {
    assert!(validate_reservation(&reservation()).is_ok());
    for path in [
        HOST_OBJECT_PATH.to_string(),
        format!("{HOST_OBJECT_PATH}/runtime_ffffffffffffffffffffffffffffffff"),
    ] {
        let mut value = reservation();
        value.host_object_path = path;
        assert_eq!(
            validate_reservation(&value),
            Err(ModuleCallError::ModuleFault)
        );
    }
    let mut value = reservation();
    value.object_path = OBJECT_PATH.to_string();
    assert_eq!(
        validate_reservation(&value),
        Err(ModuleCallError::ModuleFault)
    );
}

#[test]
fn malformed_handles_cannot_replace_another_callback() {
    for handle in [
        "",
        "../secret",
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef0123456789abcdef/other",
    ] {
        let mut value = reservation();
        value.handle = handle.to_string();
        assert_eq!(
            validate_reservation(&value),
            Err(ModuleCallError::ModuleFault)
        );
    }
}

#[tokio::test]
async fn cancelled_close_wait_retains_the_original_cleanup_worker() {
    let closing = Arc::new(Mutex::new(None));
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let acknowledged = Arc::new(AtomicBool::new(false));
    let waiter = tokio::spawn({
        let closing = closing.clone();
        let started = started.clone();
        let release = release.clone();
        let acknowledged = acknowledged.clone();
        async move {
            join_cleanup(&closing, || {
                tokio::spawn(async move {
                    started.notify_one();
                    release.notified().await;
                    acknowledged.store(true, Ordering::Release);
                    Ok(())
                })
            })
            .await
        }
    });
    started.notified().await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(!acknowledged.load(Ordering::Acquire));
    assert!(closing.lock().await.is_some());
    release.notify_one();
    join_cleanup(&closing, || panic!("must reuse the retained worker"))
        .await
        .unwrap();
    assert!(acknowledged.load(Ordering::Acquire));
    assert!(closing.lock().await.is_none());
}

#[tokio::test]
async fn a_failed_cleanup_can_be_retried_without_aborting_pending_work() {
    let closing = Mutex::new(None);
    assert_eq!(
        join_cleanup(&closing, || tokio::spawn(async {
            Err(ModuleCallError::TransportFailed)
        }))
        .await,
        Err(ModuleCallError::TransportFailed)
    );
    assert!(closing.lock().await.is_none());
    join_cleanup(&closing, || tokio::spawn(async { Ok(()) }))
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_module_stops_cleanup_retries() {
    use tinybus::module::resolution::{Resolution, ResolutionTable};

    let table = ResolutionTable::default();
    let (sender, _) = match table.claim("channel-runtime-failed-fixture") {
        tinybus::module::resolution::Claim::Run {
            sender,
            receiver: _,
        } => (sender, ()),
        _ => unreachable!(),
    };
    table.complete(
        "channel-runtime-failed-fixture",
        Resolution::Failed("fixture fault".into()),
        sender,
    );
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        retry_stop(
            {
                let attempts = attempts.clone();
                move || {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    async { Err(ModuleCallError::TransportFailed) }
                }
            },
            || resolution_is_failed(&table.peek("channel-runtime-failed-fixture")),
            |_| tokio::task::yield_now(),
        ),
    )
    .await;

    assert!(result.is_ok(), "failed module cleanup must stop retrying");
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unresponsive_module_cleanup_has_a_bounded_retry_count() {
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let retry = tokio::spawn(retry_stop(
        {
            let attempts = attempts.clone();
            move || {
                let count = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                assert!(count <= 4, "cleanup exceeded the four-attempt cap");
                async { Err(ModuleCallError::TransportFailed) }
            }
        },
        || false,
        |_| std::future::ready(()),
    ));
    assert_eq!(
        retry
            .await
            .expect("unresponsive cleanup must have a retry cap"),
        4
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn acknowledged_stop_finishes_without_backoff() {
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let delays = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = retry_stop(
        {
            let attempts = attempts.clone();
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            }
        },
        || false,
        {
            let delays = delays.clone();
            move |delay| {
                delays.lock().unwrap().push(delay);
                std::future::ready(())
            }
        },
    )
    .await;

    assert_eq!(result, 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(delays.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_retry_backoff_is_capped() {
    let delays = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = retry_stop(
        || async { Err(ModuleCallError::TransportFailed) },
        || false,
        {
            let delays = delays.clone();
            move |delay| {
                delays.lock().unwrap().push(delay);
                std::future::ready(())
            }
        },
    )
    .await;

    assert_eq!(result, STOP_MAX_ATTEMPTS);
    assert_eq!(
        delays.lock().unwrap().as_slice(),
        &[
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_secs(1),
        ]
    );
}

pub(super) fn runtime_transport(
    client: &ModuleClient,
    runtime: &'static tinybus::module::ModuleRuntime,
) -> Result<(Connection, Proxy), ModuleCallError> {
    if let Some(proxy) = client.fixture_proxy() {
        return Ok((proxy.connection().clone(), proxy));
    }
    super::runtime_transport_real(runtime)
}

#[derive(Clone)]
struct TransportFixture {
    sends: Arc<std::sync::atomic::AtomicUsize>,
    expected_text: String,
}

#[allow(
    clippy::unused_async,
    reason = "TinyBus interfaces require async methods"
)]
#[tinybus::interface(name = "ai.tinyhumans.tinychannels.ChannelsHost")]
impl TransportFixture {
    async fn dispatch_controller_backend(
        &self,
        request: tinychannels_bus::ChannelBackendRequest,
    ) -> tinybus::Result<tinychannels_bus::ChannelBackendResult> {
        use tinychannels_bus::module_progressive::{
            ProgressiveTransportRequest, ProgressiveTransportResult,
        };
        let tinychannels_bus::ChannelBackendRequest::ProgressiveTransport(request) = request else {
            return Err(tinybus::Error::failed("unexpected fixture capability"));
        };
        let value = if let ProgressiveTransportRequest::Send { message, .. } = request {
            assert!(message["idempotencyKey"].as_str().is_some());
            assert_eq!(message["text"].as_str(), Some(self.expected_text.as_str()));
            self.sends.fetch_add(1, Ordering::SeqCst);
            serde_json::json!({"messageId":"fixture-message"})
        } else {
            serde_json::Value::Null
        };
        Ok(
            tinychannels_bus::ChannelBackendResult::ProgressiveTransport(
                ProgressiveTransportResult::Success(value),
            ),
        )
    }
}

/// Run only against a separately built, byte-verified release artifact.
#[tokio::test]
#[ignore = "requires OPENHUMAN_CHANNELS_TEST_ARTIFACT pointing to the verified compiled module"]
async fn released_runtime_scopes_callbacks_and_replays_one_final_send() {
    use tinychannels_bus::module_progressive::{ProgressiveCommand, ProgressiveResult};
    let artifact =
        std::env::var("OPENHUMAN_CHANNELS_TEST_ARTIFACT").expect("verified artifact path");
    let mut config = crate::config::Config::default();
    config.modules.allow_download = false;
    config.modules.overrides = vec![crate::config::schema::ModuleOverride::with_path(
        "tinychannels",
        artifact,
    )];
    // This content fits the module state budget but exceeds the former
    // 128 KiB serialized-request limit after JSON escaping.
    let final_text = "\"\\".repeat(192 * 1024);
    let first_sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let second_sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let first = ChannelRuntimeLease::open(
        crate::modules::client(config.clone()),
        TransportFixture {
            sends: first_sends.clone(),
            expected_text: final_text.clone(),
        },
    )
    .await
    .unwrap();
    let second = ChannelRuntimeLease::open(
        crate::modules::client(config),
        TransportFixture {
            sends: second_sends.clone(),
            expected_text: final_text.clone(),
        },
    )
    .await
    .unwrap();
    first.close().await.unwrap();
    let opened: ProgressiveResult = second
        .object()
        .call_confidential(
            methods::EXECUTE_PROGRESSIVE,
            (ProgressiveCommand::Open {
                channel: "discord:fixture".into(),
            },),
        )
        .await
        .unwrap();
    let ProgressiveResult::Opened { handle } = opened else {
        panic!("expected private handle");
    };
    for _ in 0..2 {
        let completed: ProgressiveResult = second
            .object()
            .call_confidential(
                methods::EXECUTE_PROGRESSIVE,
                (ProgressiveCommand::Finish {
                    handle: handle.clone(),
                    text: final_text.clone(),
                },),
            )
            .await
            .unwrap();
        assert_eq!(completed, ProgressiveResult::Closed);
    }
    assert_eq!(first_sends.load(Ordering::SeqCst), 0);
    assert_eq!(second_sends.load(Ordering::SeqCst), 1);
    let callback_path = second.0.cleanup.callback.clone();
    let connection = second.0.cleanup.connection.clone();
    second.close().await.unwrap();
    assert!(
        !connection.unserve(&callback_path).await,
        "adapter removed only after acknowledged stop"
    );
}
