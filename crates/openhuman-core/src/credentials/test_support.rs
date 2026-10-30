//! Isolated configurations for tests of routing and synthetic credentials.

/// Keep fake credentials independent of the operator's profile and native
/// cipher consent. The profile document still encrypts its storage at rest.
/// Native encryption tests opt into their admitted module explicitly.
pub(crate) fn plaintext_fixture_config(directory: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config {
        config_path: directory.join("config.toml"),
        workspace_dir: directory.join("workspace"),
        action_dir: directory.join("workspace"),
        ..Default::default()
    };
    config.secrets.encrypt = false;
    config
}

/// Exercise the real native cipher with explicit one-time operator consent.
#[cfg(feature = "security-module")]
pub(crate) async fn with_native_cipher_approval<F: std::future::Future>(
    config: &crate::config::Config,
    operation: F,
) -> F::Output {
    use crate::security::approval::{ApprovalDecision, ApprovalGate};
    crate::modules::security::ensure_loaded(config)
        .await
        .expect("admit native cipher module");
    crate::core::bus::init().await.expect("approval events");
    let gate =
        ApprovalGate::init_global(config.clone(), format!("session-{}", uuid::Uuid::new_v4()));
    let operation = crate::core::runtime::turn_origin::with_origin(
        crate::core::runtime::turn_origin::AgentTurnOrigin::WebChat {
            thread_id: "cipher-fixture".into(),
            client_id: "test-operator".into(),
            request_id: None,
        },
        operation,
    );
    tokio::pin!(operation);
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            tokio::select! {
                result = &mut operation => break result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    for pending in gate.list_pending().expect("pending native approvals") {
                        assert_eq!(pending.tool_name, "native_secret_grant");
                        gate.decide(&pending.request_id, ApprovalDecision::ApproveOnce)
                            .expect("approve only this cipher master resolution");
                    }
                }
            }
        }
    })
    .await
    .expect("cipher operation completes after explicit consent")
}
