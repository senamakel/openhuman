use super::{
    channel_connection_update_payload, event_alias, origin_is_allowed_with_extra,
    publish_companion_state_changed, subscribe_companion_state_changed,
};

#[test]
fn companion_state_transport_delivers_payload_to_bridge() {
    let mut rx = subscribe_companion_state_changed();
    let payload = serde_json::json!({
        "session_id": "session-1",
        "state": "thinking",
        "previous_state": "listening",
    });
    assert!(publish_companion_state_changed(payload.clone()) >= 1);
    assert_eq!(rx.try_recv().expect("payload delivered"), payload);
}

#[test]
fn channel_connection_update_payload_connected_omits_error() {
    let payload = channel_connection_update_payload("discord", "connected", None);
    assert_eq!(payload["channel"], "discord");
    // Listener-backed channels always map to the bot_token auth mode.
    assert_eq!(payload["auth_mode"], "bot_token");
    assert_eq!(payload["status"], "connected");
    assert!(
        payload.get("last_error").is_none(),
        "connected payload must not carry a last_error: {payload}"
    );
}

#[test]
fn channel_connection_update_payload_error_carries_reason() {
    let payload =
        channel_connection_update_payload("discord", "error", Some("gateway closed (4004)"));
    assert_eq!(payload["channel"], "discord");
    assert_eq!(payload["auth_mode"], "bot_token");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["last_error"], "gateway closed (4004)");
}

#[test]
fn event_alias_translates_between_delimiters() {
    assert_eq!(event_alias("chat_done").as_deref(), Some("chat:done"));
    assert_eq!(event_alias("chat:error").as_deref(), Some("chat_error"));
    assert_eq!(event_alias("ready"), None);
}

#[test]
fn event_alias_suppressed_for_streaming_deltas() {
    // Streaming deltas must NOT be aliased — doubling every token frame is
    // the "double thinking-token streaming" bug. Discrete events still alias.
    assert_eq!(event_alias("thinking_delta"), None);
    assert_eq!(event_alias("text_delta"), None);
    assert_eq!(event_alias("tool_args_delta"), None);
    assert_eq!(event_alias("subagent_tool_args_delta"), None);
    // A *discrete* event that merely ends in `_delta` is NOT a streaming
    // token event and must keep its compat alias — this is what the explicit
    // STREAMING_DELTA_EVENTS set guarantees over the old `*_delta` suffix.
    assert_eq!(
        event_alias("inventory_delta").as_deref(),
        Some("inventory:delta")
    );
    // Sanity: a non-delta event in the same family still aliases.
    assert_eq!(event_alias("tool_call").as_deref(), Some("tool:call"));
}

#[test]
fn origin_allowlist_accepts_native_clients() {
    assert!(origin_is_allowed_with_extra(None, None));
}

#[test]
fn origin_allowlist_accepts_tauri_localhost_across_schemes() {
    // The CEF-served app shell stamps a platform-dependent Origin:
    //   - macOS / iOS use the native `tauri://localhost` scheme
    //   - Windows uses the CEF custom HTTP protocol → `http://tauri.localhost`
    //   - Linux / older Windows builds use `https://tauri.localhost`
    // All three flavours are the same trust tier (the bundled webview),
    // so each must pass the handshake gate.
    assert!(origin_is_allowed_with_extra(
        Some("tauri://localhost"),
        None
    ));
    assert!(origin_is_allowed_with_extra(
        Some("https://tauri.localhost"),
        None
    ));
    assert!(origin_is_allowed_with_extra(
        Some("http://tauri.localhost"),
        None
    ));
}

#[test]
fn origin_allowlist_accepts_local_dev_server() {
    assert!(origin_is_allowed_with_extra(
        Some("http://localhost:1420"),
        None
    ));
    assert!(origin_is_allowed_with_extra(
        Some("http://127.0.0.1:1420"),
        None
    ));
    assert!(origin_is_allowed_with_extra(
        Some("http://[::1]:1420"),
        None
    ));
    // Loopback without an explicit port (some CEF builds stamp this
    // shape when the shell runs on the default port).
    assert!(origin_is_allowed_with_extra(Some("http://localhost"), None));
}

#[test]
fn origin_allowlist_rejects_cross_origin_browser_pages() {
    assert!(!origin_is_allowed_with_extra(
        Some("https://attacker.example"),
        None
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("http://evil.local"),
        None
    ));
    assert!(!origin_is_allowed_with_extra(Some("null"), None));
    assert!(!origin_is_allowed_with_extra(Some(""), None));
}

#[test]
fn origin_allowlist_rejects_host_prefix_decoys() {
    // Regression: `starts_with("localhost")` accepted these; the exact
    // host match must not.
    assert!(!origin_is_allowed_with_extra(
        Some("http://localhost.attacker.example"),
        None
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("http://127.0.0.1.attacker.example"),
        None
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("https://localhost-evil"),
        None
    ));
    // Same rule applies to the tauri.localhost host — must be exact.
    assert!(!origin_is_allowed_with_extra(
        Some("http://tauri.localhost.attacker.example"),
        None
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("https://tauri.localhost.evil"),
        None
    ));
}

#[test]
fn origin_allowlist_accepts_env_allowlisted_origin() {
    // Regression: a non-loopback browser origin that the JSON-RPC CORS
    // layer accepts must also pass the socket handshake, or chat (a
    // socket-only transport) silently never connects.
    let extra = Some("https://box.example.ts.net:9000");
    assert!(origin_is_allowed_with_extra(
        Some("https://box.example.ts.net:9000"),
        extra
    ));
}

#[test]
fn origin_allowlist_env_handles_comma_list_and_whitespace() {
    let extra = Some("https://a.example:9000 , https://b.example:8443");
    assert!(origin_is_allowed_with_extra(
        Some("https://a.example:9000"),
        extra
    ));
    assert!(origin_is_allowed_with_extra(
        Some("https://b.example:8443"),
        extra
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("https://c.example"),
        extra
    ));
}

#[test]
fn origin_allowlist_env_requires_exact_match() {
    // Populating the allowlist must not loosen the decoy rules: no port
    // wildcarding, no scheme swapping, no suffix matching.
    let extra = Some("https://box.example.ts.net:9000");
    assert!(!origin_is_allowed_with_extra(
        Some("http://box.example.ts.net:9000"),
        extra
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("https://box.example.ts.net:9001"),
        extra
    ));
    assert!(!origin_is_allowed_with_extra(
        Some("https://box.example.ts.net.attacker.example:9000"),
        extra
    ));
}

#[test]
fn origin_allowlist_env_does_not_rescue_null_or_empty() {
    let extra = Some("null,");
    assert!(!origin_is_allowed_with_extra(Some("null"), extra));
    assert!(!origin_is_allowed_with_extra(Some(""), extra));
}

#[test]
fn origin_allowlist_rejects_unparseable_origin() {
    assert!(!origin_is_allowed_with_extra(Some("not a url"), None));
    assert!(!origin_is_allowed_with_extra(
        Some("javascript:alert(1)"),
        None
    ));
}
