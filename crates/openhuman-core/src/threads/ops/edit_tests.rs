//! Transcript-level tests for `threads::ops::edit`'s private truncation
//! helpers (`truncate_transcript_before_turn`, `truncate_transcript_for_regenerate`).
//!
//! Every fixture is written with the session crate's own writer
//! (`append_transcript_turn`), never hand-rolled JSONL, so a wire-format
//! change upstream surfaces here as a failure instead of silently diverging
//! from what the core actually persists. See `edit_turn_state_tests.rs` for
//! `clear_dropped_turn_states` / `next_reply_request_id_after`, which need
//! different fixtures (turn-state snapshots, the message-log store) and are
//! split out to keep both files well under the layout line limit.

use super::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tinyagents_session::transcript::{
    append_transcript_turn, resolve_keyed_transcript_path, session_stem, MessageUsage, TurnUsage,
};

const AGENT_ID: &str = "test-agent";

fn base_meta(agent_id: &str, thread_id: &str) -> TranscriptMeta {
    TranscriptMeta {
        agent_name: agent_id.to_string(),
        agent_id: Some(agent_id.to_string()),
        agent_type: Some("test".into()),
        dispatcher: "native".into(),
        provider: Some("openhuman".into()),
        model: Some("test-model".into()),
        created: "2026-09-24T00:00:00Z".into(),
        updated: "2026-09-24T00:00:00Z".into(),
        turn_count: 0,
        prefix_message_count: None,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
        thread_id: Some(thread_id.to_string()),
        task_id: None,
        session_id: None,
        parent_session_id: None,
    }
}

fn turn_usage() -> TurnUsage {
    TurnUsage {
        provider: "openhuman".into(),
        model: "test-model".into(),
        usage: MessageUsage {
            input: 10,
            output: 5,
            cached_input: 0,
            context_window: 1_000_000,
            cost_usd: 0.0,
        },
        ts: "2026-09-24T00:00:01Z".into(),
        reasoning_content: None,
        tool_calls: Vec::new(),
        iteration: 1,
    }
}

/// Write a two-turn root transcript for `thread_id`/`AGENT_ID` using the same
/// writer the runtime uses (`append_transcript_turn`), and return the file
/// path plus the two turns' `request_id`s.
fn write_two_turn_transcript(workspace: &Path, thread_id: &str) -> (PathBuf, String, String) {
    let session = SessionRef::scoped(thread_id, AGENT_ID);
    let path = resolve_keyed_transcript_path(workspace, &session_stem(&session))
        .expect("resolve fixture transcript path");
    let req1 = "turn-1".to_string();
    let req2 = "turn-2".to_string();
    let meta = base_meta(AGENT_ID, thread_id);

    let turn1 = vec![
        TranscriptMessage::new("user", "user prompt 1"),
        TranscriptMessage::assistant("answer 1"),
    ];
    let mut turn1_meta = meta.clone();
    turn1_meta.turn_count = 1;
    append_transcript_turn(
        &path,
        &[],
        &turn1,
        &turn1_meta,
        Some(&turn_usage()),
        Some(&req1),
    )
    .expect("append turn 1");

    let mut turn2 = turn1.clone();
    turn2.push(TranscriptMessage::new("user", "user prompt 2"));
    turn2.push(TranscriptMessage::assistant("answer 2"));
    let mut turn2_meta = meta.clone();
    turn2_meta.turn_count = 2;
    append_transcript_turn(
        &path,
        &turn1,
        &turn2,
        &turn2_meta,
        Some(&turn_usage()),
        Some(&req2),
    )
    .expect("append turn 2");

    (path, req1, req2)
}

/// A root transcript with a single system row and no user/assistant turn at
/// all — feeds the "nothing to regenerate" `Ok(None)` case.
fn write_turnless_transcript(workspace: &Path, thread_id: &str) -> PathBuf {
    let session = SessionRef::scoped(thread_id, AGENT_ID);
    let path = resolve_keyed_transcript_path(workspace, &session_stem(&session))
        .expect("resolve fixture transcript path");
    let meta = base_meta(AGENT_ID, thread_id);
    let messages = vec![TranscriptMessage::new("system", "boot preamble")];
    append_transcript_turn(&path, &[], &messages, &meta, None, None).expect("append system row");
    path
}

#[test]
fn truncate_transcript_before_turn_cuts_head_and_seals_original_untouched() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = "thread-before-turn";
    let (path, req1, req2) = write_two_turn_transcript(dir.path(), thread_id);

    let original_bytes = std::fs::read(&path).expect("read original transcript");

    truncate_transcript_before_turn(dir.path(), thread_id, &req2).expect("truncate");

    // Sealed generation preserved byte-for-byte: "a compaction never
    // erases" applies identically to this edit-driven fork.
    let bytes_after = std::fs::read(&path).expect("re-read original transcript");
    assert_eq!(
        original_bytes, bytes_after,
        "sealed generation must stay byte-for-byte untouched"
    );

    // Head truncated: the new head ends right before turn 2's first row.
    let (_, _, head) = resolve_head_transcript(dir.path(), thread_id).expect("resolve head");
    assert!(
        head.messages
            .iter()
            .all(|m| m.request_id.as_deref() != Some(req2.as_str())),
        "turn 2 must be entirely dropped from the head: {:?}",
        head.messages
    );
    let last = head.messages.last().expect("head keeps turn 1");
    assert_eq!(last.request_id.as_deref(), Some(req1.as_str()));
    assert_eq!(last.content, "answer 1", "turn 1's answer is kept intact");
}

#[test]
fn truncate_transcript_before_turn_errs_when_turn_not_found() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = "thread-missing-turn";
    write_two_turn_transcript(dir.path(), thread_id);

    let err = truncate_transcript_before_turn(dir.path(), thread_id, "no-such-turn")
        .expect_err("unknown request_id must fail");
    assert!(
        err.contains("no-such-turn"),
        "error should name the missing turn: {err}"
    );
}

#[test]
fn truncate_transcript_for_regenerate_targets_specific_turn() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = "thread-regen-target";
    let (path, req1, req2) = write_two_turn_transcript(dir.path(), thread_id);
    let original_bytes = std::fs::read(&path).expect("read original transcript");

    let (prompt, request_id) =
        truncate_transcript_for_regenerate(dir.path(), thread_id, Some(&req2))
            .expect("truncate")
            .expect("a turn to regenerate");
    assert_eq!(prompt, "user prompt 2");
    assert_eq!(request_id, req2);

    let bytes_after = std::fs::read(&path).expect("re-read original transcript");
    assert_eq!(
        original_bytes, bytes_after,
        "sealed generation must stay byte-for-byte untouched"
    );

    let (_, _, head) = resolve_head_transcript(dir.path(), thread_id).expect("resolve head");
    let last = head.messages.last().expect("head keeps turn 2's prompt");
    assert_eq!(last.role, "user");
    assert_eq!(last.content, "user prompt 2");
    assert!(
        head.messages.iter().all(|m| m.content != "answer 2"),
        "turn 2's answer must be dropped: {:?}",
        head.messages
    );
    let _ = req1;
}

#[test]
fn truncate_transcript_for_regenerate_none_resolves_last_turn() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = "thread-regen-last";
    let (_, _req1, req2) = write_two_turn_transcript(dir.path(), thread_id);

    let (prompt, request_id) = truncate_transcript_for_regenerate(dir.path(), thread_id, None)
        .expect("truncate")
        .expect("a turn to regenerate");

    assert_eq!(prompt, "user prompt 2", "last turn's prompt is resent");
    assert_eq!(request_id, req2, "last turn's request_id is resolved");
}

#[test]
fn truncate_transcript_for_regenerate_returns_none_when_no_turn_exists() {
    let dir = TempDir::new().expect("tempdir");
    let thread_id = "thread-no-turns";
    write_turnless_transcript(dir.path(), thread_id);

    let result = truncate_transcript_for_regenerate(dir.path(), thread_id, None).expect("truncate");
    assert_eq!(result, None, "no user/assistant turn to regenerate");
}

/// Sentry TAURI-REACT-AK: regenerating on a thread that never wrote a session
/// transcript surfaced "thread <id> has no session transcript" instead of the
/// friendly "has no turn to regenerate" the caller maps `Ok(None)` to.
#[test]
fn truncate_transcript_for_regenerate_returns_none_when_thread_has_no_transcript() {
    let dir = TempDir::new().expect("tempdir");

    let result = truncate_transcript_for_regenerate(dir.path(), "thread-without-transcript", None)
        .expect("a thread with no transcript has no turn to regenerate, not an error");
    assert_eq!(result, None);
}
