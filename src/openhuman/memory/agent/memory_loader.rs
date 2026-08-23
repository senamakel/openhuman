use crate::openhuman::memory::Memory;

/// Maximum number of `[Prior conversations]` lines surfaced into the prompt
/// at the start of a fresh chat. Tight cap on purpose: this block is meant
/// to recover continuity for high-importance facts, not to dump session
/// history into context. See issue #1399.
const PRIOR_CONVERSATION_LIMIT: usize = 3;
/// Only the importance prefix `high.` survives into the prompt block.
/// Medium/low entries stay queryable via the on-demand memory tool but
/// do not auto-pollute every fresh chat.
const PRIOR_CONVERSATION_KEY_PREFIX: &str = "high.";

/// Canonical header for the `[Cross-chat context]` block injected on
/// every turn that has FTS-surfaced hits from other threads.
///
/// The "historical" / "capabilities may have changed since" suffix is
/// deliberate: it tells the model these snippets are snapshots from
/// earlier moments and that capability claims (e.g. "I can't delete
/// emails") may be stale because the tool surface or per-toolkit scope
/// toggles can change between chats.
///
/// Single source of truth — all three call sites bind to this constant
/// so a wording tweak doesn't drift between (a) `memory_loader.rs`'s
/// primary JSONL path, (b) `harness/memory_context.rs`'s fallback
/// recall path, and (c) the orchestrator's "Capability questions"
/// prompt section that names the header verbatim. Tests assert on this
/// constant too — see `memory_loader::tests` and
/// `harness::memory_context::tests`.
// Both live in the ungated `memory::citation` and are re-exported here so
// every historical `memory::agent::memory_loader::…` path keeps resolving.
// They are inert types in always-compiled signatures; the *producer* below is
// the part that needs a driver. See that module's docs.
pub use crate::openhuman::memory::citation::{MemoryCitation, CROSS_CHAT_HEADER};

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
    // Routed through the tinyagents retrieval facade (issue #4249, 09.2): the
    // facade wraps `Memory::recall` verbatim (ranking engine unchanged) so the
    // citation set stays byte-identical, while making retrieval swappable and
    // emitting `MemoryLoaded`.
    let entries = crate::openhuman::agent::tinyagents::retriever::recall_through_facade(
        memory,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::memory::{Memory, MemoryCategory, MemoryEntry};
    use async_trait::async_trait;

    struct MockMemory {
        entries: Vec<MemoryEntry>,
        cross_chat: Vec<MemoryEntry>,
    }

    impl MockMemory {
        fn new(entries: Vec<MemoryEntry>) -> Self {
            Self {
                entries,
                cross_chat: Vec::new(),
            }
        }
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
            opts: crate::openhuman::memory::RecallOpts<'_>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            if opts.cross_session {
                return Ok(self.cross_chat.clone());
            }
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
            taint: Default::default(),
        }
    }

    #[tokio::test]
    async fn collect_recall_citations_filters_and_truncates_entries() {
        let mem = MockMemory::new(vec![
            entry("keep", "useful context", Some(0.9)),
            entry("drop", "too weak", Some(0.1)),
            entry("long", &"x".repeat(600), Some(0.8)),
        ]);

        let citations = collect_recall_citations(&mem, "hello", 5, 0.4)
            .await
            .expect("citation collection should succeed");
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].key, "keep");
        assert_eq!(citations[1].key, "long");
        assert!(citations[1].snippet.ends_with("..."));
    }

    // ── Cross-chat context (#1505) ───────────────────────────────────────
}
