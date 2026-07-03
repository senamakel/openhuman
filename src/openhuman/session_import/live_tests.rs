//! Write-side parity for the live session-store dual-write (issue #4249, 04.1).
//!
//! Drives the two persistence paths — the legacy authoritative JSONL writer
//! (`transcript::write_transcript`) and the live store mirror
//! ([`super::live::write_live_turn`]) — with the *same* completed turn, then
//! asserts the store journal renders byte-for-byte the same
//! [`JournalMessage`]s the importer's parity helper reads back off the legacy
//! JSONL. This proves the two writers stay shape-identical for new turns
//! without depending on the read path (04.2).

use std::path::Path;

use tempfile::TempDir;
use tinyagents::harness::store::{AppendStore, FileStore, JsonlAppendStore, Store};

use super::convert::{sanitize_store_name, stream_name};
use super::live::{dual_write_enabled, write_live_turn};
use super::ops::store_root;
use super::types::{JournalMessage, SessionDescriptor, NS_SESSIONS};
use crate::openhuman::agent::harness::session::transcript::{
    attach_turn_usage_metadata, read_transcript, write_transcript, MessageUsage, SessionTranscript,
    TranscriptMeta, TurnUsage,
};
use crate::openhuman::inference::provider::{ChatMessage, ToolCall};

/// A transcript meta header matching the importer's `native` fixture shape.
fn meta(thread_id: &str) -> TranscriptMeta {
    TranscriptMeta {
        agent_name: "orchestrator".to_string(),
        agent_id: Some("orchestrator".to_string()),
        agent_type: Some("root".to_string()),
        dispatcher: "native".to_string(),
        provider: Some("anthropic".to_string()),
        model: Some("claude".to_string()),
        created: "2024-01-01T00:00:00Z".to_string(),
        updated: "2024-01-01T00:05:00Z".to_string(),
        turn_count: 1,
        input_tokens: 100,
        output_tokens: 50,
        cached_input_tokens: 20,
        charged_amount_usd: 0.05,
        thread_id: Some(thread_id.to_string()),
        task_id: None,
    }
}

/// Per-turn usage carrying a native tool call, so tool-call ids are exercised
/// on the parity path (acceptance: "tool-call ids").
fn turn_usage() -> TurnUsage {
    TurnUsage {
        provider: "anthropic".to_string(),
        model: "claude".to_string(),
        usage: MessageUsage {
            input: 100,
            output: 50,
            cached_input: 20,
            context_window: 200_000,
            cost_usd: 0.05,
        },
        ts: "2024-01-01T00:00:01Z".to_string(),
        reasoning_content: None,
        tool_calls: vec![ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: "{\"path\":\"x\"}".to_string(),
            extra_content: None,
        }],
        iteration: 1,
    }
}

/// Read the store journal stream back into `JournalMessage`s, mirroring the
/// importer's `journal_readback` helper.
async fn journal_readback(ws: &Path, stream: &str) -> Vec<JournalMessage> {
    let journal = JsonlAppendStore::new(store_root(ws).join("journal"));
    journal
        .read_from(stream, 0)
        .await
        .expect("journal read")
        .into_iter()
        .map(|(_, v)| serde_json::from_value(v).expect("journal record shape"))
        .collect()
}

#[tokio::test]
async fn live_dual_write_matches_legacy_jsonl_render() {
    let ws = TempDir::new().expect("tempdir");
    let stem = "1719_orchestrator";
    let jsonl_path = ws.path().join("session_raw").join(format!("{stem}.jsonl"));

    // A user turn + an assistant turn. The base messages carry no usage
    // metadata: the legacy writer embeds it from its `turn_usage` argument,
    // exactly as `persist_session_transcript` does in production.
    let base_messages = vec![ChatMessage::user("hi"), ChatMessage::assistant("done")];
    let meta = meta("t-root");
    let usage = turn_usage();

    // (1) Legacy authoritative write — the primary persistence path.
    write_transcript(&jsonl_path, &base_messages, &meta, Some(&usage)).expect("legacy write");

    // (2) Live dual-write — replicate `session_io`'s construction: attach the
    // turn usage to the last assistant message, then mirror into the store.
    let mut live_messages = base_messages.clone();
    let last_assistant = live_messages
        .iter()
        .rposition(|m| m.role == "assistant")
        .expect("assistant message present");
    attach_turn_usage_metadata(&mut live_messages[last_assistant], &usage);
    let transcript = SessionTranscript {
        meta: meta.clone(),
        messages: live_messages,
    };
    write_live_turn(ws.path(), stem, &transcript)
        .await
        .expect("live dual-write");

    // Parity: the store journal must equal the importer's read-back of the
    // legacy JSONL, field for field (including reconstructed
    // `openhuman_turn_usage` metadata and the tool-call id).
    let expected: Vec<JournalMessage> = read_transcript(&jsonl_path)
        .expect("read legacy transcript")
        .messages
        .iter()
        .map(JournalMessage::from)
        .collect();
    let actual = journal_readback(ws.path(), &stream_name(stem)).await;
    assert_eq!(
        actual, expected,
        "live store stream diverges from the legacy JSONL render"
    );

    // The assistant record must carry the tool-call id via reconstructed usage.
    let assistant = actual
        .iter()
        .find(|m| m.role == "assistant")
        .expect("assistant record");
    let tool_id = assistant
        .extra_metadata
        .as_ref()
        .and_then(|m| m.get("openhuman_turn_usage"))
        .and_then(|u| u.get("tool_calls"))
        .and_then(|t| t.get(0))
        .and_then(|c| c.get("id"))
        .and_then(|id| id.as_str());
    assert_eq!(tool_id, Some("tc1"), "tool-call id lost on the store path");

    // The session descriptor is upserted under the sanitized stem with the
    // stem's thread id and journal stream, matching the importer's projection.
    let kv = FileStore::new(store_root(ws.path()).join("kv"));
    let desc_value = kv
        .get(NS_SESSIONS, &sanitize_store_name(stem))
        .await
        .expect("kv get")
        .expect("descriptor present after live write");
    let desc: SessionDescriptor = serde_json::from_value(desc_value).expect("descriptor shape");
    assert_eq!(desc.session_key, stem);
    assert_eq!(desc.thread_id, "t-root");
    assert!(!desc.thread_id_synthesized);
    assert_eq!(desc.stream, stream_name(stem));
    assert_eq!(desc.dispatcher, "native");
    assert_eq!(desc.provider.as_deref(), Some("anthropic"));
    assert_eq!(desc.model.as_deref(), Some("claude"));
}

/// The dual-write is driven by the `AgentConfig::session_dual_write` config
/// flag (default ON) with the `OPENHUMAN_SESSION_DUAL_WRITE` env var as a pure
/// kill switch. This exercises the decision matrix directly. Env mutation is
/// process-global, so all assertions live in one serial test and the var is
/// restored on exit; no other test reads this var.
#[test]
fn config_flag_and_env_kill_switch() {
    const ENV: &str = "OPENHUMAN_SESSION_DUAL_WRITE";
    let prior = std::env::var(ENV).ok();

    // Config OFF disables regardless of env.
    std::env::remove_var(ENV);
    assert!(!dual_write_enabled(false), "config off disables");

    // Config ON (the default) enables when the env is unset.
    assert!(dual_write_enabled(true), "config on + no env enables");

    // A falsey env value is the kill switch: forces OFF even with config ON.
    for killed in ["0", "false", "no", "off", "disable", "disabled", "OFF"] {
        std::env::set_var(ENV, killed);
        assert!(
            !dual_write_enabled(true),
            "kill switch value {killed:?} must force off"
        );
    }

    // A non-falsey env value does not force on: config still governs.
    std::env::set_var(ENV, "1");
    assert!(
        dual_write_enabled(true),
        "non-falsey env leaves config ON on"
    );
    assert!(
        !dual_write_enabled(false),
        "non-falsey env does not force config-off on"
    );

    match prior {
        Some(v) => std::env::set_var(ENV, v),
        None => std::env::remove_var(ENV),
    }
}
