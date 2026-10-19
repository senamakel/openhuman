//! Regression for generated-media blobs using the calling tenant's storage.
//!
//! This target runs in its own process because it installs a backend in the
//! process-wide storage slot. A spy reads each staged write through a second
//! scoped handle before the media artifact adapter removes the staging blob.

#![cfg(feature = "media")]

use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tinyagents_harness::tinyinference_image::MockImageGenerator;
use tinyagents_harness::tinyinference_video::{MockVideoGenerator, MockVideoScript, WaitPolicy};
use tinystoragedrivers_core::{
    Blob, BlobMeta, BlobStore, Capabilities, MemoryStorage, Result, Scope, ScopedStorage,
    StorageBackend, async_trait,
};
use tinytools::{Tool, ToolContent};

use openhuman_core::media::generation::{MediaGenerators, media_tools_from};

#[tokio::test]
async fn media_factory_stages_generated_output_in_the_scoped_blob_store() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let files = root.path().join("Files");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let _storage = ObservingStorageGuard(openhuman_core::storage::install(Arc::new(
        ObservingStorage {
            inner: MemoryStorage::new(),
            observed: Arc::clone(&observed),
        },
    )));
    let tools = media_tools_from(
        MediaGenerators::new(
            Arc::new(MockImageGenerator::new()),
            Arc::new(MockVideoGenerator::new(MockVideoScript::delivers())),
        ),
        root.path(),
        &workspace,
        &files,
        WaitPolicy::new(
            std::time::Duration::from_millis(1),
            std::time::Duration::from_secs(5),
        ),
    );

    let result = tools
        .iter()
        .find(|tool| tool.name() == "media_generate_image")
        .expect("image generation is available for a scoped caller")
        .execute(json!({ "prompt": "a test image" }))
        .await
        .unwrap();
    assert!(!result.is_error, "{result:?}");
    let payload = result
        .content
        .iter()
        .find_map(|block| match block {
            ToolContent::Json { data } => Some(data),
            _ => None,
        })
        .expect("generated media result");
    let artifact = &payload["artifacts"][0];
    assert!(artifact["artifact_id"].is_string(), "{artifact}");
    let output_path = Path::new(artifact["path"].as_str().unwrap());
    assert!(output_path.starts_with(&files));
    let output_bytes = std::fs::read(output_path).unwrap();
    let observed = observed.lock().unwrap();
    assert!(
        observed
            .iter()
            .any(|(_, written, read_back)| written == &output_bytes && read_back == &output_bytes),
        "staged media must be written into the scoped backend and readable through a second handle; observed {} writes",
        observed.len()
    );
}

struct ObservingStorageGuard(Option<Arc<dyn StorageBackend>>);

impl Drop for ObservingStorageGuard {
    fn drop(&mut self) {
        openhuman_core::storage::clear();
        if let Some(previous) = self.0.take() {
            openhuman_core::storage::install(previous);
        }
    }
}

#[derive(Debug)]
struct ObservingStorage {
    inner: MemoryStorage,
    observed: Arc<Mutex<Vec<(String, Vec<u8>, Vec<u8>)>>>,
}

impl StorageBackend for ObservingStorage {
    fn driver(&self) -> &'static str {
        self.inner.driver()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn for_scope(&self, scope: &Scope) -> Result<ScopedStorage> {
        let scoped = self.inner.for_scope(scope)?;
        let peer = Arc::clone(self.inner.for_scope(scope)?.blobs());
        Ok(ScopedStorage::new(
            scope.clone(),
            scoped.driver(),
            Arc::clone(scoped.documents()),
            Arc::clone(scoped.streams()),
            Arc::new(ObservingBlobs {
                inner: Arc::clone(scoped.blobs()),
                peer,
                observed: Arc::clone(&self.observed),
            }),
        ))
    }
}

#[derive(Debug)]
struct ObservingBlobs {
    inner: Arc<dyn BlobStore>,
    peer: Arc<dyn BlobStore>,
    observed: Arc<Mutex<Vec<(String, Vec<u8>, Vec<u8>)>>>,
}

#[async_trait]
impl BlobStore for ObservingBlobs {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: Option<&str>) -> Result<BlobMeta> {
        let meta = self.inner.put(key, bytes.clone(), content_type).await?;
        if let Some(blob) = self.peer.get(key).await? {
            self.observed
                .lock()
                .unwrap()
                .push((key.to_owned(), bytes, blob.bytes));
        }
        Ok(meta)
    }

    async fn get(&self, key: &str) -> Result<Option<Blob>> {
        self.inner.get(key).await
    }

    async fn get_range(&self, key: &str, range: Range<u64>) -> Result<Option<Vec<u8>>> {
        self.inner.get_range(key, range).await
    }

    async fn head(&self, key: &str) -> Result<Option<BlobMeta>> {
        self.inner.head(key).await
    }

    async fn delete(&self, key: &str) -> Result<bool> {
        self.inner.delete(key).await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<BlobMeta>> {
        self.inner.list(prefix).await
    }
}
