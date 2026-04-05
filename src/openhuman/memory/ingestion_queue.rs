//! Background ingestion queue — processes documents through the entity/relation
//! extraction pipeline on a dedicated worker thread so that `doc_put` callers
//! never block on the GLiNER model.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::openhuman::memory::ingestion::{MemoryIngestionConfig, MemoryIngestionRequest};
use crate::openhuman::memory::store::{NamespaceDocumentInput, UnifiedMemory};

/// A job submitted to the ingestion worker.
#[derive(Debug, Clone)]
pub struct IngestionJob {
    pub document: NamespaceDocumentInput,
    pub config: MemoryIngestionConfig,
}

/// Handle used by callers to submit ingestion jobs.
#[derive(Clone)]
pub struct IngestionQueue {
    tx: mpsc::UnboundedSender<IngestionJob>,
}

impl IngestionQueue {
    /// Submit a document for background ingestion. Returns immediately.
    ///
    /// If the worker has been shut down the job is silently dropped (logged).
    pub fn submit(&self, job: IngestionJob) {
        if let Err(e) = self.tx.send(job) {
            log::warn!(
                "[memory:ingestion_queue] failed to enqueue job (worker gone?): {}",
                e.0.document.title,
            );
        }
    }
}

/// Start the background ingestion worker.
///
/// Returns an [`IngestionQueue`] handle that can be cloned and shared with
/// any number of producers. The worker runs on a dedicated tokio task,
/// processing jobs sequentially so the GLiNER model is never loaded in
/// parallel (it is heavyweight).
pub fn start_worker(memory: Arc<UnifiedMemory>) -> IngestionQueue {
    let (tx, rx) = mpsc::unbounded_channel::<IngestionJob>();

    tokio::spawn(ingestion_worker(memory, rx));

    log::info!("[memory:ingestion_queue] background worker started");
    IngestionQueue { tx }
}

async fn ingestion_worker(
    memory: Arc<UnifiedMemory>,
    mut rx: mpsc::UnboundedReceiver<IngestionJob>,
) {
    log::debug!("[memory:ingestion_queue] worker loop entered");

    while let Some(job) = rx.recv().await {
        let title = job.document.title.clone();
        let namespace = job.document.namespace.clone();

        log::debug!(
            "[memory:ingestion_queue] processing job: namespace={namespace}, title={title}",
        );

        let request = MemoryIngestionRequest {
            document: job.document,
            config: job.config,
        };

        match memory.ingest_document(request).await {
            Ok(result) => {
                log::info!(
                    "[memory:ingestion_queue] ingested namespace={namespace} title={title} \
                     — entities={}, relations={}, chunks={}",
                    result.entity_count,
                    result.relation_count,
                    result.chunk_count,
                );
            }
            Err(e) => {
                log::error!(
                    "[memory:ingestion_queue] ingestion failed namespace={namespace} \
                     title={title}: {e}",
                );
            }
        }
    }

    log::info!("[memory:ingestion_queue] worker shut down (channel closed)");
}
