use super::*;

#[test]
fn base_urls_come_from_the_transport_and_honour_the_override() {
    // Under `cfg(test)` the plain transport answers: the configured origin,
    // or a loopback discard port when nothing is configured.
    let configured = Some(" http://127.0.0.1:4010/openai/v1/chat/completions ".to_string());
    assert_eq!(base_url(&configured).unwrap(), "http://127.0.0.1:4010");
    assert_eq!(
        inference_base_url(&configured).unwrap(),
        "http://127.0.0.1:4010"
    );
    assert_eq!(
        base_url(&Some("   ".to_string())).unwrap(),
        transport::plain::TEST_FALLBACK_BASE_URL
    );
    assert_eq!(
        base_url(&None).unwrap(),
        transport::plain::TEST_FALLBACK_BASE_URL
    );
}

#[test]
fn product_identity_comes_from_the_transport() {
    assert_eq!(
        product_identity().as_deref(),
        Some(transport::plain::TEST_PRODUCT_IDENTITY)
    );
}

#[test]
fn require_base_url_resolves_through_the_transport() {
    let configured = Some("http://127.0.0.1:4010".to_string());
    assert_eq!(
        require_base_url(&configured).unwrap(),
        "http://127.0.0.1:4010"
    );
}

#[test]
fn attribution_headers_come_from_the_transport() {
    let headers = attribution_headers();
    assert_eq!(
        headers
            .get(transport::plain::TEST_PRODUCT_HEADER)
            .and_then(|v| v.to_str().ok()),
        Some(transport::plain::TEST_PRODUCT_IDENTITY)
    );
}
