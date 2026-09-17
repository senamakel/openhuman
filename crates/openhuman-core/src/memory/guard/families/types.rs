//! The shared decorator scaffolding for every guarded family: the
//! `decorator!` macro that declares one struct's two fields, constructor,
//! and `family()` re-derivation, plus every family's invocation of it.
//!
//! Split out of `families.rs` (see that module's doc comment for why each
//! family gets its own decorator rather than the guard forwarding a raw
//! borrow). The trait implementations that make each of these types actually
//! guard something live in the sibling `*_impls`-style files under
//! [`super`].

use std::sync::Arc;

use crate::memory::api::capabilities::Capability;
use crate::memory::api::error::MemoryError;
use crate::memory::api::provider::chunks::MemoryChunks;
use crate::memory::api::provider::episodic::MemoryEpisodic;
use crate::memory::api::provider::operations::{
    MemoryAnswer, MemoryConversationIngest, MemoryDocumentIngest, MemoryEventIngest,
    MemoryLearningIngest,
};
use crate::memory::api::provider::people::MemoryPeople;
use crate::memory::api::provider::profile::MemoryProfile;
use crate::memory::api::provider::retrieval::MemoryRetrieval;
use crate::memory::api::provider::scoring::MemoryScoring;
use crate::memory::api::provider::sessions::MemoryCodingSessions;
use crate::memory::api::provider::sync::MemorySourceSync;
use crate::memory::api::provider::{
    MemoryDiff, MemoryDocuments, MemoryEntities, MemoryGoals, MemoryGraph, MemoryIngest,
    MemoryMaintenance, MemoryProvider, MemorySourceSink, MemoryToolMemory, MemoryTree,
};

use super::super::policy::GuardPolicy;

macro_rules! decorator {
    ($(#[$meta:meta])* $name:ident, $fam:ty, $accessor:ident, $cap:ident) => {
        $(#[$meta])*
        pub struct $name {
            inner: Arc<dyn MemoryProvider>,
            pub(super) policy: Arc<GuardPolicy>,
        }

        impl $name {
            pub(crate) fn new(inner: Arc<dyn MemoryProvider>, policy: Arc<GuardPolicy>) -> Self {
                Self { inner, policy }
            }

            /// The underlying family handle.
            ///
            /// The `Err` arm is **structurally unreachable**: `MemoryGuard::new`
            /// only builds this decorator when the inner provider answered
            /// `provides(Capability::$cap)`, and the contract documents the
            /// capability set as fixed at bind time. It is written as a real
            /// error rather than `.expect(...)` because a panic inside a memory
            /// call is a strictly worse failure than an `Unsupported` a caller
            /// can already handle.
            pub(super) fn family(&self) -> Result<&$fam, MemoryError> {
                self.inner
                    .$accessor()
                    .ok_or_else(|| MemoryError::unsupported(Capability::$cap))
            }
        }
    };
}

decorator!(
    /// Guarded [`MemoryIngest`].
    GuardedIngest,
    dyn MemoryIngest,
    as_ingest,
    Ingest
);
decorator!(
    /// Guarded [`MemoryDocuments`].
    GuardedDocuments,
    dyn MemoryDocuments,
    as_documents,
    Documents
);
decorator!(
    /// Guarded [`MemoryTree`] — the one family that carries step 2.
    GuardedTree,
    dyn MemoryTree,
    as_tree,
    Tree
);
decorator!(
    /// Guarded [`MemoryEntities`].
    GuardedEntities,
    dyn MemoryEntities,
    as_entities,
    Entities
);
decorator!(
    /// Guarded [`MemoryGraph`].
    GuardedGraph,
    dyn MemoryGraph,
    as_graph,
    Graph
);
decorator!(
    /// Guarded [`MemoryDiff`].
    GuardedDiff,
    dyn MemoryDiff,
    as_diff,
    Diff
);
decorator!(
    /// Guarded [`MemoryGoals`].
    GuardedGoals,
    dyn MemoryGoals,
    as_goals,
    Goals
);
decorator!(
    /// Guarded [`MemoryToolMemory`].
    GuardedToolMemory,
    dyn MemoryToolMemory,
    as_tool_memory,
    ToolMemory
);
decorator!(
    /// Guarded [`MemorySourceSink`].
    GuardedSources,
    dyn MemorySourceSink,
    as_sources,
    Sources
);
decorator!(
    /// Guarded [`MemoryMaintenance`].
    GuardedMaintenance,
    dyn MemoryMaintenance,
    as_maintenance,
    Maintenance
);
decorator!(
    /// Guarded [`MemoryPeople`].
    GuardedPeople,
    dyn MemoryPeople,
    as_people,
    People
);
decorator!(
    /// Guarded [`MemoryChunks`].
    GuardedChunks,
    dyn MemoryChunks,
    as_chunks,
    Chunks
);
decorator!(
    /// Guarded [`MemoryRetrieval`].
    GuardedRetrieval,
    dyn MemoryRetrieval,
    as_retrieval,
    Retrieval
);
decorator!(
    /// Guarded [`MemoryEpisodic`].
    GuardedEpisodic,
    dyn MemoryEpisodic,
    as_episodic,
    Episodic
);
decorator!(
    /// Guarded [`MemorySourceSync`].
    GuardedSourceSync,
    dyn MemorySourceSync,
    as_source_sync,
    SourceSync
);
decorator!(
    /// Guarded [`MemoryCodingSessions`].
    GuardedCodingSessions,
    dyn MemoryCodingSessions,
    as_coding_sessions,
    CodingSessions
);
decorator!(
    /// Guarded [`MemoryProfile`].
    GuardedProfile,
    dyn MemoryProfile,
    as_profile,
    Profile
);
decorator!(
    /// Guarded [`MemoryScoring`].
    GuardedScoring,
    dyn MemoryScoring,
    as_scoring,
    Scoring
);

decorator!(
    /// Guarded [`MemoryDocumentIngest`].
    GuardedDocumentIngest,
    dyn MemoryDocumentIngest,
    as_document_ingest,
    DocumentIngest
);
decorator!(
    /// Guarded [`MemoryConversationIngest`].
    GuardedConversationIngest,
    dyn MemoryConversationIngest,
    as_conversation_ingest,
    ConversationIngest
);
decorator!(
    /// Guarded [`MemoryLearningIngest`].
    GuardedLearningIngest,
    dyn MemoryLearningIngest,
    as_learning_ingest,
    LearningIngest
);
decorator!(
    /// Guarded [`MemoryEventIngest`].
    GuardedEventIngest,
    dyn MemoryEventIngest,
    as_event_ingest,
    EventIngest
);
decorator!(
    /// Guarded [`MemoryAnswer`].
    GuardedAnswer,
    dyn MemoryAnswer,
    as_answer,
    Answer
);
