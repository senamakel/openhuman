//! `RecordingProvider`'s type, constructor, call log, and the guard/entity
//! fixtures the family tests build from.
//!
//! Split out of the original `test_support.rs` (see that module's doc
//! comment for why `RecordingProvider` implements every family rather than
//! relying on the null driver). The family trait implementations that answer
//! each call recorded here live in the sibling `*_impls` files under
//! [`super`].

use std::sync::{Arc, Mutex};

use crate::memory::api::provider::episodic::ConversationSegment;
use crate::memory::api::provider::episodic::EpisodicTurn;
use crate::memory::api::provider::retrieval::RetrievalResponse;
use crate::memory::api::provider::types::{ExportRecord, SourceScope};
use crate::memory::api::provider::MemoryProvider;
use crate::memory::api::types::{
    MemoryCategory, MemoryEntry, MemoryTaint, NamespaceDocumentInput, NamespaceMemoryHit,
    NamespaceSummary,
};

/// One call that reached the driver.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: String,
    /// Content the driver was handed, when the method carries any.
    pub content: Option<String>,
    /// Provenance the driver was handed, when the method carries any.
    pub taint: Option<MemoryTaint>,
    /// Whether the method received a `Some(scope)`.
    pub scoped: Option<bool>,
}

/// The scope's allow list rendered for assertions, sorted for determinism.
pub(super) fn rendered_scope(scope: Option<&SourceScope>) -> Option<String> {
    scope.map(|s| {
        let mut allow = s.allow.clone();
        allow.sort();
        allow.join(",")
    })
}

impl Call {
    pub(crate) fn plain(method: &str) -> Self {
        Self {
            method: method.into(),
            content: None,
            taint: None,
            scoped: None,
        }
    }
}

/// A provider that records and answers with empties.
pub struct RecordingProvider {
    pub(super) calls: Mutex<Vec<Call>>,
    /// What `recall` returns, so budget tests can drive a known result set.
    pub(super) recall_result: Mutex<Vec<MemoryEntry>>,
    /// What `fast_retrieve` returns, so the auto-recall lane can be driven
    /// through a real guard with known hits.
    pub(super) fast_retrieve_result: Mutex<RetrievalResponse>,
    /// What `recall_namespace_scored` returns, so the vector-floored recall
    /// paths (Lane B, the contradiction check) can be driven with known scores.
    pub(super) namespace_hits: Mutex<Vec<NamespaceMemoryHit>>,
    /// What `namespaces` returns, so a namespace can look populated (Lane B
    /// asks for the count before it pays for an embed) without a real store.
    pub(super) namespace_summaries: Mutex<Vec<NamespaceSummary>>,
    /// What `session_turns` returns, so the archivist's finalize path can be
    /// driven past its empty-entries early return without an engine behind it.
    pub(super) session_turns: Mutex<Vec<EpisodicTurn>>,
    /// What `segments_pending_summary` returns, so the re-summarisation pass
    /// (#6186) can be driven over a known queue.
    pub(super) pending_segments: Mutex<Vec<ConversationSegment>>,
}

impl Default for RecordingProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`GuardPolicy`](super::super::GuardPolicy) over an embedded driver with default
/// budgets — the shipped configuration.
pub fn embedded_policy() -> super::super::GuardPolicy {
    super::super::GuardPolicy::new(
        "recording",
        crate::core::subsystem::DriverClass::Embedded,
        crate::config::schema::MemoryHooksConfig::default(),
        super::super::policy::TRUSTED,
    )
}

/// A policy over an *external* driver. No such driver can bind today
/// (`binding::admit` refuses them), so this is the only way to reach the class
/// branches that land for real in M6.
pub fn external_policy(trust_state: &str) -> super::super::GuardPolicy {
    super::super::GuardPolicy::new(
        "supermemory",
        crate::core::subsystem::DriverClass::External,
        crate::config::schema::MemoryHooksConfig::default(),
        trust_state,
    )
}

/// An [`ExportRecord`] fixture.
pub fn export_record(taint: MemoryTaint) -> ExportRecord {
    ExportRecord {
        kind: "entry".into(),
        id: "r1".into(),
        namespace: Some("ns".into()),
        taint,
        payload: serde_json::Value::Null,
    }
}

/// A guard over a fresh recording provider, plus a handle on that provider.
pub fn guarded(
    policy: super::super::GuardPolicy,
) -> (Arc<RecordingProvider>, super::super::MemoryGuard) {
    guarded_with(RecordingProvider::new(), policy)
}

/// As [`guarded`], over a caller-configured provider.
pub fn guarded_with(
    provider: RecordingProvider,
    policy: super::super::GuardPolicy,
) -> (Arc<RecordingProvider>, super::super::MemoryGuard) {
    let provider = Arc::new(provider);
    let guard = super::super::MemoryGuard::new(
        Arc::clone(&provider) as Arc<dyn MemoryProvider>,
        Arc::new(policy),
    );
    (provider, guard)
}

/// A [`MemoryEntry`] fixture.
pub fn entry(content: &str) -> MemoryEntry {
    MemoryEntry {
        id: "id".into(),
        key: "key".into(),
        content: content.into(),
        namespace: Some("ns".into()),
        category: MemoryCategory::Core,
        timestamp: "2026-01-01T00:00:00Z".into(),
        session_id: None,
        score: None,
        taint: MemoryTaint::Internal,
    }
}

pub fn document(content: &str, taint: MemoryTaint) -> NamespaceDocumentInput {
    NamespaceDocumentInput {
        namespace: "ns".into(),
        key: "k".into(),
        title: "t".into(),
        content: content.into(),
        source_type: "chat".into(),
        priority: "normal".into(),
        tags: vec![],
        metadata: serde_json::Value::Null,
        category: "core".into(),
        session_id: None,
        document_id: None,
        taint,
    }
}

impl RecordingProvider {
    /// A provider with an empty call log and every seeded answer at its
    /// default — the shape most tests want before layering a `with_*` on top.
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            recall_result: Mutex::new(Vec::new()),
            fast_retrieve_result: Mutex::new(RetrievalResponse::default()),
            namespace_hits: Mutex::new(Vec::new()),
            namespace_summaries: Mutex::new(Vec::new()),
            session_turns: Mutex::new(Vec::new()),
            pending_segments: Mutex::new(Vec::new()),
        }
    }

    /// Seed what `recall` answers, so a budget test can drive a known set.
    pub fn with_recall_result(self, entries: Vec<MemoryEntry>) -> Self {
        *self.recall_result.lock().unwrap() = entries;
        self
    }

    /// Seed what `fast_retrieve` answers, so the auto-recall lane can run
    /// through a real guard with known hits.
    pub fn with_fast_retrieve_result(self, response: RetrievalResponse) -> Self {
        *self.fast_retrieve_result.lock().unwrap() = response;
        self
    }

    /// Seed what `recall_namespace_scored` answers, so the vector-floored
    /// paths can be driven with known scores.
    pub fn with_namespace_hits(self, hits: Vec<NamespaceMemoryHit>) -> Self {
        *self.namespace_hits.lock().unwrap() = hits;
        self
    }

    /// Seed what `namespaces` answers, so a namespace can look populated
    /// without a real store behind it.
    pub fn with_namespace_summaries(self, summaries: Vec<NamespaceSummary>) -> Self {
        *self.namespace_summaries.lock().unwrap() = summaries;
        self
    }

    /// Seed what `session_turns` answers. Without this the archivist's
    /// finalize path stops at its empty-entries early return and never
    /// reaches the recap it is being tested for.
    pub fn with_session_turns(self, turns: Vec<EpisodicTurn>) -> Self {
        *self.session_turns.lock().unwrap() = turns;
        self
    }

    /// Seed the re-summarisation queue (#6186). The default is empty, so a
    /// test that does not set this drives the pass over nothing — which is
    /// the state a healthy store is in.
    pub fn with_pending_segments(self, segments: Vec<ConversationSegment>) -> Self {
        *self.pending_segments.lock().unwrap() = segments;
        self
    }

    /// Append one call to the log. Every family impl funnels through this.
    pub(super) fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    /// Every call the driver saw, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    /// How many calls reached the driver — for tests that only care that a
    /// path was or was not taken.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// The single recorded call, panicking when there is not exactly one.
    pub fn only_call(&self) -> Call {
        let calls = self.calls();
        assert_eq!(
            calls.len(),
            1,
            "expected exactly one driver call: {calls:?}"
        );
        calls.into_iter().next().unwrap()
    }
}

pub fn namespace_summary(namespace: &str, count: usize) -> NamespaceSummary {
    NamespaceSummary {
        namespace: namespace.into(),
        count,
        last_updated: None,
    }
}

/// A [`NamespaceMemoryHit`] with only the vector component set — the signal the
/// vector-floored recall paths (Lane B, the contradiction check) filter on.
pub fn namespace_hit(
    namespace: &str,
    key: &str,
    content: &str,
    vector_similarity: f64,
) -> NamespaceMemoryHit {
    NamespaceMemoryHit {
        id: format!("{namespace}/{key}"),
        kind: crate::memory::api::types::MemoryItemKind::Kv,
        namespace: namespace.into(),
        key: key.into(),
        title: None,
        content: content.into(),
        category: "core".into(),
        source_type: None,
        updated_at: 0.0,
        score: vector_similarity,
        score_breakdown: crate::memory::api::types::RetrievalScoreBreakdown {
            vector_similarity,
            ..Default::default()
        },
        document_id: None,
        chunk_id: None,
        supporting_relations: Vec::new(),
        taint: MemoryTaint::default(),
    }
}
