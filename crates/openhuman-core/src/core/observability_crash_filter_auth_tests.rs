use super::*;

#[cfg(feature = "crash-reporting")]
#[test]
fn auth_get_me_opaque_filter_keeps_full_anyhow_chain_message() {
    // Post-fix shape from `auth_get_me` now using `format!("{e:#}")`.
    let event = event_with_tags_and_message(
        &auth_get_me_tags(),
        "GET /auth/me: error sending request for url \
         (https://api.tinyhumans.ai/auth/me): operation timed out",
    );
    assert!(
        !is_auth_get_me_opaque_transport_event(&event),
        "messages carrying the underlying transport chain must surface — \
         the transient classifier handles those at the rpc dispatcher"
    );
}

#[cfg(feature = "crash-reporting")]
#[test]
fn auth_get_me_opaque_filter_keeps_other_rpc_methods() {
    // Same opaque shape but for a different RPC must NOT be dropped —
    // we don't have evidence the same anti-pattern exists elsewhere,
    // and a path-only body might be a legitimate distinct error for a
    // future endpoint.
    for method in [
        "openhuman.consume_login_token",
        "openhuman.auth_create_channel_link_token",
        "openhuman.thread_list",
    ] {
        let mut tags = auth_get_me_tags();
        // Replace the method tag.
        if let Some(slot) = tags.iter_mut().find(|(k, _)| *k == "method") {
            slot.1 = method;
        }
        let event = event_with_tags_and_message(&tags, "GET /auth/me");
        assert!(
            !is_auth_get_me_opaque_transport_event(&event),
            "filter must be scoped strictly to method=openhuman.auth_get_me \
             — saw method={method}"
        );
    }
}

#[cfg(feature = "crash-reporting")]
#[test]
fn auth_get_me_opaque_filter_requires_rpc_invoke_method_domain() {
    // Wrong domain → must surface.
    let mut tags = auth_get_me_tags();
    if let Some(slot) = tags.iter_mut().find(|(k, _)| *k == "domain") {
        slot.1 = "backend_api";
    }
    let event = event_with_tags_and_message(&tags, "GET /auth/me");
    assert!(!is_auth_get_me_opaque_transport_event(&event));

    // Wrong operation → must surface.
    let mut tags = auth_get_me_tags();
    if let Some(slot) = tags.iter_mut().find(|(k, _)| *k == "operation") {
        slot.1 = "post";
    }
    let event = event_with_tags_and_message(&tags, "GET /auth/me");
    assert!(!is_auth_get_me_opaque_transport_event(&event));
}

#[cfg(feature = "crash-reporting")]
#[test]
fn auth_get_me_opaque_filter_matches_exception_value_path() {
    // sentry-tracing path: message empty, exception last value carries
    // the body. The filter must still match.
    let mut event = event_with_tags(&auth_get_me_tags());
    event.exception.values.push(sentry::protocol::Exception {
        value: Some("GET /auth/me".to_string()),
        ..Default::default()
    });
    assert!(
        is_auth_get_me_opaque_transport_event(&event),
        "must also catch the exception-value shape (sentry-tracing bridge)"
    );
}

#[cfg(feature = "crash-reporting")]
#[test]
fn auth_get_me_opaque_filter_ignores_empty_and_unrelated() {
    // No message and no exception → false.
    let event = event_with_tags(&auth_get_me_tags());
    assert!(!is_auth_get_me_opaque_transport_event(&event));

    // Unrelated message body with the right tags → false.
    let event = event_with_tags_and_message(
        &auth_get_me_tags(),
        "session JWT verified via GET /auth/me on https://api.tinyhumans.ai",
    );
    assert!(
        !is_auth_get_me_opaque_transport_event(&event),
        "substring match must NOT trigger — strict equality only"
    );
}
