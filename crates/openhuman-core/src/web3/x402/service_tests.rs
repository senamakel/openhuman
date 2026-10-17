//! Budget and service-identity behavior for the Wallet x402 adapter.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn service(handle: &str) -> X402Service {
    X402Service {
        workspace: PathBuf::from("/workspace"),
        accounts: tinywallet_bus::wire::WalletAccountsResponse {
            configured: false,
            accounts: vec![],
        },
        config: X402ConfigResponse {
            workspace_dir: "/workspace".into(),
            session_id: "session".into(),
            per_request_max_atomic: 1,
            daily_max_atomic: 2,
            monthly_max_atomic: 3,
        },
        handle: X402ServiceHandle {
            handle: handle.into(),
        },
    }
}

#[tokio::test]
async fn stale_x402_close_failure_still_opens_and_installs_replacement() {
    let mut active = Some(service("stale"));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let open_calls = Arc::new(AtomicUsize::new(0));
    let closes = close_calls.clone();
    let opens = open_calls.clone();
    replace_service_with(
        &mut active,
        move |_| async move {
            closes.fetch_add(1, Ordering::SeqCst);
            Err(WalletCallError::Failed("opaque close details".into()))
        },
        move || async move {
            opens.fetch_add(1, Ordering::SeqCst);
            Ok(service("replacement"))
        },
    )
    .await
    .expect("replacement open should determine the result");
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert_eq!(open_calls.load(Ordering::SeqCst), 1);
    assert_eq!(active.as_ref().unwrap().handle.handle, "replacement");
}

#[test]
fn budget_defaults_and_environment_overrides_match_the_host_contract() {
    let defaults = budget_from(|_| None);
    assert_eq!(defaults, (1_000_000, 10_000_000, 100_000_000));

    let configured = budget_from(|name| match name {
        "OPENHUMAN_X402_PER_REQUEST_MAX" => Some("2500".to_string()),
        "OPENHUMAN_X402_DAILY_MAX" => Some("not-a-number".to_string()),
        "OPENHUMAN_X402_MONTHLY_MAX" => Some("9000000".to_string()),
        _ => None,
    });
    assert_eq!(configured, (2_500, 10_000_000, 9_000_000));
}
