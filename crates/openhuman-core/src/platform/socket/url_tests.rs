use super::*;

#[test]
fn credential_socket_endpoint_requires_tls_or_loopback() {
    assert!(is_safe_socket_endpoint(
        "wss://api.tinyhumans.ai/socket.io/"
    ));
    assert!(is_safe_socket_endpoint("ws://127.0.0.1:3000/socket.io/"));
    assert!(!is_safe_socket_endpoint(
        "ws://api.tinyhumans.ai/socket.io/"
    ));
    assert!(!is_safe_socket_endpoint("ws://example.com/socket.io/"));
    assert!(!is_safe_socket_endpoint(
        "ws://127.attacker.example/socket.io/"
    ));
}

#[test]
fn converts_https_to_wss() {
    let url = websocket_url("https://api.tinyhumans.ai");
    assert_eq!(
        url,
        "wss://api.tinyhumans.ai/socket.io/?EIO=4&transport=websocket"
    );
}

#[test]
fn converts_http_to_ws() {
    let url = websocket_url("http://localhost:3000");
    assert_eq!(
        url,
        "ws://localhost:3000/socket.io/?EIO=4&transport=websocket"
    );
}

#[test]
fn passes_through_unknown_scheme() {
    let url = websocket_url("ftp://example.com");
    assert_eq!(
        url,
        "ftp://example.com/socket.io/?EIO=4&transport=websocket"
    );
}

#[test]
fn strips_trailing_slash() {
    let url = websocket_url("https://api.tinyhumans.ai/");
    assert_eq!(
        url,
        "wss://api.tinyhumans.ai/socket.io/?EIO=4&transport=websocket"
    );
}

#[test]
fn strips_multiple_trailing_slashes() {
    let url = websocket_url("https://api.tinyhumans.ai///");
    assert_eq!(
        url,
        "wss://api.tinyhumans.ai/socket.io/?EIO=4&transport=websocket"
    );
}
