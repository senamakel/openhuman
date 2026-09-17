//! `MemoryDocumentIngest`, `MemoryConversationIngest`, `MemoryLearningIngest`,
//! `MemoryEventIngest`, and `MemoryAnswer` forwarding for
//! [`ModuleMemoryProvider`].

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::learning::LearningCandidate;
use tinymemory_api::provider::operations::{
    AnswerRequest, AnswerResponse, MemoryAnswer, MemoryConversationIngest, MemoryDocumentIngest,
    MemoryEventIngest, MemoryLearningIngest, RawMemoryEvent,
};
use tinymemory_api::provider::types::{IngestItem, IngestOutcome};
use tinymemory_bus::names::methods;

use super::provider::{module_call, module_call_slow, ModuleMemoryProvider};

#[async_trait]
impl MemoryDocumentIngest for ModuleMemoryProvider {
    async fn ingest_document(&self, document: IngestItem) -> Result<IngestOutcome, MemoryError> {
        module_call!(
            self,
            "typed_ingest_document",
            methods::INGEST_DOCUMENT,
            (document,)
        )
    }
}

#[async_trait]
impl MemoryConversationIngest for ModuleMemoryProvider {
    async fn ingest_conversation(
        &self,
        messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError> {
        // The wire member is the chat batch: the conversation trait is the
        // typed rename of the same ordered-batch operation — and it rides the
        // bulk deadline for the same reason ingest_chat does (review finding:
        // a large batch on the default 30s deadline is the AcceptSourceItems
        // timeout all over again).
        module_call_slow!(
            self,
            "typed_ingest_conversation",
            methods::INGEST_CHAT,
            (messages,)
        )
    }
}

#[async_trait]
impl MemoryLearningIngest for ModuleMemoryProvider {
    async fn ingest_learning(
        &self,
        learning: LearningCandidate,
    ) -> Result<IngestOutcome, MemoryError> {
        module_call!(
            self,
            "typed_ingest_learning",
            methods::INGEST_LEARNING,
            (learning,)
        )
    }
}

#[async_trait]
impl MemoryEventIngest for ModuleMemoryProvider {
    async fn ingest_event(&self, event: RawMemoryEvent) -> Result<IngestOutcome, MemoryError> {
        module_call!(self, "typed_ingest_event", methods::INGEST_EVENT, (event,))
    }
}

#[async_trait]
impl MemoryAnswer for ModuleMemoryProvider {
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, MemoryError> {
        module_call!(self, "answer", methods::ANSWER, (request,))
    }
}
