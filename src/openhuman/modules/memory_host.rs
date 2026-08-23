//! Host-owned callbacks used by the separately compiled TinyMemory module.

use crate::core::bus::BUS;
use crate::openhuman::config::Config;
use std::sync::Arc;
use tinyagents::harness::model::{ModelRequest, ModelResponse};
use tinybus::ObjectPath;
use tinymemory_api::host::{MemoryEvent, SpacyResponse};

const EMBEDDING_NAME: &str = "ai.tinyhumans.tinymemory.EmbeddingHost";
const EMBEDDING_PATH: &str = "/ai/tinyhumans/tinymemory/EmbeddingHost";
const CHAT_NAME: &str = "ai.tinyhumans.tinymemory.ChatHost";
const CHAT_PATH: &str = "/ai/tinyhumans/tinymemory/ChatHost";
const RUNTIME_NAME: &str = "ai.tinyhumans.tinymemory.RuntimeHost";
const RUNTIME_PATH: &str = "/ai/tinyhumans/tinymemory/RuntimeHost";

#[derive(Clone)]
struct EmbeddingCallbacks(Arc<Config>);

#[tinybus::interface(name = "ai.tinyhumans.tinymemory.EmbeddingHost")]
impl EmbeddingCallbacks {
    async fn embed(
        &self,
        provider: String,
        model: String,
        dimensions: usize,
        texts: Vec<String>,
    ) -> tinybus::Result<Vec<Vec<f32>>> {
        let api_key = crate::openhuman::inference::embeddings::resolve_api_key(&self.0, &provider);
        let endpoint = self
            .0
            .cloud_providers
            .iter()
            .find(|candidate| candidate.slug == provider)
            .map(|candidate| candidate.endpoint.as_str())
            .filter(|endpoint| !endpoint.is_empty());
        let embedder =
            crate::openhuman::inference::embeddings::create_embedding_provider_with_config(
                &self.0, &provider, &model, dimensions, &api_key, endpoint,
            )
            .map_err(method_error)?;
        let borrowed: Vec<&str> = texts.iter().map(String::as_str).collect();
        embedder.embed(&borrowed).await.map_err(method_error)
    }
}

#[derive(Clone)]
struct ChatCallbacks(Arc<Config>);

#[tinybus::interface(name = "ai.tinyhumans.tinymemory.ChatHost")]
impl ChatCallbacks {
    async fn complete(
        &self,
        role: String,
        request: ModelRequest,
    ) -> tinybus::Result<ModelResponse> {
        let (model, _) = crate::openhuman::inference::provider::create_chat_model_with_model_id(
            &role,
            &self.0,
            self.0.default_temperature,
        )
        .map_err(method_error)?;
        model.invoke(&(), request).await.map_err(method_error)
    }
}

#[derive(Clone)]
struct RuntimeCallbacks(Arc<Config>);

#[tinybus::interface(name = "ai.tinyhumans.tinymemory.RuntimeHost")]
impl RuntimeCallbacks {
    async fn publish_event(&self, event: MemoryEvent) -> tinybus::Result<()> {
        if let Some(event) = into_domain_event(event) {
            BUS.publish(event);
        }
        Ok(())
    }

    async fn report_error(
        &self,
        classify_expected: bool,
        rendered: String,
        domain: String,
        operation: String,
        tags: Vec<(String, String)>,
    ) -> tinybus::Result<()> {
        let borrowed: Vec<(&str, &str)> = tags
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        if classify_expected {
            crate::core::observability::report_error_or_expected(
                &rendered, &domain, &operation, &borrowed,
            );
        } else {
            crate::core::observability::report_error(&rendered, &domain, &operation, &borrowed);
        }
        Ok(())
    }

    async fn extract_spacy(&self, text: String) -> tinybus::Result<SpacyResponse> {
        let response = crate::openhuman::runtime::python_server::extract_spacy(&self.0, &text)
            .await
            .map_err(method_error)?;
        serde_json::from_value(serde_json::to_value(response).map_err(method_error)?)
            .map_err(method_error)
    }
}

fn method_error(error: impl std::fmt::Display) -> tinybus::Error {
    tinybus::Error::MethodFailed {
        name: "ai.tinyhumans.tinymemory.Error.Host".to_string(),
        message: error.to_string(),
    }
}

pub(super) async fn install(
    connection: &tinybus::Connection,
    config: Arc<Config>,
) -> tinybus::Result<()> {
    static INSTALLED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    INSTALLED
        .get_or_try_init(|| async move {
            connection
                .serve_at(
                    ObjectPath::new(EMBEDDING_PATH)?,
                    EmbeddingCallbacks(Arc::clone(&config)),
                )
                .await?;
            connection
                .serve_at(
                    ObjectPath::new(CHAT_PATH)?,
                    ChatCallbacks(Arc::clone(&config)),
                )
                .await?;
            connection
                .serve_at(ObjectPath::new(RUNTIME_PATH)?, RuntimeCallbacks(config))
                .await?;
            connection.request_name(EMBEDDING_NAME).await?;
            connection.request_name(CHAT_NAME).await?;
            connection.request_name(RUNTIME_NAME).await
        })
        .await
        .map(|_| ())
}

fn into_domain_event(event: MemoryEvent) -> Option<crate::core::events::DomainEvent> {
    use crate::core::events::DomainEvent;
    Some(match event {
        MemoryEvent::SyncStageChanged {
            trigger,
            stage,
            provider,
            connection_id,
            detail,
            source_id,
        } => DomainEvent::MemorySyncStageChanged {
            trigger,
            stage,
            provider,
            connection_id,
            detail,
            source_id,
        },
        MemoryEvent::IngestionStarted {
            document_id,
            title,
            namespace,
            queue_depth,
        } => DomainEvent::MemoryIngestionStarted {
            document_id,
            title,
            namespace,
            queue_depth,
        },
        MemoryEvent::IngestionCompleted {
            document_id,
            namespace,
            success,
            elapsed_ms,
            queue_depth,
        } => DomainEvent::MemoryIngestionCompleted {
            document_id,
            namespace,
            success,
            elapsed_ms,
            queue_depth,
        },
        MemoryEvent::DocumentCanonicalized {
            source_id,
            source_kind,
            chunks_written,
            chunk_ids,
            canonicalized_at,
            body_preview,
        } => DomainEvent::DocumentCanonicalized {
            source_id,
            source_kind,
            chunks_written,
            chunk_ids,
            canonicalized_at,
            body_preview,
        },
        MemoryEvent::TreeSummarizerHourCompleted {
            namespace,
            node_id,
            token_count,
        } => DomainEvent::TreeSummarizerHourCompleted {
            namespace,
            node_id,
            token_count,
        },
        MemoryEvent::TreeSummarizerPropagated {
            namespace,
            node_id,
            level,
            token_count,
        } => DomainEvent::TreeSummarizerPropagated {
            namespace,
            node_id,
            level,
            token_count,
        },
        MemoryEvent::TreeSummarizerRebuildCompleted {
            namespace,
            total_nodes,
        } => DomainEvent::TreeSummarizerRebuildCompleted {
            namespace,
            total_nodes,
        },
        MemoryEvent::TreeBuildProgress {
            phase,
            step,
            tree_scope,
            level,
            item_count,
            detail,
        } => DomainEvent::MemoryTreeBuildProgress {
            phase,
            step,
            tree_scope,
            level,
            item_count,
            detail,
        },
        MemoryEvent::EmbeddingModelUnhealthy(reason) => DomainEvent::EmbeddingModelUnhealthy {
            provider: reason.provider,
            model: reason.model,
            fallback_provider: reason.fallback_provider,
            message: reason.message,
        },
        MemoryEvent::DriverBindFailed {
            configured_driver,
            bound_driver,
            reason,
        } => DomainEvent::MemoryDriverBindFailed {
            configured_driver,
            bound_driver,
            reason,
        },
        MemoryEvent::DiffSnapshotTaken {
            snapshot_id,
            source_id,
            source_kind,
            item_count,
            trigger,
        } => DomainEvent::MemoryDiffSnapshotTaken {
            snapshot_id,
            source_id,
            source_kind,
            item_count,
            trigger,
        },
        MemoryEvent::DiffMarkedRead {
            source_ids,
            snapshot_ids,
        } => DomainEvent::MemoryDiffMarkedRead {
            source_ids,
            snapshot_ids,
        },
        MemoryEvent::ComposioIntegrationsChanged { toolkits } => {
            DomainEvent::ComposioIntegrationsChanged { toolkits }
        }
        MemoryEvent::SyncRequested { channel_id } => {
            DomainEvent::MemorySyncRequested { channel_id }
        }
        MemoryEvent::LocalModelUnavailable { origin } => {
            // The user-facing error surface lives in the memory tree's health
            // module. Unreachable with the family off — nothing binds a module
            // driver that could raise this — but the arm must still compile.
            #[cfg(feature = "memory")]
            crate::openhuman::memory::tree::health::user_error::publish_local_model_unavailable_user_error(&origin);
            #[cfg(not(feature = "memory"))]
            let _ = origin;
            return None;
        }
    })
}
