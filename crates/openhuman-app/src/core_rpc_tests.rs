use super::redact_url_for_log;
use super::relay_bearer_header;

#[test]
fn bearer_header_present_for_real_token() {
    assert_eq!(
        relay_bearer_header(Some("tok123")).as_deref(),
        Some("Bearer tok123")
    );
    // Surrounding whitespace is trimmed before formatting.
    assert_eq!(
        relay_bearer_header(Some("  tok123  ")).as_deref(),
        Some("Bearer tok123")
    );
}

#[test]
fn bearer_header_absent_for_missing_or_blank_token() {
    assert_eq!(relay_bearer_header(None), None);
    assert_eq!(relay_bearer_header(Some("")), None);
    assert_eq!(relay_bearer_header(Some("   ")), None);
}

#[test]
fn redact_strips_credentials_query_and_path() {
    // Userinfo, query, fragment, and path must not survive into logs; only
    // the scheme://host[:port] surface is kept for transport diagnostics.
    assert_eq!(
        redact_url_for_log("http://user:pass@192.168.1.74:7788/rpc/secret?token=t0k#frag"),
        "http://192.168.1.74:7788/"
    );
    assert_eq!(
        redact_url_for_log("https://core.example.com/rpc"),
        "https://core.example.com/"
    );
    // An unparseable URL degrades to the coarse sentinel.
    assert_eq!(redact_url_for_log("not a url"), "<invalid relay url>");
}

/// A non-loopback `http` URL carrying a bearer must be refused, and the
/// surfaced error must carry the redacted `scheme://host[:port]` form —
/// never the raw secret-bearing userinfo, path, or query (CWE-532).
#[cfg(feature = "gateways")]
#[tokio::test]
async fn insecure_transport_refusal_redacts_url() {
    let err = match super::post_json_rpc(
        "http://user:pass@192.168.1.74:7788/rpc/secret?token=t0k",
        Some("bearer-tok"),
        "body".to_string(),
    )
    .await
    {
        Err(e) => e,
        Ok(_) => panic!("insecure non-loopback + bearer must be refused"),
    };
    assert!(
        !err.contains("pass"),
        "raw userinfo leaked into refusal error: {err}"
    );
    assert!(
        !err.contains("t0k"),
        "raw query token leaked into refusal error: {err}"
    );
    assert!(
        !err.contains("/secret"),
        "raw path leaked into refusal error: {err}"
    );
    assert!(
        err.contains("http://192.168.1.74:7788/"),
        "redacted host should remain for diagnostics: {err}"
    );
}
