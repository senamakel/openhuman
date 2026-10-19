use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tinyagents_harness::tinyinference_image::MockImageGenerator;
use tinyagents_harness::tinyinference_video::{MockVideoGenerator, MockVideoScript};
use tinystoragedrivers::{
    Blob, BlobMeta, BlobStore, Capabilities, MemoryStorage, Result, Scope, ScopedStorage,
    StorageBackend, async_trait,
};
use tinytools::{PermissionLevel, Tool, ToolCategory};

use super::{
    IMAGE_TOOL_NAME, LIST_MODELS_TOOL_NAME, VIDEO_TOOL_NAME, media_tools_from, reference_policy,
};
use crate::media::generation::MediaGenerators;

fn tools(action_dir: &Path) -> Vec<Box<dyn Tool>> {
    media_tools_from(
        MediaGenerators::new(
            Arc::new(MockImageGenerator::new()),
            Arc::new(MockVideoGenerator::new(MockVideoScript::delivers())),
        ),
        action_dir,
        &action_dir.join("workspace"),
        action_dir.join("Files"),
        tinyagents_harness::tinyinference_video::WaitPolicy::new(
            std::time::Duration::from_millis(1),
            std::time::Duration::from_secs(5),
        ),
    )
}

fn by_name<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> &'a dyn Tool {
    tools
        .iter()
        .find(|tool| tool.name() == name)
        .map(AsRef::as_ref)
        .unwrap_or_else(|| panic!("missing tool {name}"))
}

/// The `media` tool pack and the image/video agents' allowlists pin these
/// names; renaming a tool silently drops it from every agent.
#[test]
fn tool_names_match_the_media_pack() {
    let dir = tempfile::tempdir().unwrap();
    let tools = tools(dir.path());
    let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
    assert_eq!(
        names,
        vec![IMAGE_TOOL_NAME, VIDEO_TOOL_NAME, LIST_MODELS_TOOL_NAME]
    );
    assert_eq!(
        names,
        vec![
            "media_generate_image",
            "media_generate_video",
            "media_list_models"
        ]
    );
}

#[test]
fn generation_tools_keep_their_host_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let tools = tools(dir.path());
    for name in [IMAGE_TOOL_NAME, VIDEO_TOOL_NAME] {
        let tool = by_name(&tools, name);
        assert_eq!(tool.permission_level(), PermissionLevel::Execute, "{name}");
        assert_eq!(tool.category(), ToolCategory::Workflow, "{name}");
        assert!(tool.external_effect(), "{name}");
        assert!(tool.policy().side_effects.payment, "{name} is billed");
    }
    let list = by_name(&tools, LIST_MODELS_TOOL_NAME);
    assert_eq!(list.permission_level(), PermissionLevel::ReadOnly);
}

#[test]
fn schemas_expose_the_reference_standards() {
    let dir = tempfile::tempdir().unwrap();
    let tools = tools(dir.path());
    let image = by_name(&tools, IMAGE_TOOL_NAME).parameters_schema();
    assert_eq!(image["required"], json!(["prompt"]));
    for key in [
        "prompt",
        "model",
        "n",
        "aspect_ratio",
        "resolution",
        "size",
        "seed",
        "references",
    ] {
        assert!(
            image["properties"].get(key).is_some(),
            "image schema missing {key}"
        );
    }
    let video = by_name(&tools, VIDEO_TOOL_NAME).parameters_schema();
    for key in [
        "prompt",
        "duration",
        "resolution",
        "aspect_ratio",
        "first_frame",
        "last_frame",
        "references",
        "resume_job_id",
    ] {
        assert!(
            video["properties"].get(key).is_some(),
            "video schema missing {key}"
        );
    }
}

#[tokio::test]
async fn list_models_reports_both_catalogs_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let tools = tools(dir.path());
    let result = by_name(&tools, LIST_MODELS_TOOL_NAME)
        .execute(json!({}))
        .await
        .unwrap();
    let text = serde_json::to_string(&result).unwrap();
    assert!(
        text.contains("mock/image") && text.contains("mock/video"),
        "{text}"
    );
}

#[tokio::test]
async fn media_factory_files_mock_generated_output_from_its_staging_store() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let files = dir.path().join("Files");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let _storage = ObservingStorageGuard(crate::storage::install(Arc::new(ObservingStorage {
        inner: MemoryStorage::new(),
        observed: Arc::clone(&observed),
    })));
    let tools = media_tools_from(
        MediaGenerators::new(
            Arc::new(MockImageGenerator::new()),
            Arc::new(MockVideoGenerator::new(MockVideoScript::delivers())),
        ),
        dir.path(),
        &workspace,
        &files,
        tinyagents_harness::tinyinference_video::WaitPolicy::new(
            std::time::Duration::from_millis(1),
            std::time::Duration::from_secs(5),
        ),
    );

    let result = by_name(&tools, IMAGE_TOOL_NAME)
        .execute(json!({ "prompt": "a test image" }))
        .await
        .unwrap();
    assert!(!result.is_error, "{result:?}");
    let payload = result
        .content
        .iter()
        .find_map(|block| match block {
            tinytools::ToolContent::Json { data } => Some(data),
            _ => None,
        })
        .expect("generated media result");
    let artifact = &payload["artifacts"][0];
    assert!(artifact["artifact_id"].is_string(), "{artifact}");
    let output_path = std::path::Path::new(artifact["path"].as_str().unwrap());
    assert!(output_path.starts_with(&files));
    let output_bytes = std::fs::read(output_path).unwrap();
    let observed = observed.lock().unwrap();
    assert!(
        observed
            .iter()
            .any(|(_, written, read_back)| written == &output_bytes && read_back == &output_bytes),
        "media staging should be written through the installed scoped store and readable through a second handle; observed {} writes",
        observed.len()
    );
}

struct ObservingStorageGuard(Option<Arc<dyn StorageBackend>>);

impl Drop for ObservingStorageGuard {
    fn drop(&mut self) {
        crate::storage::clear();
        if let Some(previous) = self.0.take() {
            crate::storage::install(previous);
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
        let peer = Arc::clone(scoped.blobs());
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

#[test]
fn reference_policy_admits_workspace_files_only() {
    let action = Path::new("/home/user/OpenHuman/projects");
    let workspace = Path::new("/home/user/.openhuman/users/u/workspace");
    let allowed_artifacts = Arc::new(Mutex::new(HashSet::new()));
    let policy = reference_policy(action, workspace, Arc::clone(&allowed_artifacts));

    assert!(policy(&action.join("art/ref.png")).is_ok());
    assert!(policy(&workspace.join("attachments/photo.jpg")).is_ok());
    assert!(policy(&action.join("../secret.png")).is_err());
    assert!(policy(Path::new("/etc/passwd")).is_err());
    assert!(policy(Path::new("/home/user/Documents/private.png")).is_err());
    let files = tempfile::tempdir().unwrap();
    let filed = files.path().join("generated.png");
    std::fs::write(&filed, b"generated image").unwrap();
    let filed = filed.canonicalize().unwrap();
    allowed_artifacts.lock().unwrap().insert(filed.clone());
    assert!(policy(&filed).is_ok());
    assert!(policy(&files.path().join("unrelated.png")).is_err());
    assert!(
        policy(&action.join(".ssh/id_rsa")).is_err(),
        "credential stores stay forbidden"
    );
}
