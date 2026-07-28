//! Business logic for the `medulla` RPC namespace.
//!
//! Each function resolves a client from ambient config, performs one backend
//! call, and returns an [`RpcOutcome`]. Transport and client-construction
//! failures become [`StructuredRpcError`]s so a host can branch on a stable
//! `data.kind` instead of matching on message text.

use serde::{Deserialize, Serialize};

use crate::openhuman::config::Config;
use crate::rpc::{RpcOutcome, StructuredRpcError};

use super::client::{ClientError, MedullaClient, RosterWorker, SessionSummary};
use super::resolve::{self, NotConfigured};

/// Whether the Medulla integration is usable, and why not when it isn't.
///
/// `Deserialize` as well as `Serialize` because this type round-trips: ops
/// serializes it onto the RPC boundary and the embed facade deserializes it
/// back on the other side. An output-only derive compiles until the facade
/// tries to read it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MedullaStatus {
    /// True when a base URL and a session token are both available.
    pub configured: bool,
    /// The resolved base URL, when one is configured.
    ///
    /// Safe to surface: it is an operator-supplied endpoint, never a credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Whether a session token was resolved. The token itself is never returned.
    pub has_session_token: bool,
    /// Stable reason discriminator when `configured` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Report integration readiness without making a network call.
///
/// Deliberately infallible: "not configured" is a state to render, not an error
/// to raise, and a host polls this to decide whether to show the surface at all.
pub async fn status(config: &Config) -> Result<RpcOutcome<MedullaStatus>, String> {
    let base = resolve::base_url(config);
    let status = match resolve::client(config) {
        Ok(_) => MedullaStatus {
            configured: true,
            base_url: base,
            has_session_token: true,
            reason: None,
        },
        Err(reason) => MedullaStatus {
            configured: false,
            has_session_token: !matches!(reason, NotConfigured::NoSessionToken),
            base_url: base,
            reason: Some(reason.kind().to_string()),
        },
    };
    log::debug!(
        "[medulla] status configured={} reason={:?}",
        status.configured,
        status.reason
    );
    Ok(RpcOutcome::new(status, Vec::new()))
}

/// List the operator's durable sessions.
pub async fn list_sessions(config: &Config) -> Result<RpcOutcome<Vec<SessionSummary>>, String> {
    let client = resolved(config)?;
    let sessions = call(client.list_sessions().await, "medulla_list_sessions")?;
    log::debug!("[medulla] list_sessions count={}", sessions.len());
    Ok(RpcOutcome::new(sessions, Vec::new()))
}

/// Read the connected worker roster.
pub async fn roster(config: &Config) -> Result<RpcOutcome<Vec<RosterWorker>>, String> {
    let client = resolved(config)?;
    let workers = call(client.roster().await, "medulla_roster")?;
    log::debug!("[medulla] roster count={}", workers.len());
    Ok(RpcOutcome::new(workers, Vec::new()))
}

/// Resolve a client, mapping "not configured" to a structured error.
///
/// Both reasons are flagged `expected_user_state`: a signed-out or unconfigured
/// host is a state the operator can fix, not an internal fault, so the RPC
/// boundary suppresses Sentry for them.
fn resolved(config: &Config) -> Result<MedullaClient, String> {
    resolve::client(config).map_err(|reason| {
        StructuredRpcError {
            message: reason.message().to_string(),
            data: Some(serde_json::json!({ "kind": reason.kind() })),
            expected_user_state: true,
        }
        .encode()
    })
}

/// Map a client error into a structured RPC error.
///
/// The backend's own `errorCode` becomes `data.kind` when present, so a host
/// branches on the backend's vocabulary rather than on prose. HTTP 401/403 are
/// marked `expected_user_state` — an expired session is a sign-in prompt, not a
/// bug report.
fn call<T>(result: Result<T, ClientError>, method: &'static str) -> Result<T, String> {
    result.map_err(|err| {
        let (kind, status, expected) = match &err {
            ClientError::Api {
                status, error_code, ..
            } => {
                let expected = matches!(status, Some(401) | Some(403));
                (error_code.clone(), *status, expected)
            }
            _ => (None, None, false),
        };
        log::debug!("[medulla] rpc_failed method={method} status={status:?} kind={kind:?}");
        StructuredRpcError {
            message: err.to_string(),
            data: Some(serde_json::json!({
                "kind": kind.unwrap_or_else(|| "MedullaRequestFailed".to_string()),
                "status": status,
            })),
            expected_user_state: expected,
        }
        .encode()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_configured_encodes_an_expected_user_state_envelope() {
        let config = Config::default();
        // No api_url and no env override, so resolution fails on base URL.
        std::env::remove_var(resolve::MEDULLA_BASE_URL_ENV);
        let err = resolved(&config).expect_err("must not resolve");
        let decoded = StructuredRpcError::decode(&err).expect("structured envelope");
        assert!(
            decoded.expected_user_state,
            "an unconfigured host is a user state, not an internal fault"
        );
        assert_eq!(
            decoded
                .data
                .and_then(|d| d["kind"].as_str().map(String::from)),
            Some("MedullaNoBaseUrl".to_string())
        );
    }

    #[test]
    fn api_error_preserves_the_backend_error_code_as_kind() {
        let err = call::<()>(
            Err(ClientError::Api {
                status: Some(409),
                message: "already archived".into(),
                error_code: Some("SessionArchived".into()),
                details: None,
            }),
            "medulla_test",
        )
        .expect_err("must be an error");
        let decoded = StructuredRpcError::decode(&err).expect("structured envelope");
        assert_eq!(decoded.data.unwrap()["kind"], "SessionArchived");
        assert!(!decoded.expected_user_state, "409 is not a sign-in prompt");
    }

    #[test]
    fn unauthorized_is_marked_expected_user_state() {
        for status in [401, 403] {
            let err = call::<()>(
                Err(ClientError::Api {
                    status: Some(status),
                    message: "nope".into(),
                    error_code: None,
                    details: None,
                }),
                "medulla_test",
            )
            .expect_err("must be an error");
            let decoded = StructuredRpcError::decode(&err).expect("structured envelope");
            assert!(
                decoded.expected_user_state,
                "{status} should read as a sign-in prompt, not a bug"
            );
        }
    }

    #[test]
    fn missing_error_code_falls_back_to_a_stable_kind() {
        let err = call::<()>(Err(ClientError::Decode("bad json".into())), "medulla_test")
            .expect_err("must be an error");
        let decoded = StructuredRpcError::decode(&err).expect("structured envelope");
        assert_eq!(decoded.data.unwrap()["kind"], "MedullaRequestFailed");
    }

    #[tokio::test]
    async fn status_reports_unconfigured_rather_than_failing() {
        std::env::remove_var(resolve::MEDULLA_BASE_URL_ENV);
        let out = status(&Config::default())
            .await
            .expect("status never errors");
        assert!(!out.value.configured);
        assert_eq!(out.value.reason.as_deref(), Some("MedullaNoBaseUrl"));
    }
}
