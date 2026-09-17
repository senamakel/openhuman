//! `RecordingProvider`'s `Episodic`, `Profile`, `Chunks`, `Retrieval`,
//! `People`, `Scoring`, `Answer`, and typed-ingestion (`DocumentIngest`,
//! `ConversationIngest`, `LearningIngest`, `EventIngest`) family
//! implementations — the read/query half plus the v1.13.7 ingestion round.
//!
//! Split out of the original `test_support.rs`; see [`super::fixtures`] for
//! `RecordingProvider`'s type and constructors. The write/tree/graph
//! families live in [`super::provider_and_sync_impls`] and
//! [`super::core_and_docs_impls`].

use crate::memory::api::chunks::Chunk;
use crate::memory::api::error::MemoryError;
use crate::memory::api::provider::chunks::{ChunkDetail, ChunkEmbedding, ChunkQuery};
use crate::memory::api::provider::operations::{
    MemoryAnswer, MemoryConversationIngest, MemoryDocumentIngest, MemoryEventIngest,
    MemoryLearningIngest,
};
use crate::memory::api::provider::people::{
    AddressBookSeedOutcome, PersonHandle, PersonInteraction, PersonRecord, PersonScore,
    RankedPerson, ResolvedPerson,
};
use crate::memory::api::provider::profile::{FacetType, ProfileFacet, UserState};
use crate::memory::api::provider::retrieval::{
    CoverWindowQuery, EntityMatch, FastRetrieveQuery, RetrievalHit, RetrievalResponse,
    SourceRetrievalQuery,
};
use crate::memory::api::provider::types::SourceScope;
use crate::memory::api::provider::types::{IngestItem, IngestOutcome};
use crate::memory::api::provider::{
    EpisodicEvent, MemoryChunks, MemoryEpisodic, MemoryPeople, MemoryProfile, MemoryRetrieval,
    MemoryScoring,
};
use crate::memory::api::types::NamespaceMemoryHit;
use async_trait::async_trait;

use super::fixtures::{rendered_scope, Call, RecordingProvider};

#[async_trait]
impl MemoryEpisodic for RecordingProvider {
    async fn insert_turn(
        &self,
        turn: &crate::memory::api::provider::episodic::EpisodicTurn,
    ) -> Result<i64, MemoryError> {
        // Records the turn text, so a guard that failed to redact one would be
        // visible here rather than only in a live store.
        self.record(Call {
            method: "episodic.insert_turn".into(),
            content: Some(turn.content.clone()),
            taint: None,
            scoped: None,
        });
        Ok(1)
    }

    async fn session_turns(
        &self,
        _session_id: &str,
    ) -> Result<Vec<crate::memory::api::provider::episodic::EpisodicTurn>, MemoryError> {
        self.record(Call::plain("episodic.session_turns"));
        Ok(self.session_turns.lock().unwrap().clone())
    }

    async fn open_segment(
        &self,
        _session_id: &str,
    ) -> Result<Option<crate::memory::api::provider::episodic::ConversationSegment>, MemoryError>
    {
        self.record(Call::plain("episodic.open_segment"));
        Ok(None)
    }

    async fn segments_pending_summary(
        &self,
        limit: u32,
    ) -> Result<Vec<crate::memory::api::provider::episodic::ConversationSegment>, MemoryError> {
        self.record(Call::plain("episodic.segments_pending_summary"));
        // Honour `limit` — a fake that ignored it would let a test drive more
        // segments than the caller asked for and hide a bounded-recovery bug.
        Ok(self
            .pending_segments
            .lock()
            .unwrap()
            .iter()
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn create_segment(
        &self,
        _segment_id: &str,
        _session_id: &str,
        _namespace: &str,
        _start_episodic_id: i64,
        _start_seq: Option<u32>,
        _start_timestamp: f64,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.create_segment"));
        Ok(())
    }

    async fn append_turn(
        &self,
        _segment_id: &str,
        _episodic_id: i64,
        _seq: Option<u32>,
        _timestamp: f64,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.append_turn"));
        Ok(())
    }

    async fn close_segment(&self, _segment_id: &str, _now: f64) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.close_segment"));
        Ok(())
    }

    async fn insert_event(&self, event: &EpisodicEvent) -> Result<(), MemoryError> {
        // Records the event text for the same reason `insert_turn` does: a guard
        // that stopped redacting one would otherwise be invisible to every test,
        // and the redaction on this path has already been missing once.
        self.record(Call {
            method: "episodic.insert_event".into(),
            content: Some(event.content.clone()),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn set_segment_summary(
        &self,
        _segment_id: &str,
        summary: &str,
        _now: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call {
            method: "episodic.set_segment_summary".into(),
            content: Some(summary.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(())
    }

    async fn upsert_segment_embedding(
        &self,
        _segment_id: &str,
        _model_signature: &str,
        _embedding: &[f32],
        _created_at: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("episodic.upsert_segment_embedding"));
        Ok(())
    }
}
#[async_trait]
impl MemoryProfile for RecordingProvider {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.list_active_facets"));
        Ok(vec![])
    }
    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.list_all_facets"));
        Ok(vec![])
    }
    async fn get_facet(&self, _key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.get_facet"));
        Ok(None)
    }
    async fn facets_by_type(
        &self,
        _facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        self.record(Call::plain("profile.facets_by_type"));
        Ok(vec![])
    }
    async fn upsert_facet(&self, _facet: &ProfileFacet) -> Result<(), MemoryError> {
        self.record(Call::plain("profile.upsert_facet"));
        Ok(())
    }
    async fn upsert_provider_facet(
        &self,
        _facet_id: &str,
        _facet_type: FacetType,
        _key: &str,
        _value: &str,
        _confidence: f64,
        _segment_id: Option<&str>,
        _observed_at: f64,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("profile.upsert_provider_facet"));
        Ok(())
    }
    async fn set_facet_user_state(
        &self,
        _key: &str,
        _user_state: UserState,
    ) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.set_facet_user_state"));
        Ok(false)
    }
    async fn delete_facet(&self, _key: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.delete_facet"));
        Ok(false)
    }
    async fn delete_facet_by_id(&self, _facet_id: &str) -> Result<bool, MemoryError> {
        self.record(Call::plain("profile.delete_facet_by_id"));
        Ok(false)
    }
    async fn drop_facets_below(&self, _threshold: f64) -> Result<usize, MemoryError> {
        self.record(Call::plain("profile.drop_facets_below"));
        Ok(0)
    }
    async fn workflow_identity_matches(&self, _pattern: &str, _value: &str) -> bool {
        self.record(Call::plain("profile.workflow_identity_matches"));
        false
    }
}

#[async_trait]
impl MemoryChunks for RecordingProvider {
    async fn list_chunks(
        &self,
        _query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        self.record(Call {
            method: "chunks.list_chunks".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn get_chunk(&self, _chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        self.record(Call::plain("chunks.get_chunk"));
        Ok(None)
    }

    async fn chunk_detail(&self, _chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        self.record(Call::plain("chunks.chunk_detail"));
        Ok(None)
    }

    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        self.record(Call::plain("chunks.storage_kinds"));
        Ok(vec![])
    }

    async fn chunk_embeddings(
        &self,
        _chunk_ids: &[String],
        _model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        self.record(Call::plain("chunks.chunk_embeddings"));
        Ok(vec![])
    }

    async fn chunk_score(
        &self,
        _chunk_id: &str,
    ) -> Result<Option<crate::memory::api::provider::chunks::ChunkScore>, MemoryError> {
        self.record(Call::plain("chunks.chunk_score"));
        Ok(None)
    }

    async fn source_ingest_status(
        &self,
        _source_prefixes: &[crate::memory::api::provider::chunks::SourceIngestQuery],
    ) -> Result<Vec<crate::memory::api::provider::chunks::SourceIngestStatus>, MemoryError> {
        self.record(Call::plain("chunks.source_ingest_status"));
        Ok(vec![])
    }
}

#[async_trait]
impl MemoryRetrieval for RecordingProvider {
    async fn fast_retrieve(
        &self,
        _query: &str,
        _options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.fast_retrieve".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(self.fast_retrieve_result.lock().unwrap().clone())
    }

    async fn cover_window(
        &self,
        _window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.cover_window".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(RetrievalResponse::default())
    }

    async fn retrieve_source(
        &self,
        _query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_source".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(RetrievalResponse::default())
    }

    async fn retrieve_children(
        &self,
        _node_id: &str,
        _max_depth: u32,
        _query: Option<&str>,
        _limit: Option<usize>,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_children".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn retrieve_leaves(
        &self,
        _chunk_ids: &[String],
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        self.record(Call {
            method: "retrieval.retrieve_leaves".into(),
            content: rendered_scope(scope),
            taint: None,
            scoped: Some(scope.is_some()),
        });
        Ok(vec![])
    }

    async fn recall_namespace_scored(
        &self,
        namespace: &str,
        _query: &str,
        limit: usize,
        _exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        // Honours the two request parameters a caller can get wrong — the
        // namespace it asks for and the page it accepts — and records them,
        // so a test can assert both rather than only the content it got.
        self.record(Call {
            method: "retrieval.recall_namespace_scored".into(),
            content: Some(format!("namespace={namespace} limit={limit}")),
            taint: None,
            scoped: None,
        });
        Ok(self
            .namespace_hits
            .lock()
            .unwrap()
            .iter()
            .filter(|hit| hit.namespace == namespace)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn recall_namespace_recent(
        &self,
        _namespace: &str,
        _limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        self.record(Call::plain("retrieval.recall_namespace_recent"));
        Ok(vec![])
    }

    async fn search_entities(
        &self,
        _query: &str,
        _kinds: Option<&[String]>,
        _limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        self.record(Call::plain("retrieval.search_entities"));
        Ok(vec![])
    }
}

#[async_trait]
impl MemoryPeople for RecordingProvider {
    async fn list_people(&self, _limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        self.record(Call::plain("people.list_people"));
        Ok(vec![])
    }

    async fn get_person(&self, _person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        self.record(Call::plain("people.get_person"));
        Ok(None)
    }

    async fn resolve_handle(
        &self,
        _handle: &PersonHandle,
        _create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        self.record(Call::plain("people.resolve_handle"));
        Ok(None)
    }

    async fn add_handle_alias(
        &self,
        _person_id: &str,
        _handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("people.add_handle_alias"));
        Ok(())
    }

    async fn score_person(&self, _person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        self.record(Call::plain("people.score_person"));
        Ok(None)
    }

    async fn record_interaction(
        &self,
        _interaction: &PersonInteraction,
    ) -> Result<(), MemoryError> {
        self.record(Call::plain("people.record_interaction"));
        Ok(())
    }

    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        self.record(Call::plain("people.seed_from_address_book"));
        Ok(AddressBookSeedOutcome::default())
    }
}

#[async_trait]
impl MemoryScoring for RecordingProvider {
    async fn extract_entities(&self, query: &str) -> Result<Vec<String>, MemoryError> {
        self.record(Call {
            method: "scoring.extract_entities".into(),
            content: Some(query.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(Vec::new())
    }

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        self.record(Call {
            method: "scoring.embed_text".into(),
            content: Some(text.to_string()),
            taint: None,
            scoped: None,
        });
        Ok(Vec::new())
    }

    async fn embedder_slug(&self) -> Result<String, MemoryError> {
        self.record(Call::plain("scoring.embedder_slug"));
        Ok(String::new())
    }
}

// ── The v1.13.7 typed-ingestion round + Answer ──────────────────────────────
// Same contract as every family above: `capabilities()` answers all(), so the
// audit demands a live accessor and a recording impl for each.

#[async_trait]
impl MemoryDocumentIngest for RecordingProvider {
    async fn ingest_document(&self, document: IngestItem) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "document_ingest.ingest_document".into(),
            content: Some(document.content),
            taint: Some(document.taint),
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }
}

#[async_trait]
impl MemoryConversationIngest for RecordingProvider {
    async fn ingest_conversation(
        &self,
        messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError> {
        for message in messages {
            self.record(Call {
                method: "conversation_ingest.ingest_conversation".into(),
                content: Some(message.content),
                taint: Some(message.taint),
                scoped: None,
            });
        }
        Ok(IngestOutcome::default())
    }
}

#[async_trait]
impl MemoryLearningIngest for RecordingProvider {
    async fn ingest_learning(
        &self,
        _learning: tinymemory_api::learning::LearningCandidate,
    ) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "learning_ingest.ingest_learning".into(),
            content: None,
            taint: None,
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }
}

#[async_trait]
impl MemoryEventIngest for RecordingProvider {
    async fn ingest_event(
        &self,
        _event: crate::memory::api::provider::operations::RawMemoryEvent,
    ) -> Result<IngestOutcome, MemoryError> {
        self.record(Call {
            method: "event_ingest.ingest_event".into(),
            content: None,
            taint: None,
            scoped: None,
        });
        Ok(IngestOutcome::default())
    }
}

#[async_trait]
impl MemoryAnswer for RecordingProvider {
    async fn answer(
        &self,
        _request: crate::memory::api::provider::operations::AnswerRequest,
    ) -> Result<crate::memory::api::provider::operations::AnswerResponse, MemoryError> {
        self.record(Call {
            method: "answer.answer".into(),
            content: None,
            taint: None,
            scoped: None,
        });
        Ok(crate::memory::api::provider::operations::AnswerResponse {
            answer: String::new(),
            model: None,
            citations: Vec::new(),
            steps: Vec::new(),
        })
    }
}
