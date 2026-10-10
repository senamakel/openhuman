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

/// A search module whose `ExecuteTool` takes `delay` to answer, like a
/// provider synthesizing a `web_answer_tool` reply.
struct SlowSearch {
    delay: std::time::Duration,
}

#[async_trait::async_trait]
impl tinybus::service::Interface for SlowSearch {
    fn name(&self) -> tinybus::InterfaceName {
        tinybus::InterfaceName::new(names::INTERFACE).expect("contract interface")
    }

    fn members(&self) -> Vec<tinybus::MemberName> {
        vec![tinybus::MemberName::new(names::methods::EXECUTE_TOOL).expect("contract member")]
    }

    async fn call(
        &self,
        _member: &tinybus::MemberName,
        _args: serde_json::Value,
    ) -> tinybus::Result<serde_json::Value> {
        tokio::time::sleep(self.delay).await;
        Ok(serde_json::json!({ "answered": true }))
    }
}

/// The service connection is returned too: dropping it would unserve the mock.
async fn slow_search_proxy(delay: std::time::Duration) -> (tinybus::Connection, tinybus::Proxy) {
    use tinybus::transport::memory::MemoryBus;
    let bus = MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let service = tinybus::Connection::connect(bus.connect().await.expect("service transport"))
        .await
        .expect("service");
    service
        .serve_at(
            tinybus::ObjectPath::new(names::OBJECT_PATH).expect("contract path"),
            SlowSearch { delay },
        )
        .await
        .expect("serve");
    service.request_name(names::INTERFACE).await.expect("name");
    let client = tinybus::Connection::connect(bus.connect().await.expect("client transport"))
        .await
        .expect("client");
    let proxy = client
        .proxy(names::INTERFACE, names::OBJECT_PATH, names::INTERFACE)
        .expect("proxy");
    (service, proxy)
}

fn web_answer_request() -> ExecuteToolRequest {
    ExecuteToolRequest {
        name: "web_answer_tool".into(),
        arguments: serde_json::json!({ "query": "q" }),
    }
}

#[tokio::test(start_paused = true)]
async fn execute_tool_outlives_the_bus_default_timeout() {
    // Production: `web_answer_tool` / `web_search_tool` failed at the 30 s bus
    // default while the module was still working.
    let delay = tinybus::connection::DEFAULT_TIMEOUT + std::time::Duration::from_secs(15);
    assert!(delay < EXECUTE_TOOL_TIMEOUT);

    // Control: the bus default really does cut this call off.
    let (_service, proxy) = slow_search_proxy(delay).await;
    let cut_off = proxy
        .clone()
        .call::<serde_json::Value>(names::methods::EXECUTE_TOOL, (web_answer_request(),))
        .await;
    assert!(
        cut_off.is_err(),
        "the default deadline must fire: {cut_off:?}"
    );

    let answered: serde_json::Value = call_execute_tool(proxy, web_answer_request())
        .await
        .expect("ExecuteTool runs under its own longer deadline");
    assert_eq!(answered["answered"], true);
}

#[test]
fn the_execute_tool_deadline_stays_under_the_harness_tool_deadline() {
    assert!(EXECUTE_TOOL_TIMEOUT > tinybus::connection::DEFAULT_TIMEOUT);
    assert!(
        EXECUTE_TOOL_TIMEOUT.as_secs() < crate::tools::timeout::DEFAULT_TIMEOUT_SECS,
        "the harness would kill the call before the module could answer"
    );
}
