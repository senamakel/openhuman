use super::*;

#[test]
fn parses_get_request_target() {
    let head = "GET /auth?token=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(
        parse_request_target(head),
        Some("/auth?token=abc&state=xyz")
    );
}

#[test]
fn rejects_non_get_methods() {
    let head = "POST /auth HTTP/1.1\r\n\r\n";
    assert_eq!(parse_request_target(head), None);
}

#[test]
fn extracts_state_value() {
    assert_eq!(extract_state("token=abc&state=xyz"), Some("xyz"));
    assert_eq!(extract_state("state=only"), Some("only"));
    assert_eq!(extract_state("token=abc"), None);
    assert_eq!(extract_state(""), None);
}

#[tokio::test]
async fn random_state_is_32_hex_chars() {
    let s = random_state_nonce();
    assert_eq!(s.len(), 32);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
}

// ── classify_request ────────────────────────────────────────────────────

fn auth_head(query: &str) -> String {
    format!("GET /auth{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
}

#[test]
fn classify_valid_auth_request_returns_callback_url() {
    let head = auth_head("?token=jwt&state=deadbeef");
    let outcome = classify_request(&head, "deadbeef", 53824);
    assert_eq!(
        outcome,
        RequestOutcome::AuthCallback {
            callback_url: "http://127.0.0.1:53824/auth?token=jwt&state=deadbeef".to_string()
        }
    );
}

#[test]
fn classify_wrong_state_returns_state_mismatch() {
    let head = auth_head("?token=jwt&state=wrong");
    assert_eq!(
        classify_request(&head, "correct", 53824),
        RequestOutcome::StateMismatch
    );
}

#[test]
fn classify_missing_state_returns_state_mismatch() {
    let head = auth_head("?token=jwt");
    assert_eq!(
        classify_request(&head, "expected", 53824),
        RequestOutcome::StateMismatch
    );
}

#[test]
fn classify_no_query_string_on_auth_path_returns_state_mismatch() {
    let head = "GET /auth HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(
        classify_request(head, "nonce", 53824),
        RequestOutcome::StateMismatch
    );
}

#[test]
fn classify_favicon_returns_not_found() {
    let head = "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(
        classify_request(head, "state", 53824),
        RequestOutcome::NotFound
    );
}

#[test]
fn classify_root_path_returns_not_found() {
    let head = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(
        classify_request(head, "state", 53824),
        RequestOutcome::NotFound
    );
}

#[test]
fn classify_post_method_returns_method_not_allowed() {
    let head = "POST /auth?state=abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(
        classify_request(head, "abc", 53824),
        RequestOutcome::MethodNotAllowed
    );
}

#[test]
fn classify_callback_url_uses_bound_port() {
    let head = auth_head("?state=s&token=t");
    let outcome = classify_request(&head, "s", 12345);
    assert_eq!(
        outcome,
        RequestOutcome::AuthCallback {
            callback_url: "http://127.0.0.1:12345/auth?state=s&token=t".to_string()
        }
    );
}

#[test]
fn classify_state_only_query_returns_callback() {
    // Minimal valid request: only state param, no other query params.
    let head = auth_head("?state=abc123");
    assert_eq!(
        classify_request(&head, "abc123", 53824),
        RequestOutcome::AuthCallback {
            callback_url: "http://127.0.0.1:53824/auth?state=abc123".to_string()
        }
    );
}

// ── bind_loopback (integration: real OS socket) ─────────────────────────

#[tokio::test]
async fn bind_loopback_succeeds_on_ephemeral_port() {
    let listener = bind_loopback(0).expect("bind on port 0 must succeed");
    let addr = listener.local_addr().expect("must have local addr");
    assert!(addr.ip().is_loopback());
    assert_ne!(addr.port(), 0, "OS should assign a non-zero ephemeral port");
}

#[tokio::test]
async fn bind_loopback_allows_rebind_via_so_reuseaddr() {
    // Bind once, drop the listener, then bind again on the same port. The
    // short TIME_WAIT window should not block the rebind because we set
    // SO_REUSEADDR.
    let listener = bind_loopback(0).expect("first bind");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _ = bind_loopback(port).expect("rebind on same port must succeed with SO_REUSEADDR");
}
