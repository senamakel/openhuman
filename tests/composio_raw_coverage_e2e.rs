//! Raw-line oriented coverage for deterministic Composio helpers.
//!
//! These tests avoid live Composio/backend calls and exercise public helper
//! surfaces that feed the JSON-RPC and agent-tool paths.

use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::tempdir;

use openhuman_core::openhuman::composio::error_mapping::{
    classify_composio_error, format_provider_error, remap_transport_error, ComposioErrorClass,
};
use openhuman_core::openhuman::composio::execute_prepare::prepare_execute_arguments;
use openhuman_core::openhuman::composio::oauth_handoff::{
    is_authorize_rate_limited, is_clearable_oauth_status, is_inflight_oauth_status,
    is_meta_oauth_toolkit, meta_oauth_rate_limit_message, wrap_authorize_rate_limit_error,
};
use openhuman_core::openhuman::composio::ComposioActionTool;
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::tools::{PermissionLevel, Tool, ToolCategory};

#[test]
fn composio_prepare_execute_arguments_normalizes_calendar_and_notion_payloads() {
    let calendar = prepare_execute_arguments(
        " GOOGLECALENDAR_EVENTS_LIST ",
        Some(json!({
            "timeMin": "2026-05-29",
            "time_max": "2026-05-30T15:00:00-07:00"
        })),
    )
    .expect("calendar args should normalize");
    assert_eq!(
        calendar.get("timeMin").and_then(Value::as_str),
        Some("2026-05-29T00:00:00Z")
    );
    assert_eq!(
        calendar.get("time_max").and_then(Value::as_str),
        Some("2026-05-30T15:00:00-07:00")
    );

    let invalid_date = prepare_execute_arguments(
        "GOOGLECALENDAR_FIND_EVENT",
        Some(json!({ "timeMax": "2026-99-99" })),
    )
    .expect_err("invalid bare dates should be rejected");
    assert!(invalid_date.contains("RFC 3339 timestamp"));

    let notion_pages = prepare_execute_arguments(
        "NOTION_FETCH_DATA",
        Some(json!({ "filter": { "value": "page" } })),
    )
    .expect("notion page filter should infer fetch type");
    assert_eq!(
        notion_pages.get("fetch_type").and_then(Value::as_str),
        Some("pages")
    );

    let notion_database = prepare_execute_arguments(
        "NOTION_FETCH_DATA",
        Some(json!({
            "fetchType": "databases",
            "filter": { "property": "page" }
        })),
    )
    .expect("explicit fetch type should win");
    assert_eq!(notion_database.get("fetch_type"), None);
    assert_eq!(
        notion_database.get("fetchType").and_then(Value::as_str),
        Some("databases")
    );
}

#[test]
fn composio_prepare_execute_arguments_validates_gmail_mutations() {
    let empty = prepare_execute_arguments("GMAIL_SEND_EMAIL", None)
        .expect_err("gmail send needs a recipient");
    assert!(empty.contains("recipient"));

    let send = prepare_execute_arguments(
        "GMAIL_SEND_EMAIL",
        Some(json!({ "recipientEmail": "person@example.test", "subject": "Hi" })),
    )
    .expect("recipientEmail alias should be accepted");
    assert_eq!(
        send.get("recipientEmail").and_then(Value::as_str),
        Some("person@example.test")
    );

    let missing_message = prepare_execute_arguments(
        "GMAIL_ADD_LABEL_TO_EMAIL",
        Some(json!({ "addLabelIds": ["Label_1"] })),
    )
    .expect_err("gmail add label needs a message id");
    assert!(missing_message.contains("message_id"));

    let missing_labels = prepare_execute_arguments(
        "GMAIL_ADD_LABEL_TO_EMAIL",
        Some(json!({ "messageId": "msg-1", "addLabelIds": ["  "] })),
    )
    .expect_err("gmail add label needs at least one non-empty label");
    assert!(missing_labels.contains("at least one"));

    let labeled = prepare_execute_arguments(
        "GMAIL_ADD_LABEL_TO_EMAIL",
        Some(json!({ "messageId": "msg-1", "remove_label_ids": "Label_2" })),
    )
    .expect("string label alias should be accepted");
    assert_eq!(
        labeled.get("messageId").and_then(Value::as_str),
        Some("msg-1")
    );

    let non_object = prepare_execute_arguments("GMAIL_SEND_EMAIL", Some(json!("bad")))
        .expect_err("arguments must be an object");
    assert!(non_object.contains("must be a JSON object"));
}

#[test]
fn composio_error_mapping_classifies_and_formats_provider_failures() {
    assert_eq!(
        classify_composio_error("GMAIL_SEND_EMAIL", "missing required field to"),
        ComposioErrorClass::Validation
    );
    assert_eq!(
        classify_composio_error(
            "GMAIL_FETCH_EMAILS",
            "403 insufficient authentication scopes for Gmail"
        ),
        ComposioErrorClass::InsufficientScope
    );
    assert_eq!(
        classify_composio_error("SLACK_POST_MESSAGE", "429 too many requests"),
        ComposioErrorClass::RateLimited
    );
    assert_eq!(
        classify_composio_error("GMAIL_FETCH_EMAILS", "Mailbox provider exploded"),
        ComposioErrorClass::UpstreamProvider
    );
    assert_eq!(
        classify_composio_error("CUSTOM_ACTION", "connection error, try to authenticate"),
        ComposioErrorClass::ComposioPlatform
    );
    assert_eq!(
        classify_composio_error("CUSTOM_ACTION", "502 Bad Gateway"),
        ComposioErrorClass::Gateway
    );
    assert_eq!(
        classify_composio_error("CUSTOM_ACTION", "plain unknown failure"),
        ComposioErrorClass::Other
    );

    let scope = format_provider_error(
        "GMAIL_FETCH_EMAILS",
        "insufficient authentication scopes: gmail.readonly",
    );
    assert!(scope.starts_with("[composio:error:insufficient_scope]"));
    assert!(scope.contains("Reconnect the integration"));

    let gateway = remap_transport_error(
        "GMAIL_FETCH_EMAILS",
        "Backend returned 502 Bad Gateway for POST: {\"error\":\"insufficient scope\"}",
    );
    assert!(
        gateway.starts_with("[composio:error:insufficient_scope]"),
        "embedded provider errors should not be bucketed as gateway: {gateway}"
    );
}

#[test]
fn composio_oauth_handoff_helpers_classify_meta_status_and_rate_limits() {
    assert!(is_meta_oauth_toolkit(" Instagram "));
    assert!(is_meta_oauth_toolkit("FACEBOOK"));
    assert!(!is_meta_oauth_toolkit("gmail"));

    for status in ["pending", "INITIATED", " initializing "] {
        assert!(
            is_inflight_oauth_status(status),
            "{status} should be inflight"
        );
        assert!(
            is_clearable_oauth_status(status),
            "{status} should be clearable"
        );
    }
    for status in ["failed", "ERROR", " expired "] {
        assert!(!is_inflight_oauth_status(status));
        assert!(is_clearable_oauth_status(status));
    }
    assert!(!is_clearable_oauth_status("ACTIVE"));

    for message in ["HTTP 429", "too many requests", "rate_limit", "ratelimited"] {
        assert!(is_authorize_rate_limited(message));
    }
    assert!(!is_authorize_rate_limited("plain auth failure"));

    let instagram = meta_oauth_rate_limit_message("instagram");
    assert!(instagram.contains("Instagram Business or Creator"));
    let facebook = meta_oauth_rate_limit_message("facebook");
    assert!(facebook.contains("Business Manager"));
    let unknown = meta_oauth_rate_limit_message("threads");
    assert!(!unknown.contains("Business Manager"));

    let wrapped =
        wrap_authorize_rate_limit_error("instagram", anyhow::anyhow!("429 too many requests"));
    assert!(wrapped.to_string().contains("temporarily rate-limiting"));
    let passthrough = wrap_authorize_rate_limit_error("gmail", anyhow::anyhow!("429"));
    assert_eq!(passthrough.to_string(), "429");
}

#[test]
fn composio_action_tool_metadata_is_stable_without_network_execution() {
    let dir = tempdir().expect("tempdir");
    let config = Config {
        workspace_dir: dir.path().to_path_buf(),
        config_path: dir.path().join("config.toml"),
        ..Config::default()
    };
    let tool = ComposioActionTool::new(
        Arc::new(config),
        "GMAIL_SEND_EMAIL".into(),
        "Send an email".into(),
        Some(json!({
            "type": "object",
            "properties": { "to": { "type": "string" } }
        })),
    );

    assert_eq!(tool.name(), "GMAIL_SEND_EMAIL");
    assert_eq!(tool.description(), "Send an email");
    assert_eq!(tool.permission_level(), PermissionLevel::Write);
    assert_eq!(tool.category(), ToolCategory::Skill);
    assert_eq!(
        tool.parameters_schema().pointer("/properties/to/type"),
        Some(&json!("string"))
    );

    let default_schema = ComposioActionTool::new(
        Arc::new(Config::default()),
        "NOTION_FETCH_DATA".into(),
        "Fetch Notion data".into(),
        None,
    );
    assert_eq!(
        default_schema.parameters_schema(),
        json!({ "type": "object" })
    );
}
