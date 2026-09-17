//! The ten optional-family decorators — the load-bearing half of the guard.
//!
//! ## Why these exist at all
//!
//! [`MemoryProvider::as_tree`] and its nine siblings return a **borrow** of a
//! family trait object. If [`MemoryGuard`]'s override simply forwarded
//! `self.inner.as_tree()`, every caller that reached memory through a family
//! accessor would hold a raw, unguarded driver handle — and the guard's whole
//! reason to exist ("the only handle product code receives") would be
//! bypassable by one method call. Nine of the thirteen families are *only*
//! reachable that way.
//!
//! So each family gets its own decorator, and the accessor hands back a borrow
//! of that. Because the accessor returns a reference, the decorators cannot be
//! constructed on demand inside it — a reference to a temporary does not
//! outlive the call — so they are **fields on the guard, built once at
//! construction**. That is also what makes their presence mirror the inner
//! driver's exactly: a field exists iff `inner.provides(...)` said so, which is
//! what keeps `audit_provider` happy.
//!
//! ## Why each decorator holds the provider, not the family
//!
//! A `GuardedTree { inner: &dyn MemoryTree }` borrowed out of an
//! `Arc<dyn MemoryProvider>` the same struct owns is self-referential, and Rust
//! has no way to express that without unsafe pinning. Holding
//! `Arc<dyn MemoryProvider>` and re-deriving the family per call sidesteps it
//! entirely, at the cost of one `Option` unwrap that is structurally
//! unreachable — see [`family`](GuardedTree::family).
//!
//! [`MemoryGuard`]: super::MemoryGuard

#[cfg(test)]
#[path = "families_tests.rs"]
mod tests;

mod types;

mod graph_and_bookkeeping;
mod ingest_and_tree;
mod retrieval_and_profile;
mod typed_ingest_and_answer;

pub use types::{
    GuardedAnswer, GuardedChunks, GuardedCodingSessions, GuardedConversationIngest, GuardedDiff,
    GuardedDocumentIngest, GuardedDocuments, GuardedEntities, GuardedEpisodic, GuardedEventIngest,
    GuardedGoals, GuardedGraph, GuardedIngest, GuardedLearningIngest, GuardedMaintenance,
    GuardedPeople, GuardedProfile, GuardedRetrieval, GuardedScoring, GuardedSourceSync,
    GuardedSources, GuardedToolMemory, GuardedTree,
};
