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
use openhuman_core::openhuman::composio::trigger_history::ComposioTriggerHistoryStore;
use openhuman_core::openhuman::composio::types::{
    ComposioActiveTrigger, ComposioActiveTriggersResponse, ComposioAvailableTrigger,
    ComposioAvailableTriggerRepo, ComposioAvailableTriggersResponse, ComposioCapabilitiesResponse,
    ComposioCapability, ComposioConnection, ComposioConnectionsResponse,
    ComposioCreateTriggerResponse, ComposioDeleteResponse, ComposioDisableTriggerResponse,
    ComposioEnableTriggerResponse, ComposioExecuteResponse, ComposioGithubRepo,
    ComposioGithubReposResponse, ComposioToolFunction, ComposioToolSchema,
    ComposioToolkitsResponse, ComposioToolsResponse, ComposioTriggerEvent,
    ComposioTriggerHistoryEntry, ComposioTriggerMetadata,
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

#[test]
fn composio_types_roundtrip_connection_tool_trigger_and_history_shapes() {
    let toolkits: ComposioToolkitsResponse = serde_json::from_value(json!({})).unwrap();
    assert!(toolkits.toolkits.is_empty());

    let capabilities = ComposioCapabilitiesResponse {
        capabilities: vec![ComposioCapability {
            toolkit: "gmail".into(),
            description: "Gmail".into(),
            native_provider: true,
            curated_tools: true,
            curated_tool_count: 3,
            tool_execution: true,
            user_profile: true,
            initial_sync: true,
            periodic_sync: true,
            sync_interval_secs: Some(3600),
            trigger_webhooks: true,
            memory_ingest: true,
        }],
    };
    assert_eq!(
        serde_json::to_value(&capabilities).unwrap()["capabilities"][0]["toolkit"],
        "gmail"
    );

    let connections: ComposioConnectionsResponse = serde_json::from_value(json!({
        "connections": [
            { "id": "c1", "toolkit": " Gmail ", "status": " connected ", "createdAt": "2026-05-29T00:00:00Z" },
            { "id": "c2", "toolkit": "slack", "status": "PENDING" }
        ]
    }))
    .unwrap();
    assert_eq!(connections.connections[0].normalized_toolkit(), "gmail");
    assert!(connections.connections[0].is_active());
    assert!(!connections.connections[1].is_active());
    let serialized_connection = serde_json::to_value(&connections.connections[0]).unwrap();
    assert_eq!(serialized_connection["createdAt"], "2026-05-29T00:00:00Z");

    let default_connection = ComposioConnection {
        id: "c3".into(),
        toolkit: "notion".into(),
        status: "FAILED".into(),
        created_at: None,
    };
    assert!(serde_json::to_value(default_connection)
        .unwrap()
        .get("createdAt")
        .is_none());

    let tools = ComposioToolsResponse {
        tools: vec![ComposioToolSchema {
            kind: "function".into(),
            function: ComposioToolFunction {
                name: "GMAIL_SEND_EMAIL".into(),
                description: Some("Send mail".into()),
                parameters: Some(json!({ "type": "object" })),
            },
        }],
    };
    assert_eq!(
        serde_json::to_value(&tools).unwrap()["tools"][0]["type"],
        "function"
    );
    let default_kind: ComposioToolSchema = serde_json::from_value(json!({
        "function": { "name": "SLACK_SENDS_A_MESSAGE_TO_A_SLACK_CHANNEL" }
    }))
    .unwrap();
    assert_eq!(default_kind.kind, "function");
    assert_eq!(default_kind.function.description, None);

    let execute: ComposioExecuteResponse = serde_json::from_value(json!({
        "data": { "id": "msg-1" },
        "successful": true,
        "costUsd": 0.03,
        "markdownFormatted": "**sent**"
    }))
    .unwrap();
    assert!(execute.successful);
    assert_eq!(execute.cost_usd, 0.03);
    assert_eq!(execute.markdown_formatted.as_deref(), Some("**sent**"));

    let repos = ComposioGithubReposResponse {
        connection_id: "conn-github".into(),
        repositories: vec![ComposioGithubRepo {
            owner: "tinyhumansai".into(),
            repo: "openhuman".into(),
            full_name: "tinyhumansai/openhuman".into(),
            private: Some(false),
            default_branch: Some("main".into()),
            html_url: Some("https://github.com/tinyhumansai/openhuman".into()),
        }],
    };
    assert_eq!(
        serde_json::to_value(&repos).unwrap()["connectionId"],
        "conn-github"
    );

    let create = ComposioCreateTriggerResponse {
        trigger_id: "trig-1".into(),
        status: Some("enabled".into()),
    };
    assert_eq!(
        serde_json::to_value(&create).unwrap()["triggerId"],
        "trig-1"
    );
    let available = ComposioAvailableTriggersResponse {
        triggers: vec![ComposioAvailableTrigger {
            slug: "GITHUB_PULL_REQUEST_EVENT".into(),
            scope: "github_repo".into(),
            default_config: Some(json!({ "event": "pull_request" })),
            required_config_keys: Some(vec!["owner".into(), "repo".into()]),
            repo: Some(ComposioAvailableTriggerRepo {
                owner: "tinyhumansai".into(),
                repo: "openhuman".into(),
            }),
        }],
    };
    assert_eq!(
        serde_json::to_value(&available).unwrap()["triggers"][0]["repo"]["repo"],
        "openhuman"
    );

    let active: ComposioActiveTriggersResponse = serde_json::from_value(json!({
        "triggers": [{
            "id": { "id": "trigger-id" },
            "slug": { "slug": "GMAIL_NEW_GMAIL_MESSAGE" },
            "toolkit": { "name": "gmail" },
            "connectionId": { "key": "conn-1" },
            "triggerConfig": { "label": "INBOX" },
            "state": { "state": "enabled" }
        }]
    }))
    .unwrap();
    let active_trigger: &ComposioActiveTrigger = &active.triggers[0];
    assert_eq!(active_trigger.id, "trigger-id");
    assert_eq!(active_trigger.slug, "GMAIL_NEW_GMAIL_MESSAGE");
    assert_eq!(active_trigger.toolkit, "gmail");
    assert_eq!(active_trigger.connection_id, "conn-1");
    assert_eq!(active_trigger.state.as_deref(), Some("enabled"));
    let active_without_state: ComposioActiveTrigger = serde_json::from_value(json!({
        "id": "trigger-2",
        "slug": "SLACK_NEW_MESSAGE",
        "toolkit": "slack",
        "connectionId": "conn-2",
        "state": { "unexpected": true }
    }))
    .unwrap();
    assert_eq!(active_without_state.state, None);
    assert!(serde_json::from_value::<ComposioActiveTrigger>(json!({
        "id": ["bad"],
        "slug": "x",
        "toolkit": "gmail",
        "connectionId": "c"
    }))
    .is_err());

    let enable = ComposioEnableTriggerResponse {
        trigger_id: "trig-2".into(),
        slug: "SLACK_NEW_MESSAGE".into(),
        connection_id: "conn-2".into(),
    };
    assert_eq!(
        serde_json::to_value(&enable).unwrap()["connectionId"],
        "conn-2"
    );
    assert!(
        serde_json::to_value(ComposioDisableTriggerResponse { deleted: false })
            .unwrap()
            .get("deleted")
            .is_some()
    );
    assert_eq!(
        serde_json::to_value(ComposioDeleteResponse {
            deleted: true,
            memory_chunks_deleted: 4,
        })
        .unwrap()["memory_chunks_deleted"],
        4
    );

    let event: ComposioTriggerEvent = serde_json::from_value(json!({
        "toolkit": "gmail",
        "trigger": "GMAIL_NEW_GMAIL_MESSAGE",
        "payload": { "subject": "coverage" },
        "metadata": { "id": "m1", "uuid": "u1" }
    }))
    .unwrap();
    assert_eq!(event.metadata.id, "m1");
    assert_eq!(event.payload["subject"], "coverage");
    let default_event: ComposioTriggerEvent = serde_json::from_value(json!({})).unwrap();
    assert_eq!(default_event.metadata.uuid, "");
    let metadata = ComposioTriggerMetadata {
        id: "m2".into(),
        uuid: "u2".into(),
    };
    assert_eq!(serde_json::to_value(metadata).unwrap()["uuid"], "u2");
    let entry = ComposioTriggerHistoryEntry {
        received_at_ms: 42,
        toolkit: "gmail".into(),
        trigger: "GMAIL_NEW_GMAIL_MESSAGE".into(),
        metadata_id: "m1".into(),
        metadata_uuid: "u1".into(),
        payload: json!({ "subject": "coverage" }),
    };
    assert_eq!(serde_json::to_value(entry).unwrap()["received_at_ms"], 42);
}

#[test]
fn composio_trigger_history_store_handles_limits_and_bad_archive_lines() {
    let dir = tempdir().expect("tempdir");
    let store = ComposioTriggerHistoryStore::new(dir.path()).expect("history store");
    let empty = store.list_recent(0).expect("empty history");
    assert!(empty.entries.is_empty());
    assert!(empty.archive_dir.ends_with("state/triggers"));

    let first = store
        .record_trigger(
            "gmail",
            "GMAIL_NEW_GMAIL_MESSAGE",
            "metadata-1",
            "uuid-1",
            &json!({ "subject": "first" }),
        )
        .expect("record first");
    assert_eq!(first.toolkit, "gmail");
    let second = store
        .record_trigger(
            "slack",
            "SLACK_NEW_MESSAGE",
            "metadata-2",
            "uuid-2",
            &json!({ "text": "second" }),
        )
        .expect("record second");
    assert!(second.received_at_ms >= first.received_at_ms);

    let one = store.list_recent(1).expect("limited history");
    assert_eq!(one.entries.len(), 1);
    assert_eq!(one.entries[0].metadata_id, "metadata-2");

    std::fs::write(
        dir.path()
            .join("state")
            .join("triggers")
            .join("1999-01-01.jsonl"),
        "\nnot-json\n{\"received_at_ms\":1,\"toolkit\":\"old\",\"trigger\":\"OLD\",\"metadata_id\":\"m\",\"metadata_uuid\":\"u\",\"payload\":{}}\n",
    )
    .expect("write legacy archive");
    let all = store.list_recent(10).expect("history skips bad lines");
    assert!(all.entries.iter().any(|entry| entry.toolkit == "old"));
    assert!(all
        .entries
        .iter()
        .any(|entry| entry.metadata_id == "metadata-1"));
}
