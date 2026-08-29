//! Unit tests for [`AgentExperienceStore`](super::AgentExperienceStore).
//!
//! A sibling `*_tests.rs` rather than an inline `#[cfg(test)] mod tests`, for
//! two reasons that point the same way. It keeps `store.rs` near the ~500-line
//! guideline, and it is the path both memory ratchets skip
//! (`direct_engine_refs_tests::is_test_path`), which is what lets the two
//! sanitizer regressions below keep constructing a real `UnifiedMemory`. That
//! reference is legitimate: `tinymemory-core` is a dev-dependency after #5560,
//! and a dev-dependency is not linked into the shipped binary — but in
//! `store.rs` it read as an unmigrated production call site and cost an
//! allowlist entry that documented the opposite of the truth.

use super::*;
use crate::openhuman::agent::experience::types::{
    AgentExperience, ExperienceOutcome, ExperienceSource,
};
use crate::openhuman::memory::tool_memory::test_helpers::MockMemory;
use std::sync::Arc;

fn sample_experience(
    id: &str,
    task_summary: &str,
    tools: Vec<&str>,
    tags: Vec<&str>,
    confidence: f32,
) -> AgentExperience {
    let sequence = tools.iter().map(|tool| (*tool).to_string()).collect();
    AgentExperience {
        id: id.to_string(),
        created_at_ms: 1,
        updated_at_ms: 1,
        source: ExperienceSource::ToolLoop,
        agent_id: Some("orchestrator".into()),
        entrypoint: Some("chat".into()),
        profile_id: None,
        task_fingerprint: format!("fp-{id}"),
        task_summary: task_summary.to_string(),
        tools_used: tools.iter().map(|tool| (*tool).to_string()).collect(),
        tool_sequence: sequence,
        outcome: ExperienceOutcome::Success,
        error_class: None,
        lesson: format!("lesson for {task_summary}"),
        reuse_hint: format!("reuse for {task_summary}"),
        avoid_hint: None,
        confidence,
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        payload_hash: None,
        dismissed: false,
    }
}

fn fresh_store() -> (AgentExperienceStore, Arc<MockMemory>) {
    let memory = Arc::new(MockMemory::default());
    (AgentExperienceStore::new(memory.clone()), memory)
}

#[tokio::test]
async fn put_list_and_dismiss_round_trip() {
    let (store, memory) = fresh_store();
    store
        .put(sample_experience(
            "exp_success",
            "search repository docs",
            vec!["grep", "file_read"],
            vec!["docs"],
            0.8,
        ))
        .await
        .unwrap();

    assert!(memory.entries.lock().contains_key(&(
        AGENT_EXPERIENCE_NAMESPACE.into(),
        "experience/exp_success".into()
    )));

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "exp_success");

    let dismissed = store.dismiss("exp_success").await.unwrap();
    assert!(dismissed);
    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].dismissed);
}

/// Regression for #5209. `Memory::store` runs document content through the
/// free-text secret/PII sanitizer, whose credit-card (Luhn-gated) pattern
/// matches any 13–19-digit run. A serialized experience carries bare 13-digit
/// millisecond timestamps, so a Luhn-valid timestamp used to be rewritten to
/// a `[REDACTED_PII_*]` token — corrupting the JSON so the record silently
/// vanished on read (~10% of writes, whichever `now_ms()` happened to be
/// Luhn-valid). Exercised over the *real* `UnifiedMemory` because `MockMemory`
/// does not sanitize. `1785148840502` is a Luhn-valid 13-digit ms timestamp;
/// `put` preserves a positive `created_at_ms`, so the corruption is forced
/// deterministically rather than depending on the wall clock.
#[tokio::test]
async fn experience_survives_content_sanitizer_with_luhn_valid_timestamp() {
    use crate::openhuman::inference::embeddings::NoopEmbedding;
    use crate::openhuman::memory::Memory;
    use tinymemory_core::store::UnifiedMemory;

    let tmp = tempfile::TempDir::new().unwrap();
    let memory: Arc<dyn Memory> =
        Arc::new(UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap());
    let store = AgentExperienceStore::new(memory);

    let mut experience = sample_experience(
        "exp_luhn",
        "Deploy the Rust service safely",
        vec![],
        vec![],
        0.9,
    );
    // Luhn-valid 13-digit ms timestamp; preserved by `put` (positive value),
    // so the vulnerable numeric field is present on every run.
    experience.created_at_ms = 1_785_148_840_502;
    experience.lesson = "Legacy shared deployment guidance".into();

    store.put(experience).await.unwrap();

    let listed = store.list().await.unwrap();
    assert_eq!(
        listed.len(),
        1,
        "experience must survive the memory content sanitizer round-trip"
    );
    assert_eq!(listed[0].lesson, "Legacy shared deployment guidance");
    assert_eq!(listed[0].created_at_ms, 1_785_148_840_502);
}

/// Security regression for PR #5211 review (P1). Base64-encoding the payload
/// makes the memory layer's store-time content scrubber a no-op over the
/// stored bytes, so the store must run the full scrubber over free-text
/// fields itself before serialization ([`redact_experience`]). A secret in a
/// captured free-text field must be redacted in the recalled record (so it
/// is neither stored nor returned reversibly), while the numeric timestamp
/// survives intact. Exercised over the real `UnifiedMemory` (the store path
/// that base64 now shields).
#[tokio::test]
async fn secrets_in_free_text_are_redacted_before_storage() {
    use crate::openhuman::inference::embeddings::NoopEmbedding;
    use crate::openhuman::memory::Memory;
    use tinymemory_core::store::UnifiedMemory;

    let tmp = tempfile::TempDir::new().unwrap();
    let memory: Arc<dyn Memory> =
        Arc::new(UnifiedMemory::new(tmp.path(), Arc::new(NoopEmbedding), None).unwrap());
    let store = AgentExperienceStore::new(memory);

    let mut experience = sample_experience("exp_secret", "deploy the service", vec![], vec![], 0.9);
    // Luhn-valid 13-digit ms timestamp; must survive intact (the #5209 fix).
    experience.created_at_ms = 1_785_148_840_502;
    experience.lesson = "provider token sk_live_12345678901234567890 then dial +15551234567".into();
    experience.reuse_hint =
        "-----BEGIN PRIVATE KEY-----\nMIIabc123\n-----END PRIVATE KEY-----".into();
    experience.error_class = Some("phone leaked: +15551234567".into());

    store.put(experience).await.unwrap();

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1, "record must still parse and round-trip");
    let recalled = &listed[0];

    // (a) secrets are redacted in the recalled free-text fields.
    assert!(
        !recalled.lesson.contains("sk_live_12345678901234567890"),
        "Stripe key must be redacted, got: {}",
        recalled.lesson
    );
    assert!(
        !recalled.lesson.contains("+15551234567"),
        "phone number must be redacted, got: {}",
        recalled.lesson
    );
    assert!(
        !recalled.reuse_hint.contains("PRIVATE KEY"),
        "private-key block must be redacted, got: {}",
        recalled.reuse_hint
    );
    assert!(
        recalled
            .error_class
            .as_deref()
            .is_none_or(|e| !e.contains("+15551234567")),
        "phone in error_class must be redacted, got: {:?}",
        recalled.error_class
    );
    assert!(
        recalled.lesson.contains("REDACTED") && recalled.reuse_hint.contains("REDACTED"),
        "expected redaction markers in scrubbed fields"
    );

    // (b) the numeric timestamp survives intact and the record parses.
    assert_eq!(recalled.created_at_ms, 1_785_148_840_502);
}

#[tokio::test]
async fn generated_ids_partition_identical_experiences_by_profile() {
    let (store, _) = fresh_store();
    let mut alice = sample_experience("", "same task", vec!["grep"], vec!["docs"], 0.8);
    alice.profile_id = Some("alice".into());
    let mut bob = alice.clone();
    bob.profile_id = Some("bob".into());

    let alice = store.put(alice).await.unwrap();
    let bob = store.put(bob).await.unwrap();

    assert_ne!(alice.id, bob.id);
    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .any(|item| item.profile_id.as_deref() == Some("alice")));
    assert!(listed
        .iter()
        .any(|item| item.profile_id.as_deref() == Some("bob")));
}

#[tokio::test]
async fn retrieve_ranks_tool_and_query_matches() {
    let (store, _) = fresh_store();
    store
        .put(sample_experience(
            "exp_docs",
            "search repository docs",
            vec!["grep", "file_read"],
            vec!["docs"],
            0.6,
        ))
        .await
        .unwrap();
    store
        .put(sample_experience(
            "exp_email",
            "send a careful email",
            vec!["email"],
            vec!["mail"],
            1.0,
        ))
        .await
        .unwrap();

    let hits = store
        .retrieve(ExperienceQuery {
            query: "search docs with grep".into(),
            tools: vec!["grep".into()],
            tags: vec!["docs".into()],
            agent_id: Some("orchestrator".into()),
            entrypoint: Some("chat".into()),
            profile_id: None,
            max_hits: 2,
        })
        .await
        .unwrap();

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].experience.id, "exp_docs");
    assert!(hits[0].score > hits[1].score);
    assert!(hits[0].match_reasons.contains(&"tool_overlap".into()));
    assert!(hits[0].match_reasons.contains(&"query_overlap".into()));
}

#[test]
fn experience_matches_profile_partition_rules() {
    // Profile-less query sees everything.
    assert!(experience_matches_profile(None, None));
    assert!(experience_matches_profile(Some("p"), None));
    // Profiled query: own + legacy in, sibling out.
    assert!(experience_matches_profile(Some("p"), Some("p")));
    assert!(experience_matches_profile(None, Some("p")));
    assert!(!experience_matches_profile(Some("q"), Some("p")));
}

async fn seed_partitioned(store: &AgentExperienceStore) {
    let mut own = sample_experience("exp_p", "task p", vec!["grep"], vec!["docs"], 0.8);
    own.profile_id = Some("p".into());
    store.put(own).await.unwrap();

    let mut sibling = sample_experience("exp_q", "task q", vec!["grep"], vec!["docs"], 0.8);
    sibling.profile_id = Some("q".into());
    store.put(sibling).await.unwrap();

    // Unstamped legacy record.
    store
        .put(sample_experience(
            "exp_legacy",
            "task legacy",
            vec!["grep"],
            vec!["docs"],
            0.8,
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn retrieve_partitions_by_profile() {
    let (store, _) = fresh_store();
    seed_partitioned(&store).await;

    // Profile P: sees P + legacy, never Q.
    let hits = store
        .retrieve(ExperienceQuery {
            query: "task".into(),
            tools: vec!["grep".into()],
            tags: vec!["docs".into()],
            profile_id: Some("p".into()),
            max_hits: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    let ids: BTreeSet<_> = hits.iter().map(|h| h.experience.id.clone()).collect();
    assert!(ids.contains("exp_p"), "profile P must see its own record");
    assert!(
        ids.contains("exp_legacy"),
        "profile P must see legacy records"
    );
    assert!(!ids.contains("exp_q"), "profile P must not see sibling Q");

    // Profile-less: sees everything.
    let all = store
        .retrieve(ExperienceQuery {
            query: "task".into(),
            tools: vec!["grep".into()],
            tags: vec!["docs".into()],
            profile_id: None,
            max_hits: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    let all_ids: BTreeSet<_> = all.iter().map(|h| h.experience.id.clone()).collect();
    assert!(all_ids.contains("exp_p"));
    assert!(all_ids.contains("exp_q"));
    assert!(all_ids.contains("exp_legacy"));
}

#[tokio::test]
async fn dismiss_for_profile_rejects_sibling_record() {
    let (store, _) = fresh_store();
    seed_partitioned(&store).await;

    assert!(!store.dismiss_for_profile("exp_q", Some("p")).await.unwrap());
    assert!(store
        .list()
        .await
        .unwrap()
        .iter()
        .find(|experience| experience.id == "exp_q")
        .is_some_and(|experience| !experience.dismissed));

    assert!(store
        .dismiss_for_profile("exp_legacy", Some("p"))
        .await
        .unwrap());
    assert!(store.dismiss_for_profile("exp_p", Some("p")).await.unwrap());
}

#[tokio::test]
async fn list_for_profile_partitions() {
    let (store, _) = fresh_store();
    seed_partitioned(&store).await;

    let p_ids: BTreeSet<_> = store
        .list_for_profile(Some("p"))
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(
        p_ids,
        BTreeSet::from(["exp_legacy".to_string(), "exp_p".to_string()])
    );

    let all_ids: BTreeSet<_> = store
        .list_for_profile(None)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(
        all_ids,
        BTreeSet::from([
            "exp_legacy".to_string(),
            "exp_p".to_string(),
            "exp_q".to_string()
        ])
    );
}

#[tokio::test]
async fn retrieve_ignores_dismissed_records() {
    let (store, _) = fresh_store();
    store
        .put(sample_experience(
            "exp_dismissed",
            "search repository docs",
            vec!["grep", "file_read"],
            vec!["docs"],
            0.8,
        ))
        .await
        .unwrap();
    store.dismiss("exp_dismissed").await.unwrap();

    let hits = store
        .retrieve(ExperienceQuery {
            query: "search repository docs".into(),
            tools: vec!["grep".into()],
            tags: vec!["docs".into()],
            agent_id: None,
            entrypoint: None,
            profile_id: None,
            max_hits: 5,
        })
        .await
        .unwrap();

    assert!(hits.is_empty());
}
