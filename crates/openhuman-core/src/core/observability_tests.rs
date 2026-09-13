use super::*;

#[cfg(feature = "crash-reporting")]
fn event_with_tags(pairs: &[(&str, &str)]) -> sentry::protocol::Event<'static> {
    let mut event = sentry::protocol::Event::default();
    let mut tags: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (k, v) in pairs {
        tags.insert((*k).to_string(), (*v).to_string());
    }
    event.tags = tags;
    event
}

#[cfg(feature = "crash-reporting")]
fn event_with_message(msg: &str) -> sentry::protocol::Event<'static> {
    let mut event = sentry::protocol::Event::default();
    event.message = Some(msg.to_string());
    event
}

#[cfg(feature = "crash-reporting")]
fn channel_message_404_event(method: &str) -> sentry::protocol::Event<'static> {
    let mut event = sentry::protocol::Event::default();
    event.tags.insert("domain".into(), "backend_api".into());
    event.tags.insert("failure".into(), "non_2xx".into());
    event.tags.insert("status".into(), "404".into());
    event.tags.insert("method".into(), method.into());
    event.message = Some(
        "PATCH /channels/telegram/messages/1103 failed (404); response_body_len=172".to_string(),
    );
    event
}

fn managed_body(status: &str, code: &str) -> String {
    format!(
        "OpenHuman API error ({status}): {{\"error\":{{\"errorCode\":\"{code}\",\"message\":\"x\"}}}}"
    )
}

#[cfg(feature = "crash-reporting")]
fn auth_get_me_tags() -> Vec<(&'static str, &'static str)> {
    vec![
        ("domain", "rpc"),
        ("operation", "invoke_method"),
        ("method", "openhuman.auth_get_me"),
        ("elapsed_ms", "5003"),
    ]
}

#[cfg(feature = "crash-reporting")]
fn event_with_tags_and_message(
    pairs: &[(&str, &str)],
    message: &str,
) -> sentry::protocol::Event<'static> {
    let mut event = event_with_tags(pairs);
    event.message = Some(message.to_string());
    event
}

#[cfg(feature = "crash-reporting")]
fn event_with_exception_value(value: &str) -> sentry::protocol::Event<'static> {
    let mut event = sentry::protocol::Event::default();
    event.exception = vec![sentry::protocol::Exception {
        value: Some(value.to_string()),
        ..Default::default()
    }]
    .into();
    event
}

#[path = "observability_crash_filter_auth_tests.rs"]
mod crash_filter_auth;
#[path = "observability_crash_filter_integrations_tests.rs"]
mod crash_filter_integrations;
#[path = "observability_crash_filter_messages_tests.rs"]
mod crash_filter_messages;
#[path = "observability_error_classification_core_tests.rs"]
mod error_classification_core;
#[path = "observability_error_classification_network_tests.rs"]
mod error_classification_network;
#[path = "observability_error_classification_provider_tests.rs"]
mod error_classification_provider;
#[path = "observability_error_classification_session_tests.rs"]
mod error_classification_session;
#[path = "observability_error_classification_user_state_tests.rs"]
mod error_classification_user_state;
