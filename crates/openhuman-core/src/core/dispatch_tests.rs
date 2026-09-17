use super::*;
use serde_json::json;
use std::ffi::OsString;

/// Holds the shared config-env lock while an RPC handler reads the active
/// workspace. The controller path loads and may initialize config, so it
/// cannot safely share another test's transient workspace.
struct WorkspaceEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

impl WorkspaceEnvGuard {
    fn set(path: &std::path::Path) -> Self {
        let lock = crate::config::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os("OPENHUMAN_WORKSPACE");
        std::env::set_var("OPENHUMAN_WORKSPACE", path);
        Self {
            _lock: lock,
            previous,
        }
    }
}

impl Drop for WorkspaceEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("OPENHUMAN_WORKSPACE", value),
            None => std::env::remove_var("OPENHUMAN_WORKSPACE"),
        }
    }
}

fn test_state() -> AppState {
    AppState {
        core_version: "9.9.9-test".to_string(),
    }
}

#[tokio::test]
async fn dispatch_core_ping_returns_ok_true() {
    let out = dispatch(test_state(), "core.ping", json!({}))
        .await
        .expect("core.ping should succeed");
    assert_eq!(out, json!({ "ok": true }));
}

#[tokio::test]
async fn dispatch_core_version_returns_state_version() {
    let out = dispatch(test_state(), "core.version", json!({}))
        .await
        .expect("core.version should succeed");
    assert_eq!(out, json!({ "version": "9.9.9-test" }));
}

#[tokio::test]
async fn dispatch_core_ignores_params() {
    // Params must be tolerated even when the method takes none.
    let out = dispatch(test_state(), "core.ping", json!({ "extra": 1 }))
        .await
        .expect("core.ping should ignore extra params");
    assert_eq!(out, json!({ "ok": true }));
}

#[tokio::test]
async fn dispatch_rewrites_legacy_alias_before_lookup() {
    // `openhuman.ping` is a legacy alias for `core.ping` in the shared
    // alias table. Going through the dispatcher must rewrite it and
    // route successfully to Tier 1 instead of falling through to the
    // unknown-method error path.
    let out = dispatch(test_state(), "openhuman.ping", json!({}))
        .await
        .expect("legacy alias openhuman.ping must resolve to core.ping");
    assert_eq!(out, json!({ "ok": true }));
}

#[tokio::test]
async fn dispatch_unknown_method_returns_error() {
    let err = dispatch(test_state(), "does.not.exist", json!({}))
        .await
        .expect_err("unknown methods must error");
    assert!(err.contains("unknown method"));
    assert!(err.contains("does.not.exist"));
}

#[tokio::test]
async fn dispatch_empty_method_returns_unknown_method_error() {
    let err = dispatch(test_state(), "", json!({}))
        .await
        .expect_err("empty method must error");
    assert!(err.contains("unknown method"));
}

#[tokio::test]
async fn dispatch_delegates_to_tier2_for_domain_method() {
    // Tier 2 dispatcher handles `openhuman.security_policy_info`, so
    // it must succeed and return a policy object.
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let _workspace_env = WorkspaceEnvGuard::set(workspace.path());
    let out = dispatch(test_state(), "openhuman.security_policy_info", json!({}))
        .await
        .expect("security_policy_info should route via tier 2");
    // With logs present, payload is wrapped as { result, logs }.
    assert!(out.get("result").is_some() || out.get("autonomy").is_some());
}

#[test]
fn try_core_dispatch_returns_none_for_non_core_namespace() {
    let state = test_state();
    assert!(try_core_dispatch(&state, "openhuman.memory_list_namespaces", json!({})).is_none());
    assert!(try_core_dispatch(&state, "corez.ping", json!({})).is_none());
}

#[test]
fn try_core_dispatch_matches_exact_ping_and_version() {
    let state = test_state();
    assert!(try_core_dispatch(&state, "core.ping", json!({})).is_some());
    assert!(try_core_dispatch(&state, "core.version", json!({})).is_some());
    // Prefix match alone must not count.
    assert!(try_core_dispatch(&state, "core.pingz", json!({})).is_none());
    assert!(try_core_dispatch(&state, "core", json!({})).is_none());
}

#[test]
fn try_core_dispatch_version_reflects_appstate() {
    let state = AppState {
        core_version: "0.0.0-abc".into(),
    };
    let result = try_core_dispatch(&state, "core.version", json!({}))
        .expect("core.version must be routed")
        .expect("core.version must produce InvocationResult");
    assert_eq!(result.value, json!({ "version": "0.0.0-abc" }));
    assert!(result.logs.is_empty());
}

#[tokio::test]
async fn dispatch_legacy_ping_rewrites_and_succeeds() {
    let out = dispatch(test_state(), "openhuman.ping", json!({}))
        .await
        .expect("openhuman.ping should be rewritten to core.ping and succeed");
    assert_eq!(out, json!({ "ok": true }));
}

#[test]
fn is_known_probe_method_matches_allow_list_exactly() {
    // Every allow-listed probe / legacy health name is recognised.
    for m in [
        "rpc.discover",
        "list_methods",
        "status",
        "auth.status",
        "config/get",
        "openhuman.memory_tree_create_namespace",
        "openhuman.harness_init_status",
    ] {
        assert!(
            is_known_probe_method(m),
            "{m} should be a debug-only known miss"
        );
    }
    // Genuinely-unknown methods and near-misses are NOT allow-listed, so
    // they stay on the warn-for-triage path rather than being silenced.
    assert!(!is_known_probe_method("does.not.exist"));
    assert!(!is_known_probe_method("core.not_a_real_method"));
    assert!(!is_known_probe_method("Status")); // case-sensitive
    assert!(!is_known_probe_method("rpc.discover.extra")); // exact match only
    assert!(!is_known_probe_method("memory_tree_create_namespace"));
    assert!(!is_known_probe_method(""));
}

/// `openhuman.harness_init_status` is allow-listed as a debug-only miss so
/// client/core surface skew stops paging Sentry (#5157) — but it is a
/// **live** method, not a retired one. That allow-list entry means a
/// genuine regression (controller dropped from the registry) would go
/// completely silent: no error, no warn, no Sentry event. This test is the
/// replacement signal — if the method stops being served in a full build,
/// this fails instead of the regression shipping unnoticed.
#[test]
fn harness_init_status_is_registered_in_a_full_build() {
    let served: Vec<String> = crate::core::all::all_controller_schemas()
        .iter()
        .map(crate::core::all::rpc_method_name)
        .collect();
    assert!(
        served.iter().any(|m| m == "openhuman.harness_init_status"),
        "harness_init_status must remain a registered controller — it is \
         allow-listed in KNOWN_PROBE_METHODS for client/core skew only, so \
         losing the real handler would be silently swallowed"
    );
}

#[tokio::test]
#[cfg(feature = "channels")]
async fn dispatch_dotted_channel_list_aliases_route_to_registry() {
    for method in ["channels.list", "openhuman.channels.list"] {
        let out = dispatch(test_state(), method, json!({}))
            .await
            .unwrap_or_else(|err| panic!("{method} should route via channels_list: {err}"));
        assert!(
            out.is_array() || out.get("result").is_some(),
            "expected {method} to return the channels_list payload, got {out}"
        );
    }
}

#[test]
fn unknown_method_name_extracts_from_error_string_only() {
    // The classifier round-trips the exact string `dispatch` emits.
    let err = format!("{UNKNOWN_METHOD_PREFIX}rpc.discover");
    assert_eq!(unknown_method_name(&err), Some("rpc.discover"));
    // Unrelated error strings are not misclassified as unknown-method.
    assert_eq!(unknown_method_name("unknown param 'x' for ns.fn"), None);
    assert_eq!(unknown_method_name("Session expired"), None);
}

#[tokio::test]
async fn dispatch_probe_method_still_returns_unknown_method_error() {
    // Allow-listed probe names must not be silently "handled" — the caller
    // still gets a method-not-found error. Only the Sentry severity (in the
    // transport layer) changes; the dispatch contract is unchanged.
    let err = dispatch(test_state(), "rpc.discover", json!({}))
        .await
        .expect_err("probe methods are still unknown to the dispatcher");
    assert_eq!(unknown_method_name(&err), Some("rpc.discover"));
    assert!(is_known_probe_method(
        unknown_method_name(&err).expect("unknown-method error")
    ));
}

#[tokio::test]
async fn dispatch_legacy_alias_routes_to_registry() {
    // This alias targets a controller registered in the domain registry.
    // Do not invoke it here: its implementation can persist default config,
    // which makes this routing test depend on a filesystem workspace.
    let method = crate::core::legacy_aliases::resolve_legacy("openhuman.get_analytics_settings");
    assert!(
        crate::core::all::schema_for_rpc_method(method).is_some(),
        "legacy alias must resolve to a registered controller: {method}"
    );
}
