//! The per-thread session checkout every turn on a thread goes through — user
//! turns and host-authored turns alike (`ops/system_turn.rs`).
//!
//! Regression context: background delivery used to run on a throwaway host
//! bound to the thread, writing a competing root transcript that the next
//! cold-boot resume preferred over the real conversation. Routing it through
//! this checkout is what keeps one live history per thread.

use std::path::Path;

use tinyagents_session::transcript::{write_transcript, TranscriptMeta};

use super::{
    checkin_session_agent, checkin_session_agent_if_vacant, checkout_session_agent,
    fingerprint_diff, CheckedOutSession, CheckoutPolicy,
};
use crate::agent::messages::{ChatMessage, ConversationMessage};
use crate::agent::OpenHumanSessionHost;
use crate::config::Config;
use crate::web_chat::ops::{key_for, THREAD_SESSIONS};
use crate::web_chat::types::SessionCacheFingerprint;

fn test_config(tmp: &tempfile::TempDir) -> Config {
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    config
}

fn unique_thread(tag: &str) -> String {
    format!("thread-checkout-{tag}-{}", uuid::Uuid::new_v4())
}

/// A root transcript for `thread_id` with the given prose rows, as the
/// session persistence writes one.
fn write_thread_transcript(workspace_dir: &Path, stem: &str, thread_id: &str, rows: &[&str]) {
    let path = workspace_dir
        .join("session_raw")
        .join(format!("{stem}.jsonl"));
    let messages: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if index % 2 == 0 {
                ChatMessage::user(*text)
            } else {
                ChatMessage::assistant(*text)
            }
        })
        .map(|message| crate::agent::messages::transcript_message_from_chat(&message))
        .collect();
    let meta = TranscriptMeta {
        session_id: None,
        parent_session_id: None,
        agent_name: "orchestrator_thread".into(),
        agent_id: Some("orchestrator".into()),
        agent_type: Some("root".into()),
        dispatcher: "native".into(),
        provider: None,
        model: None,
        created: "2026-09-20T15:33:42Z".into(),
        updated: "2026-09-20T15:36:32Z".into(),
        turn_count: rows.len() / 2,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
        thread_id: Some(thread_id.to_string()),
        task_id: None,
    };
    write_transcript(&path, &messages, &meta, None).unwrap();
}

fn prose(history: &[ConversationMessage]) -> Vec<String> {
    history
        .iter()
        .filter_map(|message| match message {
            ConversationMessage::Chat(chat) => Some(chat.content.clone()),
            _ => None,
        })
        .collect()
}

fn host_seeded_with(config: &Config, marker: &str) -> OpenHumanSessionHost {
    let mut host = OpenHumanSessionHost::from_config_for_agent(config, "orchestrator").unwrap();
    host.seed_resume_from_messages(
        vec![
            ("user".to_string(), marker.to_string()),
            ("agent".to_string(), "ok".to_string()),
        ],
        "",
    )
    .unwrap();
    host
}

async fn evict(thread_id: &str) {
    THREAD_SESSIONS.lock().await.remove(&key_for(thread_id));
}

#[tokio::test]
async fn checkout_cold_boots_from_the_thread_transcript_and_checkin_keeps_it_warm() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let thread_id = unique_thread("cold");
    write_thread_transcript(
        &config.workspace_dir,
        "1789918422_orchestrator_thread",
        &thread_id,
        &[
            "help me plan a trip to north india",
            "sure — how long, and who is going?",
        ],
    );

    // A host-authored turn checks out with no overrides and no user text.
    let CheckedOutSession {
        mut agent,
        fingerprint,
    } = checkout_session_agent(
        &config,
        super::super::SYSTEM_CLIENT_ID,
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::AdoptCached,
    )
    .await
    .unwrap();
    // Checkout binds the thread's durable session identity; the history loads
    // when the session resumes, which every turn does for itself. The
    // conversation the transcript holds must come back either way.
    assert!(
        agent.resume_bound_session().await.unwrap(),
        "a thread with a transcript must resume"
    );
    let history = prose(&agent.history());
    assert!(
        history
            .iter()
            .any(|row| row.contains("plan a trip to north india")),
        "cold checkout must resume the thread's transcript, got {history:?}"
    );

    checkin_session_agent(&thread_id, agent, fingerprint).await;
    assert!(THREAD_SESSIONS
        .lock()
        .await
        .contains_key(&key_for(&thread_id)));

    // The next checkout — a user turn — reuses the warm agent with that history.
    let CheckedOutSession { agent, .. } = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();
    assert!(
        prose(&agent.history())
            .iter()
            .any(|row| row.contains("plan a trip to north india")),
        "warm checkout must carry the same history"
    );
    // Checked out means removed: nobody else can drive this agent meanwhile.
    assert!(!THREAD_SESSIONS
        .lock()
        .await
        .contains_key(&key_for(&thread_id)));
    evict(&thread_id).await;
}

#[tokio::test]
async fn checkin_if_vacant_yields_to_a_turn_that_re_cached_meanwhile() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let thread_id = unique_thread("vacant");
    let fingerprint =
        |c: &Config| super::build_session_fingerprint(c, None, None, "orchestrator".into(), "chat");

    // A user turn finished while the system turn was running and cached its
    // agent unconditionally.
    let user_turn_agent = host_seeded_with(&config, "user-turn-history");
    checkin_session_agent(&thread_id, user_turn_agent, fingerprint(&config)).await;

    // The system turn must not clobber it.
    let system_turn_agent = host_seeded_with(&config, "system-turn-history");
    assert!(
        !checkin_session_agent_if_vacant(&thread_id, system_turn_agent, fingerprint(&config)).await
    );
    let CheckedOutSession { agent, .. } = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();
    assert_eq!(prose(&agent.history()), vec!["user-turn-history", "ok"]);

    // Into a vacant slot it goes in.
    let system_turn_agent = host_seeded_with(&config, "system-turn-history");
    assert!(
        checkin_session_agent_if_vacant(&thread_id, system_turn_agent, fingerprint(&config)).await
    );
    let CheckedOutSession { agent, .. } = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();
    assert_eq!(prose(&agent.history()), vec!["system-turn-history", "ok"]);
    evict(&thread_id).await;
}

#[tokio::test]
async fn a_fork_never_takes_or_returns_the_cached_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let thread_id = unique_thread("fork");
    let fingerprint =
        super::build_session_fingerprint(&config, None, None, "orchestrator".into(), "chat");
    checkin_session_agent(
        &thread_id,
        host_seeded_with(&config, "primary-history"),
        fingerprint,
    )
    .await;

    let CheckedOutSession { agent, .. } = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Fork,
    )
    .await
    .unwrap();
    // Built fresh: no transcript on disk for this thread, so an empty history.
    assert!(prose(&agent.history()).is_empty());
    // The primary's cached agent was left in place.
    assert!(THREAD_SESSIONS
        .lock()
        .await
        .contains_key(&key_for(&thread_id)));
    evict(&thread_id).await;
}

#[tokio::test]
async fn a_system_turn_adopts_the_cached_agent_and_its_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let thread_id = unique_thread("adopt");
    // The user's last turn pinned a temperature; a system turn has none and
    // would miss an exact fingerprint match.
    let pinned =
        super::build_session_fingerprint(&config, None, Some(0.2), "orchestrator".into(), "chat");
    checkin_session_agent(
        &thread_id,
        host_seeded_with(&config, "pinned-history"),
        pinned.clone(),
    )
    .await;

    let CheckedOutSession { agent, fingerprint } = checkout_session_agent(
        &config,
        super::super::SYSTEM_CLIENT_ID,
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::AdoptCached,
    )
    .await
    .unwrap();
    assert_eq!(prose(&agent.history()), vec!["pinned-history", "ok"]);
    assert_eq!(fingerprint, pinned, "handed back under the user's settings");

    // An exact user checkout with the same settings then still hits warm.
    checkin_session_agent_if_vacant(&thread_id, agent, fingerprint).await;
    let CheckedOutSession { agent, .. } = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        Some(0.2),
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();
    assert_eq!(prose(&agent.history()), vec!["pinned-history", "ok"]);
    evict(&thread_id).await;
}

/// The identity that fixes the reported bug: a thread resolves to one
/// transcript, named without a timestamp, so two cold boots address the same
/// file instead of accumulating one root per launch.
#[tokio::test]
async fn a_thread_binds_one_stable_session_across_cold_boots() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let thread_id = unique_thread("stable");

    let first = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();
    // Nothing is checked back in, so the next checkout is a genuine cold boot.
    let second = checkout_session_agent(
        &config,
        "client-1",
        &thread_id,
        None,
        None,
        None,
        CheckoutPolicy::Exact,
    )
    .await
    .unwrap();

    let session_id = first
        .agent
        .session_id()
        .expect("a chat thread is a session");
    assert_eq!(
        second.agent.session_id().as_deref(),
        Some(session_id.as_str()),
        "two cold boots of one thread must address the same session"
    );
    assert!(
        session_id.starts_with(&thread_id),
        "the session is named for its conversation, got {session_id}"
    );
    assert!(
        !session_id
            .split(['.', '_'])
            .any(|part| part.len() >= 10 && part.chars().all(|c| c.is_ascii_digit())),
        "a timestamp in the name is what made every launch a new transcript: {session_id}"
    );
    evict(&thread_id).await;
}

/// A fingerprint with every field set, so a test can change exactly one and
/// know the diff it expects is the only one available.
fn sample_fingerprint() -> SessionCacheFingerprint {
    SessionCacheFingerprint {
        model_override: Some("hint:chat".to_string()),
        temperature: Some(0.7),
        target_agent_id: "orchestrator".to_string(),
        provider_binding: "openhuman".to_string(),
        autonomy_signature: r#"{"approval_required":true,"action_dir":"/home/u/w"}"#.to_string(),
        model_registry_signature: r#"[{"id":"m","provider":"p","vision":false}]"#.to_string(),
    }
}

#[test]
fn fingerprint_diff_is_empty_for_equal_fingerprints() {
    // Guards the miss log's own "no field differs" branch: if this ever became
    // non-empty for equal inputs, every cache HIT would be reported as a miss
    // with a fabricated reason.
    assert!(fingerprint_diff(&sample_fingerprint(), &sample_fingerprint()).is_empty());
}

#[test]
fn fingerprint_diff_names_the_field_the_old_log_could_not_show() {
    // The four fields the previous log never printed. Each is checked on its
    // own so the assertion cannot pass because some *other* field differed —
    // the exact way the old two-field log misled readers (openhuman#6414).
    let base = sample_fingerprint();

    let mut model = base.clone();
    model.model_override = Some("gpt-4".to_string());
    let diff = fingerprint_diff(&base, &model);
    assert_eq!(
        diff.len(),
        1,
        "expected exactly one differing field: {diff:?}"
    );
    assert!(
        diff[0].starts_with("model_override:"),
        "diff must name model_override, got {diff:?}"
    );

    let mut temp = base.clone();
    temp.temperature = Some(0.1);
    let diff = fingerprint_diff(&base, &temp);
    assert_eq!(
        diff.len(),
        1,
        "expected exactly one differing field: {diff:?}"
    );
    assert!(
        diff[0].starts_with("temperature:"),
        "diff must name temperature, got {diff:?}"
    );

    let mut autonomy = base.clone();
    autonomy.autonomy_signature =
        r#"{"approval_required":false,"action_dir":"/home/u/w"}"#.to_string();
    let diff = fingerprint_diff(&base, &autonomy);
    assert_eq!(
        diff.len(),
        1,
        "expected exactly one differing field: {diff:?}"
    );
    assert!(
        diff[0].starts_with("autonomy_signature:"),
        "diff must name autonomy_signature, got {diff:?}"
    );

    let mut registry = base.clone();
    registry.model_registry_signature = r#"[{"id":"m","provider":"p","vision":true}]"#.to_string();
    let diff = fingerprint_diff(&base, &registry);
    assert_eq!(
        diff.len(),
        1,
        "expected exactly one differing field: {diff:?}"
    );
    assert!(
        diff[0].starts_with("model_registry_signature:"),
        "diff must name model_registry_signature, got {diff:?}"
    );
}

#[test]
fn fingerprint_diff_summarises_signatures_without_printing_them() {
    // `config.autonomy` carries the user's filesystem paths, so the miss log
    // must not echo the subtree onto the chat hot path. The summary has to be
    // useful *and* quiet: it names where the two diverge, not what they say.
    let base = sample_fingerprint();
    let mut changed = base.clone();
    changed.autonomy_signature =
        r#"{"approval_required":false,"action_dir":"/home/u/secret-dir"}"#.to_string();

    let diff = fingerprint_diff(&base, &changed);
    assert_eq!(diff.len(), 1, "{diff:?}");
    let line = &diff[0];

    assert!(
        !line.contains("secret-dir") && !line.contains("action_dir"),
        "signature contents must not be logged, got {line}"
    );
    assert!(
        line.contains("differs at byte"),
        "summary must locate the change, got {line}"
    );
    assert!(
        line.contains(&base.autonomy_signature.len().to_string()),
        "summary must carry the prior length, got {line}"
    );
}

#[test]
fn fingerprint_diff_reports_every_differing_field() {
    // A miss caused by two fields at once must not report only the first.
    let base = sample_fingerprint();
    let mut next = base.clone();
    next.temperature = None;
    next.provider_binding = "byok".to_string();

    let diff = fingerprint_diff(&base, &next);
    assert_eq!(diff.len(), 2, "{diff:?}");
    assert!(
        diff.iter().any(|d| d.starts_with("temperature:")),
        "{diff:?}"
    );
    assert!(
        diff.iter().any(|d| d.starts_with("provider_binding:")),
        "{diff:?}"
    );
}

/// `[agent] chat_agent_id` is the only lever that moves the web-chat path off
/// the orchestrator. A definition's `effective_max_iterations()` overwrites
/// `agent.max_tool_iterations` in `session_host::builder::factory`, so an
/// operator who needs a longer-running turn has to change *which agent
/// answers*, not the cap — these cases pin that selection.
#[test]
fn chat_agent_id_selects_the_web_chat_agent_and_defaults_to_the_orchestrator() {
    use super::pick_target_agent_id;
    crate::agent::harness::AgentDefinitionRegistry::init_global_builtins().unwrap();

    let mut config = crate::config::Config::default();
    assert_eq!(
        config.agent.chat_agent_id, None,
        "the shipped default leaves it unset"
    );
    assert_eq!(
        pick_target_agent_id(&config),
        "orchestrator",
        "unset falls back to what the app runs"
    );

    config.agent.chat_agent_id = Some("researcher".to_string());
    assert_eq!(pick_target_agent_id(&config), "researcher");

    // Padding is an operator typo in a hand-edited config.toml, not a request
    // for an agent whose id has spaces in it.
    config.agent.chat_agent_id = Some("  researcher  ".to_string());
    assert_eq!(pick_target_agent_id(&config), "researcher");

    // Blank is "unset", not "an agent named empty string": a turn routed at an
    // id the registry cannot answer would fail chat outright.
    for blank in ["", "   "] {
        config.agent.chat_agent_id = Some(blank.to_string());
        assert_eq!(
            pick_target_agent_id(&config),
            "orchestrator",
            "blank {blank:?} falls back rather than routing nowhere"
        );
    }

    config.agent.chat_agent_id = Some("typoed_agent".to_string());
    assert_eq!(
        pick_target_agent_id(&config),
        "orchestrator",
        "an unknown optional setting must not take web chat down"
    );
}
