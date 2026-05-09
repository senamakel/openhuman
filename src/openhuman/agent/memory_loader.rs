use crate::openhuman::memory::Memory;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::harness::memory_context::{WORKING_MEMORY_KEY_PREFIX, WORKING_MEMORY_LIMIT};
use crate::openhuman::learning::transcript_ingest::CONVERSATION_MEMORY_NAMESPACE;

/// Maximum number of `[Prior conversations]` lines surfaced into the prompt
/// at the start of a fresh chat. Tight cap on purpose: this block is meant
/// to recover continuity for high-importance facts, not to dump session
/// history into context. See issue #1399.
const PRIOR_CONVERSATION_LIMIT: usize = 3;
/// Only the importance prefix `high.` survives into the prompt block.
/// Medium/low entries stay queryable via the on-demand memory tool but
/// do not auto-pollute every fresh chat.
const PRIOR_CONVERSATION_KEY_PREFIX: &str = "high.";

#[async_trait]
pub trait MemoryLoader: Send + Sync {
    async fn load_context(&self, memory: &dyn Memory, user_message: &str)
        -> anyhow::Result<String>;
}

pub struct DefaultMemoryLoader {
    limit: usize,
    min_relevance_score: f64,
    /// Maximum characters of memory context to inject (0 = unlimited).
    max_context_chars: usize,
}

/// Lightweight citation object derived from recalled memory entries.
///
/// These citations are attached to agent responses so the UI can show
/// provenance for memory-informed answers without exposing full raw memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryCitation {
    pub id: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub timestamp: String,
    pub snippet: String,
}

impl Default for DefaultMemoryLoader {
    fn default() -> Self {
        Self {
            limit: 5,
            min_relevance_score: 0.4,
            max_context_chars: 2000,
        }
    }
}

impl DefaultMemoryLoader {
    pub fn new(limit: usize, min_relevance_score: f64) -> Self {
        Self {
            limit: limit.max(1),
            min_relevance_score,
            max_context_chars: 2000,
        }
    }

    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_context_chars = max_chars;
        self
    }
}

/// Collect citation metadata from semantic memory recall for a user turn.
///
/// This mirrors the primary recall path used by `DefaultMemoryLoader` so the
/// UI can display trusted sources whenever memory context influenced a reply.
pub async fn collect_recall_citations(
    memory: &dyn Memory,
    user_message: &str,
    limit: usize,
    min_relevance_score: f64,
) -> anyhow::Result<Vec<MemoryCitation>> {
    let entries = memory
        .recall(
            user_message,
            limit.max(1),
            crate::openhuman::memory::RecallOpts::default(),
        )
        .await?;

    let citations = entries
        .into_iter()
        .filter(|entry| match entry.score {
            Some(score) => score >= min_relevance_score,
            None => true,
        })
        .map(|entry| {
            let snippet = if entry.content.chars().count() > 280 {
                crate::openhuman::util::truncate_with_ellipsis(&entry.content, 280)
            } else {
                entry.content
            };
            MemoryCitation {
                id: entry.id,
                key: entry.key,
                namespace: entry.namespace,
                score: entry.score,
                timestamp: entry.timestamp,
                snippet,
            }
        })
        .collect();

    Ok(citations)
}

#[async_trait]
impl MemoryLoader for DefaultMemoryLoader {
    async fn load_context(
        &self,
        memory: &dyn Memory,
        user_message: &str,
    ) -> anyhow::Result<String> {
        // Primary `[Memory context]` semantic recall used to be injected here,
        // but it duplicated content the agent can already reach via the
        // compressed memory tree (eager prefetch) and the on-demand memory
        // search tool — and worse, the auto-saved `user_msg` entry would come
        // back as the top "relevant" memory and echo the user's text back at
        // them. Only the bounded `[User working memory]` block remains: it
        // surfaces sync-derived profile facts (timezone, preferences) that the
        // tree digest doesn't always carry, and it is keyed by a fixed
        // `working.user.*` namespace so it can't catch arbitrary chat content.
        let mut context = String::new();
        let budget = if self.max_context_chars > 0 {
            self.max_context_chars
        } else {
            usize::MAX
        };

        let working_query = format!("working.user {user_message}");
        let working_entries = memory
            .recall(
                &working_query,
                WORKING_MEMORY_LIMIT + 2,
                crate::openhuman::memory::RecallOpts::default(),
            )
            .await
            .unwrap_or_default();
        let mut appended_working_header = false;
        for entry in working_entries
            .into_iter()
            .filter(|entry| entry.key.starts_with(WORKING_MEMORY_KEY_PREFIX))
            .filter(|entry| match entry.score {
                Some(score) => score >= self.min_relevance_score,
                None => true,
            })
            .take(WORKING_MEMORY_LIMIT)
        {
            if !appended_working_header {
                let section = "[User working memory]\n";
                if section.len() > budget {
                    break;
                }
                context.push_str(section);
                appended_working_header = true;
            }
            let line = format!("- {}: {}\n", entry.key, entry.content);
            if context.len() + line.len() > budget {
                tracing::debug!(
                    budget,
                    current_len = context.len(),
                    skipped_line_len = line.len(),
                    "[memory_loader] context budget reached while appending working memory"
                );
                break;
            }
            context.push_str(&line);
        }

        // ── Prior conversations (issue #1399) ─────────────────────────
        // High-importance, transcript-derived facts from earlier chats.
        // Namespace-scoped recall keeps this block small and tightly
        // bounded — only entries the heuristic extractor flagged as
        // `high.*` are eligible, and only the first short snippet of
        // each is included so the block never crowds out the user's
        // actual message.
        let prior_query = format!("{} {}", CONVERSATION_MEMORY_NAMESPACE, user_message);
        let prior_entries = memory
            .recall(
                &prior_query,
                PRIOR_CONVERSATION_LIMIT * 4,
                crate::openhuman::memory::RecallOpts {
                    namespace: Some(CONVERSATION_MEMORY_NAMESPACE),
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_default();

        let mut appended_prior_header = false;
        let mut prior_added = 0usize;
        for entry in prior_entries
            .into_iter()
            .filter(|e| e.key.starts_with(PRIOR_CONVERSATION_KEY_PREFIX))
            .filter(|e| match e.score {
                Some(score) => score >= self.min_relevance_score,
                None => true,
            })
        {
            if prior_added >= PRIOR_CONVERSATION_LIMIT {
                break;
            }
            // The stored content is two lines:
            //   [high preference] I prefer Postgres ...
            //   [provenance] {"thread_id":"thr_…", ...}
            // For the prompt we keep only the first line so the block
            // stays compact. Provenance survives in the underlying
            // memory entry and is queryable through the memory tool.
            let primary = entry
                .content
                .lines()
                .find(|l| !l.trim_start().starts_with("[provenance]"))
                .unwrap_or(&entry.content)
                .trim();
            if primary.is_empty() {
                continue;
            }
            if !appended_prior_header {
                let section = "[Prior conversations]\n";
                if context.len() + section.len() > budget {
                    break;
                }
                context.push_str(section);
                appended_prior_header = true;
            }
            let line = format!("- {primary}\n");
            if context.len() + line.len() > budget {
                tracing::debug!(
                    budget,
                    current_len = context.len(),
                    skipped_line_len = line.len(),
                    "[memory_loader] context budget reached while appending prior conversations"
                );
                break;
            }
            context.push_str(&line);
            prior_added += 1;
        }

        if context.is_empty() {
            return Ok(String::new());
        }
        context.push('\n');
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory::{Memory, MemoryCategory, MemoryEntry};

    struct MockMemory {
        entries: Vec<MemoryEntry>,
    }

    #[async_trait]
    impl Memory for MockMemory {
        fn name(&self) -> &str {
            "mock"
        }

        async fn store(
            &self,
            _namespace: &str,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            _limit: usize,
            _opts: crate::openhuman::memory::RecallOpts<'_>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(self.entries.clone())
        }

        async fn get(&self, _namespace: &str, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _namespace: Option<&str>,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(Vec::new())
        }

        async fn forget(&self, _namespace: &str, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn namespace_summaries(
            &self,
        ) -> anyhow::Result<Vec<crate::openhuman::memory::NamespaceSummary>> {
            Ok(Vec::new())
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(self.entries.len())
        }

        async fn health_check(&self) -> bool {
            true
        }
    }

    fn entry(key: &str, content: &str, score: Option<f64>) -> MemoryEntry {
        MemoryEntry {
            id: format!("id-{key}"),
            key: key.to_string(),
            content: content.to_string(),
            namespace: Some("test".to_string()),
            category: MemoryCategory::Conversation,
            timestamp: "2026-04-22T00:00:00Z".to_string(),
            session_id: None,
            score,
        }
    }

    #[tokio::test]
    async fn loader_surfaces_prior_conversation_high_importance_only() {
        // Prior chat extracted two memories: one high-importance preference
        // and one medium-importance unresolved task. Only the high one
        // should make it into the loader's prompt block (#1399).
        let mem = MockMemory {
            entries: vec![
                MemoryEntry {
                    id: "id-1".into(),
                    key: "high.preference.aaaaaaaaaaaa".into(),
                    content: "[high preference] I prefer Postgres for new services.\n[provenance] {\"thread_id\":\"thr_old\"}".into(),
                    namespace: Some(super::CONVERSATION_MEMORY_NAMESPACE.to_string()),
                    category: MemoryCategory::Conversation,
                    timestamp: "2026-04-22T00:00:00Z".into(),
                    session_id: Some("thr_old".into()),
                    score: Some(0.9),
                },
                MemoryEntry {
                    id: "id-2".into(),
                    key: "med.unresolved_task.bbbbbbbbbbbb".into(),
                    content: "[med unresolved_task] still need to migrate auth.".into(),
                    namespace: Some(super::CONVERSATION_MEMORY_NAMESPACE.to_string()),
                    category: MemoryCategory::Conversation,
                    timestamp: "2026-04-22T00:00:00Z".into(),
                    session_id: None,
                    score: Some(0.9),
                },
            ],
        };

        let loader = DefaultMemoryLoader::default();
        let out = loader
            .load_context(&mem, "what should I default to for storage?")
            .await
            .expect("loader must succeed");

        assert!(
            out.contains("[Prior conversations]"),
            "expected prior conversations block, got:\n{out}"
        );
        assert!(out.contains("Postgres"));
        assert!(
            !out.contains("migrate auth"),
            "med-importance entries must not auto-surface, got:\n{out}"
        );
        assert!(
            !out.contains("[provenance]"),
            "provenance is not rendered into the prompt block, got:\n{out}"
        );
    }

    #[tokio::test]
    async fn collect_recall_citations_filters_and_truncates_entries() {
        let mem = MockMemory {
            entries: vec![
                entry("keep", "useful context", Some(0.9)),
                entry("drop", "too weak", Some(0.1)),
                entry("long", &"x".repeat(600), Some(0.8)),
            ],
        };

        let citations = collect_recall_citations(&mem, "hello", 5, 0.4)
            .await
            .expect("citation collection should succeed");
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].key, "keep");
        assert_eq!(citations[1].key, "long");
        assert!(citations[1].snippet.ends_with("..."));
    }
}
