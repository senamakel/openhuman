use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn service(handle: &str) -> Web3Service {
    Web3Service {
        workspace: PathBuf::from("/workspace"),
        accounts: tinywallet_bus::wire::WalletAccountsResponse {
            configured: false,
            accounts: vec![],
        },
        configuration: Web3ConfigureRequest {
            solana_cluster: tinywallet_bus::wire::SolanaCluster::Mainnet,
            overridden_endpoints: vec![],
        },
        response: Web3ConfigureResponse {
            service: tinywallet_bus::wire::Web3ServiceHandle {
                handle: handle.into(),
            },
            endpoint_references: vec![],
        },
        quotes: HashMap::new(),
    }
}

#[tokio::test]
async fn stale_web3_close_failure_still_opens_and_installs_replacement() {
    let mut active = Some(service("stale"));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let open_calls = Arc::new(AtomicUsize::new(0));
    let closes = close_calls.clone();
    let opens = open_calls.clone();
    replace_service_with(
        &mut active,
        move |_| async move {
            closes.fetch_add(1, Ordering::SeqCst);
            Err(wallet::WalletCallError::Failed(
                "opaque close details".into(),
            ))
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
    assert_eq!(
        active.as_ref().unwrap().response.service.handle,
        "replacement"
    );
    assert!(active.as_ref().unwrap().quotes.is_empty());
}
