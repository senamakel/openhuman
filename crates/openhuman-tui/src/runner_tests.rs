use super::*;

#[test]
fn extract_thread_id_handles_bare_summary() {
    let v = json!({ "id": "thread-1", "title": "x" });
    assert_eq!(extract_thread_id(&v).as_deref(), Some("thread-1"));
}

#[test]
fn extract_thread_id_handles_api_envelope() {
    let v = json!({ "data": { "id": "thread-2" }, "meta": {} });
    assert_eq!(extract_thread_id(&v).as_deref(), Some("thread-2"));
}

#[test]
fn extract_thread_id_handles_log_envelope_around_api_envelope() {
    let v = json!({
        "result": { "data": { "id": "thread-3" }, "meta": {} },
        "logs": ["created"]
    });
    assert_eq!(extract_thread_id(&v).as_deref(), Some("thread-3"));
}

#[test]
fn extract_thread_id_missing_returns_none() {
    let v = json!({ "meta": {} });
    assert_eq!(extract_thread_id(&v), None);
}

#[test]
fn short_hex_is_twelve_chars() {
    assert_eq!(short_hex().len(), 12);
}

/// Regression guard: the RPC method names the TUI invokes must be the
/// canonical `openhuman.<namespace>_<function>` form that the registry
/// resolves — NOT the dotted `namespace.function` short form, which
/// `schema_for_rpc_method` does not recognise and which would make every
/// turn (and the launch-time thread creation) fail with "unknown method".
/// The dispatcher only rewrites a fixed set of legacy aliases; none of
/// these three are in that table, so the short form never resolves.
#[test]
fn tui_invokes_use_canonical_registered_rpc_method_names() {
    #[allow(unused_mut)]
    let mut methods = vec![
        "openhuman.channel_web_chat",
        "openhuman.channel_web_cancel",
        "openhuman.channel_web_queue_status",
        "openhuman.threads_create_new",
        "openhuman.threads_list",
        "openhuman.threads_transcript_get",
        "openhuman.threads_update_title",
        "openhuman.threads_delete",
        "openhuman.threads_task_board_get",
        "openhuman.threads_token_usage",
        "openhuman.thread_goals_get",
        "openhuman.thread_goals_set",
        "openhuman.profiles_list",
        "openhuman.profiles_select",
        "openhuman.ai_list_artifacts",
        "openhuman.approval_list_pending",
        "openhuman.approval_decide",
        "openhuman.plan_review_decide",
        "openhuman.config_get_client_config",
        "openhuman.config_get_agent_paths",
        "openhuman.config_update_model_settings",
        "openhuman.config_get_autonomy_settings",
        "openhuman.config_update_autonomy_settings",
        "openhuman.config_get_privacy_mode",
        "openhuman.config_set_privacy_mode",
        "openhuman.auth_get_state",
        "openhuman.auth_get_me",
        "openhuman.auth_consume_login_token",
        "openhuman.auth_store_session",
        "openhuman.auth_clear_session",
    ];
    methods.push("openhuman.skills_list");
    methods.push("openhuman.mcp_clients_installed_list");
    for method in methods {
        assert!(
            openhuman_core::core::all::schema_for_rpc_method(method).is_some(),
            "TUI invokes `{method}`, but it is not a registered RPC method — \
             the tabbed terminal UI would fail with `unknown method: {method}`"
        );
    }
}
