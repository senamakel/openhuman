use crate::openhuman::agent_experience::types::{
    redact_text, stable_experience_id_for_profile, AgentExperience, ExperienceHit,
};
use crate::openhuman::memory::{Memory, MemoryCategory};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const AGENT_EXPERIENCE_NAMESPACE: &str = "agent_experience";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperienceQuery {
    pub query: String,
    pub tools: Vec<String>,
    pub tags: Vec<String>,
    pub agent_id: Option<String>,
    pub entrypoint: Option<String>,
    /// Profile partition (1c). When `Some(P)`, retrieval returns records stamped
    /// `P` plus unstamped legacy records and excludes records stamped with a
    /// different profile. When `None` (the profile-less session), every record
    /// is in scope — see [`experience_matches_profile`].
    pub profile_id: Option<String>,
    pub max_hits: usize,
}

/// Profile partition predicate shared by retrieval and RPC list (1c).
///
/// - A **profile-less** query (`query_profile == None`) sees **everything**:
///   the default session historically owns the whole shared experience pool and
///   must keep recalling every record it and prior versions wrote, so narrowing
///   it would silently drop guidance the default agent still relies on.
/// - A **profiled** query (`Some(P)`) sees records stamped `P` plus unstamped
///   legacy/shared records (`record_profile == None`), and excludes records
///   stamped with a different profile `Q` — the isolation the feature adds.
pub fn experience_matches_profile(
    record_profile: Option<&str>,
    query_profile: Option<&str>,
) -> bool {
    match query_profile {
        None => true,
        Some(active) => match record_profile {
            None => true,
            Some(owner) => owner == active,
        },
    }
}

#[derive(Clone)]
pub struct AgentExperienceStore {
    memory: Arc<dyn Memory>,
}

impl AgentExperienceStore {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    pub async fn put(&self, mut experience: AgentExperience) -> Result<AgentExperience, String> {
        if experience.id.trim().is_empty() {
            experience.id = stable_experience_id_for_profile(
                &experience.task_summary,
                &experience.tool_sequence,
                experience.outcome,
                experience.profile_id.as_deref(),
            );
        }
        if experience.task_summary.trim().is_empty() {
            return Err("task_summary is required".to_string());
        }
        if experience.lesson.trim().is_empty() {
            return Err("lesson is required".to_string());
        }

        let key = storage_key(&experience.id);
        if let Some(existing) = self.fetch(&key).await? {
            experience.created_at_ms = existing.created_at_ms;
        } else if experience.created_at_ms <= 0 {
            experience.created_at_ms = now_ms();
        }
        experience.updated_at_ms = now_ms();
        experience = redact_experience(experience);

        let content = serde_json::to_string(&experience).map_err(|e| e.to_string())?;
        self.memory
            .store(
                AGENT_EXPERIENCE_NAMESPACE,
                &key,
                &content,
                MemoryCategory::Custom(AGENT_EXPERIENCE_NAMESPACE.into()),
                None,
            )
            .await
            .map_err(|e| format!("store agent experience: {e:#}"))?;

        Ok(experience)
    }

    pub async fn list(&self) -> Result<Vec<AgentExperience>, String> {
        let entries = self
            .memory
            .list(Some(AGENT_EXPERIENCE_NAMESPACE), None, None)
            .await
            .map_err(|e| format!("list agent experiences: {e:#}"))?;

        let mut experiences: Vec<AgentExperience> = entries
            .into_iter()
            .filter(|entry| entry.key.starts_with("experience/"))
            .filter_map(
                |entry| match serde_json::from_str::<AgentExperience>(&entry.content) {
                    Ok(experience) => Some(experience),
                    Err(err) => {
                        log::warn!(
                            "[agent-experience] skipping malformed entry key={}: {err}",
                            entry.key
                        );
                        None
                    }
                },
            )
            .collect();

        experiences.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(experiences)
    }

    /// [`Self::list`] narrowed to a profile partition (1c). `profile_id == None`
    /// returns everything (the profile-less view); `Some(P)` returns records
    /// stamped `P` plus unstamped legacy records. Shares
    /// [`experience_matches_profile`] with retrieval so the two never diverge.
    pub async fn list_for_profile(
        &self,
        profile_id: Option<&str>,
    ) -> Result<Vec<AgentExperience>, String> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter(|experience| {
                experience_matches_profile(experience.profile_id.as_deref(), profile_id)
            })
            .collect())
    }

    pub async fn dismiss(&self, id: &str) -> Result<bool, String> {
        self.dismiss_for_profile(id, None).await
    }

    /// Dismiss an experience only when it belongs to the caller's visible
    /// profile partition. A profiled caller may dismiss its own or unstamped
    /// legacy records, but never a sibling profile's record even if it knows
    /// the storage id.
    pub async fn dismiss_for_profile(
        &self,
        id: &str,
        profile_id: Option<&str>,
    ) -> Result<bool, String> {
        let key = storage_key(id);
        let Some(mut experience) = self.fetch(&key).await? else {
            return Ok(false);
        };
        if !experience_matches_profile(experience.profile_id.as_deref(), profile_id) {
            return Ok(false);
        }
        experience.dismissed = true;
        experience.updated_at_ms = now_ms();
        self.put(experience).await?;
        Ok(true)
    }

    pub async fn retrieve(&self, query: ExperienceQuery) -> Result<Vec<ExperienceHit>, String> {
        if query.max_hits == 0 {
            return Ok(Vec::new());
        }

        let query_terms = terms(&query.query);
        let query_tools = normalized_set(&query.tools);
        let query_tags = normalized_set(&query.tags);

        let mut hits: Vec<ExperienceHit> = self
            .list()
            .await?
            .into_iter()
            .filter(|experience| !experience.dismissed)
            .filter(|experience| {
                experience_matches_profile(
                    experience.profile_id.as_deref(),
                    query.profile_id.as_deref(),
                )
            })
            .filter_map(|experience| {
                let (score, match_reasons) = score_experience(
                    &experience,
                    &query_terms,
                    &query_tools,
                    &query_tags,
                    query.agent_id.as_deref(),
                    query.entrypoint.as_deref(),
                );
                (score > 0.0).then_some(ExperienceHit {
                    experience,
                    score,
                    match_reasons,
                })
            })
            .collect();

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| b.experience.updated_at_ms.cmp(&a.experience.updated_at_ms))
                .then_with(|| a.experience.id.cmp(&b.experience.id))
        });
        hits.truncate(query.max_hits);
        Ok(hits)
    }

    async fn fetch(&self, key: &str) -> Result<Option<AgentExperience>, String> {
        let entry = self
            .memory
            .get(AGENT_EXPERIENCE_NAMESPACE, key)
            .await
            .map_err(|e| format!("get agent experience: {e:#}"))?;
        match entry {
            Some(entry) => serde_json::from_str::<AgentExperience>(&entry.content)
                .map(Some)
                .map_err(|e| format!("parse agent experience: {e}")),
            None => Ok(None),
        }
    }
}

/// Retrieve one logical experience pool across multiple physical memory stores.
///
/// Dedicated profiles write new experiences into their own memory subtree, but
/// still need to recall unstamped legacy experiences from the shared store.
/// Keep the merge, de-duplication, ordering, and final limit in one place so the
/// RPC and live-turn paths cannot drift.
pub async fn retrieve_across_stores(
    stores: &[AgentExperienceStore],
    query: ExperienceQuery,
) -> Result<Vec<ExperienceHit>, String> {
    let max_hits = query.max_hits;
    let mut by_id: BTreeMap<String, ExperienceHit> = BTreeMap::new();
    for store in stores {
        for hit in store.retrieve(query.clone()).await? {
            let id = hit.experience.id.clone();
            match by_id.get(&id) {
                Some(existing) if existing.score >= hit.score => {}
                _ => {
                    by_id.insert(id, hit);
                }
            }
        }
    }
    let mut hits: Vec<_> = by_id.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.experience.updated_at_ms.cmp(&a.experience.updated_at_ms))
            .then_with(|| a.experience.id.cmp(&b.experience.id))
    });
    hits.truncate(max_hits);
    Ok(hits)
}

fn storage_key(id: &str) -> String {
    format!("experience/{}", id.trim())
}

fn redact_experience(mut experience: AgentExperience) -> AgentExperience {
    experience.task_summary = redact_text(&experience.task_summary);
    experience.lesson = redact_text(&experience.lesson);
    experience.reuse_hint = redact_text(&experience.reuse_hint);
    experience.avoid_hint = experience.avoid_hint.map(|hint| redact_text(&hint));
    experience
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn score_experience(
    experience: &AgentExperience,
    query_terms: &BTreeSet<String>,
    query_tools: &BTreeSet<String>,
    query_tags: &BTreeSet<String>,
    agent_id: Option<&str>,
    entrypoint: Option<&str>,
) -> (f32, Vec<String>) {
    let mut score = experience.confidence.clamp(0.0, 1.0) * 0.2;
    let mut reasons = Vec::new();

    let experience_tools = normalized_set(&experience.tools_used);
    let tool_overlap = overlap_count(query_tools, &experience_tools);
    if tool_overlap > 0 {
        score += 3.0 + tool_overlap as f32 * 0.5;
        reasons.push("tool_overlap".to_string());
    }

    let experience_tags = normalized_set(&experience.tags);
    let tag_overlap = overlap_count(query_tags, &experience_tags);
    if tag_overlap > 0 {
        score += 2.0 + tag_overlap as f32 * 0.25;
        reasons.push("tag_overlap".to_string());
    }

    let haystack = terms(&format!(
        "{} {} {} {}",
        experience.task_summary,
        experience.lesson,
        experience.reuse_hint,
        experience.avoid_hint.as_deref().unwrap_or_default()
    ));
    let query_overlap = overlap_count(query_terms, &haystack);
    if query_overlap > 0 {
        score += 1.0 + query_overlap as f32 * 0.2;
        reasons.push("query_overlap".to_string());
    }

    if let (Some(query_agent), Some(exp_agent)) = (agent_id, experience.agent_id.as_deref()) {
        if normalize(query_agent) == normalize(exp_agent) {
            score += 1.0;
            reasons.push("agent_match".to_string());
        }
    }

    if let (Some(query_entrypoint), Some(exp_entrypoint)) =
        (entrypoint, experience.entrypoint.as_deref())
    {
        if normalize(query_entrypoint) == normalize(exp_entrypoint) {
            score += 0.5;
            reasons.push("entrypoint_match".to_string());
        }
    }

    (score, reasons)
}

fn normalized_set(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| normalize(value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn terms(input: &str) -> BTreeSet<String> {
    input
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(normalize)
        .filter(|term| term.len() > 2)
        .collect()
}

fn overlap_count(a: &BTreeSet<String>, b: &BTreeSet<String>) -> usize {
    a.intersection(b).count()
}

fn normalize(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::agent_experience::types::{
        AgentExperience, ExperienceOutcome, ExperienceSource,
    };
    use crate::openhuman::memory_tools::test_helpers::MockMemory;
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
}
