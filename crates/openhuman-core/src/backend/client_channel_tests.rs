use super::*;

#[tokio::test]
async fn channel_routes_build_expected_requests_and_validate_inputs() {
    type Requests = Arc<Mutex<Vec<(String, String, Value)>>>;
    async fn capture(
        State(seen): State<Requests>,
        request: axum::http::Request<axum::body::Body>,
    ) -> Json<Value> {
        let method = request.method().to_string();
        let path = request.uri().to_string();
        let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        seen.lock().unwrap().push((method, path, body));
        Json(json!({ "success": true, "data": { "ok": true } }))
    }

    let seen = Requests::default();
    let app = Router::new().fallback(capture).with_state(seen.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = BackendClient::new(&format!("http://{addr}")).unwrap();

    client
        .send_channel_message(" /telegram/ ", "jwt", json!({ "text": "hello" }))
        .await
        .unwrap();
    client
        .send_channel_reaction("telegram", "jwt", json!({ "emoji": "👍" }))
        .await
        .unwrap();
    client
        .create_channel_thread("telegram", "jwt", "  topic  ")
        .await
        .unwrap();
    client
        .update_channel_thread("telegram", "jwt", "thread-1", "close")
        .await
        .unwrap();
    client
        .list_channel_threads("telegram", "jwt", Some(true))
        .await
        .unwrap();
    client
        .list_channel_threads("telegram", "jwt", Some(false))
        .await
        .unwrap();
    client
        .list_channel_threads("telegram", "jwt", None)
        .await
        .unwrap();

    let requests = seen.lock().unwrap();
    assert_eq!(
        requests[0],
        (
            "POST".into(),
            "/channels/telegram/messages".into(),
            json!({ "text": "hello" })
        )
    );
    assert_eq!(
        requests[1],
        (
            "POST".into(),
            "/channels/telegram/reactions".into(),
            json!({ "emoji": "👍" })
        )
    );
    assert_eq!(
        requests[2],
        (
            "POST".into(),
            "/channels/telegram/threads".into(),
            json!({ "title": "topic" })
        )
    );
    assert_eq!(
        requests[3],
        (
            "PATCH".into(),
            "/channels/telegram/threads/thread-1".into(),
            json!({ "action": "close" })
        )
    );
    assert_eq!(requests[4].1, "/channels/telegram/threads?active=true");
    assert_eq!(requests[5].1, "/channels/telegram/threads?active=false");
    assert_eq!(requests[6].1, "/channels/telegram/threads");
    drop(requests);

    assert!(client
        .send_channel_message(" / ", "jwt", json!({}))
        .await
        .is_err());
    assert!(client
        .send_channel_reaction(" ", "jwt", json!({}))
        .await
        .is_err());
    assert!(client
        .create_channel_thread("telegram", "jwt", " ")
        .await
        .is_err());
    assert!(client
        .update_channel_thread("telegram", "jwt", " ", "close")
        .await
        .is_err());
    assert!(client
        .update_channel_thread("telegram", "jwt", "thread-1", "delete")
        .await
        .is_err());
    assert!(client.list_channel_threads("", "jwt", None).await.is_err());
}

#[tokio::test]
async fn authed_json_reports_non_channel_404_still_propagates() {
    // TAURI-RUST-8C: a GET 404 on a non-channel path (e.g. `/teams/me/usage`)
    // falls through to `report_error` (not a typed/suppressed state) — it must
    // still return an Err (no suppression) and not a typed `BackendApiError`.
    let app = Router::new().route(
        "/teams/me/usage",
        get(|| async {
            (
                axum::http::StatusCode::NOT_FOUND,
                r#"{"message":"Not Found"}"#,
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let client = BackendClient::new(&base_url).unwrap();

    let err = client
        .authed_json("mock-jwt", Method::GET, "/teams/me/usage", None)
        .await
        .unwrap_err();
    assert!(err.downcast_ref::<BackendApiError>().is_none());
    let msg = format!("{err:#}");
    assert!(msg.contains("404"), "error should carry the status: {msg}");
    assert!(
        msg.contains("/teams/me/usage"),
        "error should carry the path: {msg}"
    );
}

#[test]
fn flatten_authed_error_maps_unauthorized_to_session_expired_sentinel() {
    // #3297: the typed `Unauthorized` (expected session-lapse 401) must flatten
    // onto a string that the JSON-RPC session-expiry classifiers recognise, so
    // it is suppressed from Sentry (TAURI-RUST-8WY / 8WZ) instead of leaking.
    let err = anyhow::Error::new(BackendApiError::Unauthorized {
        method: "GET".to_string(),
        path: "/teams/me/usage".to_string(),
    });
    let flat = flatten_authed_error(err);

    // Carries the SESSION_EXPIRED sentinel + preserves method/path for logs.
    assert!(
        flat.contains("SESSION_EXPIRED"),
        "expected sentinel, got: {flat}"
    );
    assert!(flat.contains("GET"), "method preserved: {flat}");
    assert!(flat.contains("/teams/me/usage"), "path preserved: {flat}");

    // Contract cross-check: the flattened string MUST classify as session
    // expiry. This couples the mapping to the actual classifier — if either the
    // sentinel or the classifier drifts, this fails instead of silently leaking.
    assert!(
        crate::core::observability::is_session_expired_message(&flat),
        "flattened Unauthorized must classify as session expiry: {flat}"
    );
}

#[test]
fn flatten_authed_error_preserves_non_unauthorized_chain() {
    // A non-Unauthorized failure (e.g. a transient network/timeout error) keeps
    // its full `{e:#}` anyhow chain and must NOT be demoted to session expiry —
    // genuine failures still reach Sentry.
    let err = anyhow::anyhow!("connect timeout").context("backend request GET /teams/me/usage");
    let flat = flatten_authed_error(err);

    assert!(!flat.contains("SESSION_EXPIRED"), "must not map: {flat}");
    assert!(flat.contains("connect timeout"), "cause preserved: {flat}");
    assert!(
        !crate::core::observability::is_session_expired_message(&flat),
        "non-auth error must NOT classify as session expiry: {flat}"
    );
}

#[test]
fn flatten_authed_error_does_not_swallow_message_not_found() {
    // `MessageNotFound` is a different expected state handled by its own callers
    // (channel streaming/delete paths downcast it); it must not be collapsed
    // into the session-expiry sentinel here.
    let err = anyhow::Error::new(BackendApiError::MessageNotFound {
        provider: "telegram".to_string(),
        message_id: "1103".to_string(),
    });
    let flat = flatten_authed_error(err);

    assert!(!flat.contains("SESSION_EXPIRED"), "must not map: {flat}");
    assert!(
        flat.contains("message not found"),
        "display preserved: {flat}"
    );
}

#[tokio::test]
async fn authed_json_403_is_not_demoted_to_unauthorized() {
    // 403 (Forbidden) is a genuine authorization/permission problem — the
    // token authenticated but lacked scope. That IS a code/config bug we
    // want to keep in Sentry; only 401 (token rejected as a whole) maps
    // to the expected-state `Unauthorized` variant.
    let app = Router::new().route(
        "/openai/v1/audio/speech",
        post(|| async { (axum::http::StatusCode::FORBIDDEN, "Forbidden") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let client = BackendClient::new(&base_url).unwrap();

    let err = client
        .authed_json("mock-jwt", Method::POST, "/openai/v1/audio/speech", None)
        .await
        .unwrap_err();
    assert!(
        err.downcast_ref::<BackendApiError>().is_none(),
        "403 must not be classified as Unauthorized"
    );
}

#[tokio::test]
async fn authed_json_404_outside_messages_path_still_reports() {
    // 404 on a non-`/channels/<provider>/messages/<id>` path should NOT be
    // demoted to MessageNotFound — it's a real backend bug or routing
    // mistake and must keep its Sentry signal.
    let app = Router::new().route(
        "/auth/profile",
        get(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let client = BackendClient::new(&base_url).unwrap();

    let err = client
        .authed_json("mock-jwt", Method::GET, "/auth/profile", None)
        .await
        .unwrap_err();
    assert!(
        err.downcast_ref::<BackendApiError>().is_none(),
        "non-channel-message 404 must not be classified as MessageNotFound"
    );
}

// ── parse_message_path unit tests (TAURI-R7 regression guard) ───────────────

#[test]
fn parse_message_path_canonical_form() {
    assert_eq!(
        parse_message_path("/channels/telegram/messages/1103"),
        Some(("telegram", "1103"))
    );
}

#[test]
fn parse_message_path_discord_provider() {
    assert_eq!(
        parse_message_path("/channels/discord/messages/abc"),
        Some(("discord", "abc"))
    );
}

#[test]
fn parse_message_path_base_path_prefix() {
    // TAURI-R7 root cause: BACKEND_URL with a path prefix adds segments,
    // breaking the strict 4-segment check. The sliding window must handle it.
    assert_eq!(
        parse_message_path("/api/v1/channels/telegram/messages/1103"),
        Some(("telegram", "1103"))
    );
}

#[test]
fn parse_message_path_double_prefix() {
    assert_eq!(
        parse_message_path("/v2/api/channels/discord/messages/abc"),
        Some(("discord", "abc"))
    );
}

#[test]
fn parse_message_path_trailing_slash() {
    assert_eq!(
        parse_message_path("/channels/telegram/messages/1103/"),
        Some(("telegram", "1103"))
    );
}

#[test]
fn parse_message_path_percent_encoded_slug() {
    // Channel slugs with percent-encoded characters must pass through verbatim.
    assert_eq!(
        parse_message_path("/channels/telegram%3Abot/messages/1103"),
        Some(("telegram%3Abot", "1103"))
    );
}

#[test]
fn parse_message_path_non_message_path_returns_none() {
    assert_eq!(parse_message_path("/channels/telegram/typing"), None);
    assert_eq!(parse_message_path("/channels/telegram"), None);
    assert_eq!(parse_message_path("/auth/profile"), None);
    assert_eq!(parse_message_path("/"), None);
    assert_eq!(parse_message_path(""), None);
}

// ── authed_json defense-in-depth: PATCH 404 with base-path prefix ───────────

#[tokio::test]
async fn authed_json_patch_404_with_base_path_prefix_does_not_report() {
    // Regression for TAURI-R7: if the resolved URL has a base-path prefix,
    // authed_json must still suppress the 404 — NOT call report_error.
    //
    // Since BackendClient strips the base path in `new()`, the path
    // passed to authed_json is always joined against the stripped base. We
    // verify that a PATCH 404 returns an error without panicking and that it
    // is not classified as a code bug (no Sentry event).
    //
    // #5230: the classification is `ChannelEditUnsupported`, NOT
    // `MessageNotFound`. The backend implements no `PATCH
    // /channels/:channel/messages/:messageId`, so this 404 is route absence.
    // Reporting it as a missing *message* made `bus.rs` forget a message id it
    // still owned, orphaning the streaming draft and the "💭 Thinking:" bubble.
    let app = axum::Router::new().route(
        "/channels/telegram/messages/9999",
        axum::routing::any(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_url = format!("http://{addr}");
    let client = BackendClient::new(&base_url).unwrap();

    let err = client
        .authed_json(
            "mock-jwt",
            Method::PATCH,
            "/channels/telegram/messages/9999",
            None,
        )
        .await
        .unwrap_err();
    let typed = err.downcast_ref::<BackendApiError>().unwrap();
    let BackendApiError::ChannelEditUnsupported {
        provider,
        message_id,
    } = typed
    else {
        panic!("expected ChannelEditUnsupported, got {typed:?}");
    };
    assert_eq!(provider, "telegram");
    assert_eq!(message_id, "9999");
}

#[tokio::test]
async fn send_channel_edit_404_is_route_absence_not_a_missing_message() {
    // #5230 root-cause pin, driven through the real client method rather than
    // `authed_json` directly: the deployed backend serves only
    // `POST /channels/:channel/messages` and
    // `DELETE /channels/:channel/messages/:messageId`, so every edit hits the
    // unmatched-route 404. It must NOT surface as `MessageNotFound` — that
    // variant means "this message is gone", and acting on it discards a
    // message id that is still live.
    // POST + DELETE only — deliberately mirrors the real backend router. The
    // explicit `fallback` reproduces Express's behaviour for an unmatched
    // method+path pair: it falls through to the app's 404 handler rather than
    // answering 405 (which is what axum would do on its own).
    let app = Router::new().route(
        "/channels/telegram/messages/1103",
        post(|| async { (axum::http::StatusCode::OK, "{\"success\":true}") })
            .delete(|| async { (axum::http::StatusCode::OK, "{\"success\":true}") })
            .fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .send_channel_edit("telegram", "1103", "mock-jwt", serde_json::json!({}))
        .await
        .unwrap_err();

    let typed = err
        .downcast_ref::<BackendApiError>()
        .expect("edit 404 must carry a typed BackendApiError");
    assert!(
        matches!(typed, BackendApiError::ChannelEditUnsupported { .. }),
        "edit 404 must be ChannelEditUnsupported, got {typed:?}"
    );
    assert!(
        !matches!(typed, BackendApiError::MessageNotFound { .. }),
        "a missing edit route must never masquerade as a deleted message"
    );
}

#[tokio::test]
async fn channel_edit_404_from_a_real_handler_stays_a_missing_message() {
    // #5230 review: the twin of the test above, for the world where the edit
    // route DOES exist (staging, a custom backend, or after the backend PR
    // lands). A handler answering "that message is gone" returns a JSON
    // envelope, exactly as `DELETE /channels/:channel/messages/:messageId`
    // already does. Classifying that as `ChannelEditUnsupported` would make
    // `bus.rs` call `mark_channel_edits_unsupported` and switch progressive
    // edits off for the whole provider for the rest of the process — because
    // one message expired.
    let app = Router::new().route(
        "/channels/telegram/messages/1103",
        axum::routing::patch(|| async {
            (
                axum::http::StatusCode::NOT_FOUND,
                "{\"success\":false,\"error\":\"message not found\"}",
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .send_channel_edit("telegram", "1103", "mock-jwt", serde_json::json!({}))
        .await
        .unwrap_err();

    let typed = err
        .downcast_ref::<BackendApiError>()
        .expect("edit 404 must carry a typed BackendApiError");
    assert!(
        matches!(typed, BackendApiError::MessageNotFound { .. }),
        "a handler-level edit 404 must stay per-message, got {typed:?}"
    );
    assert!(
        !matches!(typed, BackendApiError::ChannelEditUnsupported { .. }),
        "one missing message must not disable edits for the whole provider"
    );
}

#[tokio::test]
async fn channel_edit_404_on_a_prefixed_path_keeps_the_parsed_ids() {
    // A `BACKEND_URL` with a base-path prefix plus a trailing segment. The
    // sliding window in `parse_message_path` still finds
    // `[channels, telegram, messages, 9999]`, so this must be classified as
    // route absence for PATCH *and* carry the parsed ids — not fall through to
    // the generic untyped bail! that the DELETE branch uses.
    let app = axum::Router::new().route(
        "/api/v1/channels/telegram/messages/9999/extra",
        axum::routing::any(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .authed_json(
            "mock-jwt",
            Method::PATCH,
            "/api/v1/channels/telegram/messages/9999/extra",
            None,
        )
        .await
        .unwrap_err();

    let typed = err
        .downcast_ref::<BackendApiError>()
        .expect("prefixed edit path must still carry a typed error");
    match typed {
        BackendApiError::ChannelEditUnsupported {
            provider,
            message_id,
        } => {
            assert_eq!(provider, "telegram");
            assert_eq!(message_id, "9999");
        }
        other => panic!("expected ChannelEditUnsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn channel_edit_404_on_an_undecomposable_path_falls_back_to_unknown_ids() {
    // The case that actually exercises `authed_json`'s
    // `unwrap_or_else(("unknown", "unknown"))`: an empty message-id segment.
    // `parse_message_path` drops empty segments, so this yields only
    // `[channels, telegram, messages]` — no 4-window, hence `None` — while the
    // path still satisfies the `/channels/` + `/messages/` substring guard, so
    // the PATCH branch is entered with nothing parsed.
    let app = axum::Router::new().route(
        "/channels/telegram/messages/",
        axum::routing::any(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .authed_json(
            "mock-jwt",
            Method::PATCH,
            "/channels/telegram/messages/",
            None,
        )
        .await
        .unwrap_err();

    let typed = err
        .downcast_ref::<BackendApiError>()
        .expect("undecomposable edit path must still carry a typed error");
    match typed {
        BackendApiError::ChannelEditUnsupported {
            provider,
            message_id,
        } => {
            assert_eq!(provider, "unknown");
            assert_eq!(message_id, "unknown");
        }
        other => panic!("expected ChannelEditUnsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn channel_delete_404_still_means_the_message_is_gone() {
    // The fix must not widen: `DELETE` keeps `MessageNotFound`, whose
    // provider-side-deletion semantics are exactly what its caller
    // (`delete_channel_message`) wants — "already gone, nothing to clean up".
    let app = Router::new().route(
        "/channels/telegram/messages/1103",
        axum::routing::delete(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .send_channel_delete("telegram", "1103", "mock-jwt")
        .await
        .unwrap_err();

    let typed = err.downcast_ref::<BackendApiError>().unwrap();
    assert!(
        matches!(typed, BackendApiError::MessageNotFound { .. }),
        "DELETE 404 must stay MessageNotFound, got {typed:?}"
    );
}

// The channel methods below now reach the backend through the vendored
// `tinyhumans-sdk` transport instead of `authed_json`. The routes they call
// still classify expected backend states the same way — a route must not
// change its Sentry or session-expiry behaviour just because it moved onto a
// typed SDK method. These pin that equivalence.

#[tokio::test]
async fn sdk_backed_channel_delete_surfaces_message_not_found_on_404() {
    let app = Router::new().route(
        "/channels/telegram/messages/1103",
        axum::routing::delete(|| async { (axum::http::StatusCode::NOT_FOUND, "Not Found") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .send_channel_delete("telegram", "1103", "mock-jwt")
        .await
        .unwrap_err();

    let typed = err.downcast_ref::<BackendApiError>().unwrap();
    let BackendApiError::MessageNotFound {
        provider,
        message_id,
    } = typed
    else {
        panic!("expected MessageNotFound, got {typed:?}");
    };
    assert_eq!(provider, "telegram");
    assert_eq!(message_id, "1103");
}

#[tokio::test]
async fn sdk_backed_channel_typing_surfaces_unauthorized_on_401() {
    let app = Router::new().route(
        "/channels/telegram/typing",
        post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = BackendClient::new(&format!("http://{addr}")).unwrap();
    let err = client
        .send_channel_typing("telegram", "mock-jwt")
        .await
        .unwrap_err();

    let typed = err.downcast_ref::<BackendApiError>().unwrap();
    let BackendApiError::Unauthorized { method, path } = typed else {
        panic!("expected Unauthorized, got {typed:?}");
    };
    assert_eq!(method, "POST");
    assert_eq!(path, "/channels/telegram/typing");
    // The session-expiry sentinel must still be derivable, so the dispatcher
    // keeps routing this to re-sign-in rather than to Sentry.
    assert!(flatten_authed_error(err).starts_with("SESSION_EXPIRED:"));
}

#[test]
fn unmatched_route_404_is_expresss_html_page() {
    // Express's built-in finalhandler — what the backend returns today, since it
    // registers no catch-all 404 and implements no PATCH route.
    assert!(is_unmatched_route_404(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<title>Error</title>\n</head>\n<body>\n         <pre>Cannot PATCH /channels/telegram/messages/1103</pre>\n</body>\n</html>"
    ));
}

#[test]
fn unmatched_route_404_covers_empty_and_plain_text_bodies() {
    // Ambiguous shapes default to route absence, preserving today's behaviour.
    assert!(is_unmatched_route_404(""));
    assert!(is_unmatched_route_404("   "));
    assert!(is_unmatched_route_404("Not Found"));
}

#[test]
fn handler_level_404_json_envelope_is_not_route_absence() {
    // The shape `DELETE /channels/:channel/messages/:messageId` already returns,
    // and the one a future PATCH handler would mirror.
    assert!(!is_unmatched_route_404(
        r#"{"success": false, "error": "message not found"}"#
    ));
    assert!(!is_unmatched_route_404(r#"{"error":"gone"}"#));
}
