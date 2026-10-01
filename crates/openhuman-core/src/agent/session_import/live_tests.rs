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
use tinyagents_harness::store::{AppendStore, JsonlAppendStore};

use super::live::{dual_write_enabled, shadow_reads_enabled};
use super::projector::journal_message_from_transcript as project;
use crate::config::test_env::EnvVarGuard;
use tinyagents_session::transcript::import::convert::journal_messages as journal_messages_with;
use tinyagents_session::transcript::import::live::{
    shadow_read_compare as shadow_read_compare_with, write_live_turn as write_live_turn_with,
    ShadowReadOutcome,
};
use tinyagents_session::transcript::import::ops::store_root;
use tinyagents_session::transcript::import::types::JournalMessage;
use tinyagents_session::transcript::TranscriptMessage;
use tinyagents_session::transcript::{
    read_transcript, write_transcript, MessageUsage, SessionTranscript, TranscriptMeta,
    TranscriptToolCall, TurnUsage,
};

fn journal_messages(t: &SessionTranscript) -> Vec<JournalMessage> {
    journal_messages_with(t, project)
}

async fn write_live_turn(workspace: &Path, key: &str, t: &SessionTranscript) -> anyhow::Result<()> {
    write_live_turn_with(workspace, key, t, project).await
}

async fn shadow_read_compare(
    workspace: &Path,
    key: &str,
    t: &SessionTranscript,
) -> ShadowReadOutcome {
    shadow_read_compare_with(workspace, key, t, project).await
}

fn durable_messages(messages: &[TranscriptMessage]) -> Vec<TranscriptMessage> {
    messages.to_vec()
}

/// A transcript meta header matching the importer's `native` fixture shape.
fn meta(thread_id: &str) -> TranscriptMeta {
    TranscriptMeta {
        session_id: None,
        parent_session_id: None,
        agent_name: "orchestrator".to_string(),
        agent_id: Some("orchestrator".to_string()),
        agent_type: Some("root".to_string()),
        dispatcher: "native".to_string(),
        provider: Some("anthropic".to_string()),
        model: Some("claude".to_string()),
        created: "2024-01-01T00:00:00Z".to_string(),
        updated: "2024-01-01T00:05:00Z".to_string(),
        turn_count: 1,
        prefix_message_count: None,
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
        tool_calls: vec![TranscriptToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: "{\"path\":\"x\"}".to_string(),
            extra_content: None,
        }],
        iteration: 1,
    }
}

/// A multi-turn session whose messages carry the sidecar `extra_metadata` the
/// single-turn happy-path fixture cannot express: a system message, a failed
/// `tool` message tagged with `openhuman_tool_failure`, two assistant turns, and
/// the trailing assistant that this turn's usage attaches to. The writer lifts
/// the tool-failure marker onto the line's top-level `failure`/`failure_detail`
/// fields, and since #6282 the read-back restores it to `extra_metadata`, so the
/// marker round-trips.
fn rich_base_messages() -> Vec<TranscriptMessage> {
    let mut failed_tool = TranscriptMessage::tool("read_file failed: boom");
    failed_tool.tool_failure = Some(tinyagents_session::transcript::ToolFailure {
        failed: true,
        detail: Some("boom".into()),
    });
    vec![
        TranscriptMessage::system("you are the orchestrator"),
        TranscriptMessage::user("read the file"),
        TranscriptMessage::assistant("calling read_file"),
        failed_tool,
        TranscriptMessage::assistant("done"),
    ]
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

/// The dual-write is driven by the `AgentConfig::session_dual_write` config
/// flag (default ON) with the `OPENHUMAN_SESSION_DUAL_WRITE` env var as a pure
/// kill switch. This exercises the decision matrix directly. Env mutation is
/// process-global, so all assertions live in one serial test and the var is
/// restored on exit; no other test reads this var.
#[test]
fn config_flag_and_env_kill_switch() {
    const ENV: &str = "OPENHUMAN_SESSION_DUAL_WRITE";
    // Crate-wide env lock, held to the end; restores the prior value on drop
    // (also on unwind).
    let _env = EnvVarGuard::locked_unset(ENV);

    // Config OFF disables regardless of env.
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
}

// ── Store-backed shadow read (issue #4249, 04.2 phase 2) ────────────────────

/// A session written by the dual-write and then read back through the shadow
/// reader must render byte-for-byte the messages the legacy JSONL reader
/// produces — no divergence. This is the read-path twin of
/// [`live_dual_write_matches_legacy_jsonl_render`]: it drives the *same*
/// writers, then compares via the actual `shadow_read_compare` path and asserts
/// a clean [`ShadowReadOutcome::Match`].
#[tokio::test]
async fn shadow_read_roundtrip_matches_legacy() {
    let ws = TempDir::new().expect("tempdir");
    let stem = "1719_orchestrator";
    let jsonl_path = ws.path().join("session_raw").join(format!("{stem}.jsonl"));

    // A rich session (system + tool-failure + two assistant turns) so the fixture
    // can actually express the sidecar-metadata asymmetries; a two-message
    // happy-path turn cannot, which is why the pre-#6149 version passed vacuously.
    let base_messages = rich_base_messages();
    let meta = meta("t-root");
    let usage = turn_usage();

    // (1) Legacy authoritative write.
    write_transcript(
        &jsonl_path,
        &durable_messages(&base_messages),
        &meta,
        Some(&usage),
    )
    .expect("legacy write");

    // (2) Live dual-write, mirrored exactly as the fixed
    // `maybe_dual_write_session_store` does (#6149): the store record IS
    // `read_transcript(path)` — the legacy JSONL after its write→read round-trip,
    // the same value the shadow reader compares against — not a hand-rebuilt copy
    // of the in-memory turn.
    let mirrored = read_transcript(&jsonl_path).expect("read legacy transcript for mirror");
    write_live_turn(ws.path(), stem, &mirrored)
        .await
        .expect("live dual-write");

    // The legacy reader materializes this SessionTranscript for a resume; the
    // shadow reader compares it against the store stream.
    let legacy = read_transcript(&jsonl_path).expect("read legacy transcript");
    let outcome = shadow_read_compare(ws.path(), stem, &legacy).await;
    assert_eq!(
        outcome,
        ShadowReadOutcome::Match {
            messages: legacy.messages.len()
        },
        "round-tripped shadow read must match the legacy render with no divergence"
    );
}

/// An in-memory reconstruction remains observably distinct from the durable
/// JSONL read-back even when replay metadata is not materialized explicitly.
#[tokio::test]
async fn in_memory_store_reconstruction_diverges_from_legacy_on_sidecar_metadata() {
    let ws = TempDir::new().expect("tempdir");
    let stem = "1719_orchestrator";
    let jsonl_path = ws.path().join("session_raw").join(format!("{stem}.jsonl"));

    let base_messages = rich_base_messages();
    let meta = meta("t-root");
    let usage = turn_usage();

    // Legacy write the way `persist_session_transcript` does it: append-only,
    // stamped with the turn's request id.
    tinyagents_session::transcript::append_transcript_turn(
        &jsonl_path,
        &[],
        &durable_messages(&base_messages),
        &meta,
        Some(&usage),
        Some("req-1"),
    )
    .expect("legacy write");

    // Pre-fix store construction: clone the in-memory turn, attach usage to the
    // last assistant, and mirror THAT — never round-tripping through the JSONL,
    // so the tool-failure marker the round-trip strips is still present.
    let mut live_messages = base_messages.clone();
    let last_assistant = live_messages
        .iter()
        .rposition(|m| m.role == "assistant")
        .expect("assistant message present");
    live_messages[last_assistant].turn_usage = Some(usage.clone());
    let reconstructed = SessionTranscript {
        tools: None,
        meta: meta.clone(),
        messages: durable_messages(&live_messages),
    };
    write_live_turn(ws.path(), stem, &reconstructed)
        .await
        .expect("live dual-write");

    let legacy = read_transcript(&jsonl_path).expect("read legacy transcript");
    let outcome = shadow_read_compare(ws.path(), stem, &legacy).await;

    let rendered = base_messages.len();
    assert_eq!(
        outcome,
        ShadowReadOutcome::Divergence {
            legacy: rendered,
            shadow: rendered,
            first_diff: Some(0),
        },
        "the in-memory reconstruction must diverge from the durable read-back at index zero"
    );
}

/// The shadow read is driven by the `AgentConfig::session_shadow_reads` config
/// flag (default **ON** since the Phase 2 parity soak) with the
/// `OPENHUMAN_SESSION_SHADOW_READS` env var as a
/// pure kill switch (can only force OFF, never ON). This exercises the decision
/// matrix directly — the gate `maybe_shadow_read_session_store` early-returns
/// (never invoking the reader) whenever this returns `false`. Env mutation is
/// process-global, so all assertions live in one serial test and the var is
/// restored on exit.
#[test]
fn shadow_read_flag_and_env_kill_switch() {
    const ENV: &str = "OPENHUMAN_SESSION_SHADOW_READS";
    // Crate-wide env lock, held to the end; restores the prior value on drop
    // (also on unwind).
    let _env = EnvVarGuard::locked_unset(ENV);

    // Config OFF (the default) disables regardless of env — reader not invoked.
    assert!(
        !shadow_reads_enabled(false),
        "config off (default) disables the shadow read"
    );

    // Config ON enables when the env is unset.
    assert!(
        shadow_reads_enabled(true),
        "config on + no env enables the shadow read"
    );

    // A falsey env value is the kill switch: forces OFF even with config ON.
    for killed in ["0", "false", "no", "off", "disable", "disabled", "OFF"] {
        std::env::set_var(ENV, killed);
        assert!(
            !shadow_reads_enabled(true),
            "kill switch value {killed:?} must force off even with flag on"
        );
    }

    // A non-falsey env value does not force on: config still governs, and it
    // can never turn a default-off flag on.
    std::env::set_var(ENV, "1");
    assert!(
        shadow_reads_enabled(true),
        "non-falsey env leaves config ON on"
    );
    assert!(
        !shadow_reads_enabled(false),
        "non-falsey env cannot force a default-off flag on"
    );
}

// ── Legacy on-disk shapes (plan-agents.md Phase 2) ────────────────────────────
//
// Phase 2's exit criteria name two legacy layouts that must survive the
// migration: the date-grouped `session_raw/DDMMYYYY/` directory and the
// markdown transcripts `read_transcript_legacy_md` still parses. Both predate
// the store, so both reach the shadow read by a different route than the happy
// path above — and a real user upgrading has them on disk today.
