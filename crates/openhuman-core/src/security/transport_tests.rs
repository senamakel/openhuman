use super::*;

#[test]
fn bearer_transport_allows_tls_and_only_http_loopback() {
    assert!(is_safe_endpoint_for_bearer("https://api.example.test/v1"));
    assert!(is_safe_endpoint_for_bearer("http://localhost:8080/v1"));
    assert!(is_safe_endpoint_for_bearer("http://[::1]:8080/v1"));
    assert!(is_safe_endpoint_for_bearer("http://127.0.0.1:8080/v1"));
    assert!(is_safe_endpoint_for_bearer("http://127.200.10.20:8080/v1"));

    assert!(!is_safe_endpoint_for_bearer("http://192.168.1.10:8080/v1"));
    assert!(!is_safe_endpoint_for_bearer(
        "http://127.attacker.example/v1"
    ));
    assert!(!is_safe_endpoint_for_bearer("ftp://localhost/v1"));
    assert!(!is_safe_endpoint_for_bearer("not a url"));
}
