//! Regression coverage for `agent_id`-scoped transcript resume.
//!
//! Split from `session_thread_resume_tests.rs` to keep that file under the
//! repo's 750-line layout cap (rust:layout / check-openhuman-rust-layout.mjs).
//! Shares its parent's `use super::*` fixtures via the same `#[path]` mount.

use super::*;

/// Regression: a library host can give two distinct runtime agents the same
/// caller-supplied `thread_id` — nothing in the id namespace prevents it.
/// Without `agent_id` scoping, resume would resolve to whichever agent's
/// transcript for that thread is newest, splicing agent A's history into
/// agent B's turn. `seed_resume_from_thread_transcript_scoped` must instead
/// resume only the transcript whose `_meta.agent_id` matches the caller.
#[test]
fn seed_resume_from_thread_transcript_scoped_does_not_leak_across_agents() {
    use super::super::transcript::{self, TranscriptMeta};
    use crate::agent::messages::ChatMessage;

    let ws = tempfile::TempDir::new().expect("temp workspace");
    let wsp = ws.path().to_path_buf();
    // Two independently configured runtime agents reuse the same thread id —
    // e.g. a host that mints thread ids per conversation turn rather than per
    // agent.
    let thread_id = "thr_shared_across_agents";

    let meta = |agent_id: &str, stamp: &str| TranscriptMeta {
        agent_name: format!("{agent_id}_thr_shared"),
        agent_id: Some(agent_id.to_string()),
        agent_type: Some("root".to_string()),
        dispatcher: "native".to_string(),
        provider: None,
        model: None,
        created: stamp.to_string(),
        updated: stamp.to_string(),
        turn_count: 1,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
        thread_id: Some(thread_id.to_string()),
        task_id: None,
    };

    // Agent A's transcript — written LATER (newer `created`), so an unscoped
    // newest-wins lookup would pick this one for agent B too.
    let a_messages = vec![
        ChatMessage::system("system prompt"),
        ChatMessage::user("agent A's private question"),
        ChatMessage::assistant("agent A's private answer"),
    ];
    let a_path = wsp.join("session_raw").join("1700009999_agent_a.jsonl");
    std::fs::create_dir_all(a_path.parent().unwrap()).unwrap();
    transcript::write_transcript(
        &a_path,
        &a_messages,
        &meta("agent_a", "2026-02-02T00:00:00Z"),
        None,
    )
    .expect("write agent A transcript");

    // Agent B's own transcript — older, but the one agent B must resume.
    let b_messages = vec![
        ChatMessage::system("system prompt"),
        ChatMessage::user("agent B's own question"),
        ChatMessage::assistant("agent B's own answer"),
    ];
    let b_path = wsp.join("session_raw").join("1700000000_agent_b.jsonl");
    std::fs::create_dir_all(b_path.parent().unwrap()).unwrap();
    transcript::write_transcript(
        &b_path,
        &b_messages,
        &meta("agent_b", "2026-01-01T00:00:00Z"),
        None,
    )
    .expect("write agent B transcript");

    let mut agent_b = build_minimal_agent_with_definition_name(Some("agent_b"));
    agent_b.workspace_dir = wsp.clone();
    agent_b.session_raw_subdir = "session_raw".to_string();

    assert!(
        agent_b.seed_resume_from_thread_transcript_scoped(thread_id, Some("agent_b")),
        "agent B must resume its own transcript for the shared thread id"
    );
    let cached = agent_b
        .cached_transcript_messages
        .as_ref()
        .expect("cached transcript populated");
    assert!(
        cached.iter().any(|m| m.content.contains("agent B's own")),
        "agent B's resume must load its own transcript"
    );
    assert!(
        !cached
            .iter()
            .any(|m| m.content.contains("agent A's private")),
        "agent B's resume must never load agent A's transcript for the same thread id, \
         got {cached:?}"
    );
}
