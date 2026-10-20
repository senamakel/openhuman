use super::MemoryRoundtripCheck;
use crate::config::Config;
use crate::doctor::contract::{DoctorCheck, DoctorCtx, FindingSeverity};
use crate::memory::engine;
use crate::memory::test_fixtures::stored;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tinymemory_api::conformance::ReferenceEngine;
use tinymemory_api::{
    EngineDescriptor, EngineHealth, Error as MemoryApiError, FetchPage, FetchRequest,
    ForgetReport, ForgetTarget, ItemId, ListPage, ListRequest, MemoryEngine, MemoryMeta,
    MetaFilter, Namespace, Reach, RecallAnswer, RecallRequest, StoreItem, StoreReceipt,
};

#[derive(Clone)]
enum Failure {
    None,
    Fetch,
    StoreAfterAccept(MemoryApiError),
}

#[derive(Default)]
struct Calls {
    steps: Vec<&'static str>,
    stored: Vec<(ItemId, Namespace)>,
    scoped_forgets: Vec<(Vec<ItemId>, Reach)>,
    unscoped_forgets: usize,
}

struct ProbeEngine {
    inner: ReferenceEngine,
    failure: Failure,
    calls: Mutex<Calls>,
}

impl ProbeEngine {
    fn new(failure: Failure) -> Self {
        Self {
            inner: ReferenceEngine::new(),
            failure,
            calls: Mutex::new(Calls::default()),
        }
    }

    fn bind(self: &Arc<Self>, config: &Config) -> EngineBinding {
        engine::install_test_engine(&config.workspace_dir, self.clone());
        EngineBinding(config.workspace_dir.clone())
    }
}

struct EngineBinding(std::path::PathBuf);

impl Drop for EngineBinding {
    fn drop(&mut self) {
        engine::remove_test_engine(&self.0);
    }
}

#[async_trait]
impl MemoryEngine for ProbeEngine {
    fn descriptor(&self) -> &EngineDescriptor {
        self.inner.descriptor()
    }

    async fn health(&self) -> EngineHealth {
        self.inner.health().await
    }

    async fn recall(&self, request: RecallRequest) -> tinymemory_api::Result<RecallAnswer> {
        self.inner.recall(request).await
    }

    async fn fetch(&self, request: FetchRequest) -> tinymemory_api::Result<FetchPage> {
        self.calls.lock().unwrap().steps.push("fetch");
        if matches!(&self.failure, Failure::Fetch) {
            return Err(MemoryApiError::Unavailable("mock fetch failure".into()));
        }
        self.inner.fetch(request).await
    }

    async fn store(&self, item: StoreItem) -> tinymemory_api::Result<StoreReceipt> {
        self.calls.lock().unwrap().steps.push("store");
        let id = ItemId::new(item.fingerprint());
        let namespace = item.meta().namespace.clone();
        let receipt = self.inner.store(item).await?;
        self.calls
            .lock()
            .unwrap()
            .stored
            .push((id, namespace));
        if let Failure::StoreAfterAccept(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(receipt)
    }

    async fn forget(&self, target: ForgetTarget) -> tinymemory_api::Result<ForgetReport> {
        self.calls.lock().unwrap().unscoped_forgets += 1;
        self.inner.forget(target).await
    }

    async fn forget_within(
        &self,
        ids: Vec<ItemId>,
        reach: Reach,
    ) -> tinymemory_api::Result<ForgetReport> {
        let mut calls = self.calls.lock().unwrap();
        calls.steps.push("forget_within");
        calls.scoped_forgets.push((ids.clone(), reach.clone()));
        drop(calls);
        self.inner.forget_within(ids, reach).await
    }

    async fn list(&self, request: ListRequest) -> tinymemory_api::Result<ListPage> {
        self.inner.list(request).await
    }

}

fn config_in(tmp: &TempDir) -> Config {
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace directory");
    Config {
        workspace_dir: workspace.clone(),
        action_dir: workspace,
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    }
}

async fn seed_real_memory(engine: &ProbeEngine) -> ItemId {
    let item = StoreItem::document("existing user memory", MemoryMeta::default());
    let id = ItemId::new(item.fingerprint());
    engine.inner.store(item).await.expect("seed existing memory");
    id
}

async fn run_check(engine: &Arc<ProbeEngine>, tmp: &TempDir) -> Vec<crate::doctor::contract::Finding> {
    let config = config_in(tmp);
    let _binding = engine.bind(&config);
    MemoryRoundtripCheck.detect(&DoctorCtx::from_config(config)).await
}

async fn assert_scratch_namespaces_are_empty(engine: &ProbeEngine, namespaces: &[Namespace]) {
    for namespace in namespaces {
        let items = stored(
            &engine.inner,
            MetaFilter {
                reach: Some(Reach::subtree(namespace.clone())),
                ..MetaFilter::default()
            },
        )
        .await;
        assert!(items.is_empty(), "scratch item remained in {namespace}");
    }
}

fn only_error(findings: &[crate::doctor::contract::Finding]) -> &crate::doctor::contract::Finding {
    findings
        .iter()
        .find(|finding| finding.severity == FindingSeverity::Error)
        .expect("the live memory failure is reported as an error")
}

#[tokio::test]
async fn fetch_failure_still_cleans_the_unique_service_scoped_item() {
    let tmp = TempDir::new().expect("temporary config and workspace");
    let config = config_in(&tmp);
    let engine = Arc::new(ProbeEngine::new(Failure::Fetch));
    let real_id = seed_real_memory(&engine).await;
    let _binding = engine.bind(&config);

    let findings = MemoryRoundtripCheck
        .detect(&DoctorCtx::from_config(config))
        .await;

    assert!(only_error(&findings).message.contains("fetch"));
    let calls = engine.calls.lock().unwrap();
    assert_eq!(calls.steps, ["store", "fetch", "forget_within"]);
    assert_eq!(calls.unscoped_forgets, 0, "cleanup is confined to the probe scope");
    assert_eq!(calls.stored.len(), 1);
    let (scratch_id, namespace) = &calls.stored[0];
    assert_ne!(scratch_id, &real_id);
    assert_ne!(scratch_id.as_str(), "doctor:probe");
    assert!(namespace.to_string().starts_with("service:openhuman-doctor-"));
    assert!(!Reach::subtree(Namespace::ROOT).admits(namespace));
    assert_eq!(calls.scoped_forgets.len(), 1);
    assert_eq!(calls.scoped_forgets[0].0, vec![scratch_id.clone()]);
    assert_eq!(calls.scoped_forgets[0].1, Reach::exact(namespace.clone()));
    let namespaces = calls
        .stored
        .iter()
        .map(|(_, namespace)| namespace.clone())
        .collect::<Vec<_>>();
    drop(calls);

    assert_scratch_namespaces_are_empty(&engine, &namespaces).await;
    let remaining = stored(&engine.inner, MetaFilter::default()).await;
    assert_eq!(remaining.len(), 1, "only the pre-existing user memory remains");
    assert_eq!(remaining[0].id, real_id);
}

#[tokio::test]
async fn ambiguous_store_failure_attempts_scoped_cleanup_and_reports_a_billing_hint() {
    let tmp = TempDir::new().expect("temporary config and workspace");
    let config = config_in(&tmp);
    let engine = Arc::new(ProbeEngine::new(Failure::StoreAfterAccept(
        MemoryApiError::Engine(
            "[USER_INSUFFICIENT_CREDITS] mock account has insufficient credits (HTTP 402)".into(),
        ),
    )));
    let real_id = seed_real_memory(&engine).await;
    let _binding = engine.bind(&config);

    let findings = MemoryRoundtripCheck
        .detect(&DoctorCtx::from_config(config))
        .await;

    let finding = only_error(&findings);
    assert!(
        finding.fix_hint.to_ascii_lowercase().contains("credit")
            || finding.fix_hint.to_ascii_lowercase().contains("billing"),
        "insufficient credits needs an actionable billing hint: {}",
        finding.fix_hint
    );
    let calls = engine.calls.lock().unwrap();
    assert_eq!(calls.steps, ["store", "forget_within"]);
    assert_eq!(calls.unscoped_forgets, 0);
    assert_eq!(calls.scoped_forgets.len(), 1);
    assert_eq!(calls.scoped_forgets[0].0, vec![calls.stored[0].0.clone()]);
    let namespaces = calls
        .stored
        .iter()
        .map(|(_, namespace)| namespace.clone())
        .collect::<Vec<_>>();
    drop(calls);

    assert_scratch_namespaces_are_empty(&engine, &namespaces).await;
    let remaining = stored(&engine.inner, MetaFilter::default()).await;
    assert_eq!(remaining.len(), 1, "cleanup removes a possibly accepted write");
    assert_eq!(remaining[0].id, real_id);
}

#[tokio::test]
async fn unauthorized_and_unavailable_errors_have_safe_recovery_hints() {
    let cases = [
        (
            MemoryApiError::Unauthorized("secret token must not be echoed".into()),
            ["sign in", "auth", "credential"],
        ),
        (
            MemoryApiError::Unavailable("private endpoint detail must not be echoed".into()),
            ["retry", "available", "service"],
        ),
    ];

    for (error, hint_words) in cases {
        let tmp = TempDir::new().expect("temporary config and workspace");
        let engine = Arc::new(ProbeEngine::new(Failure::StoreAfterAccept(error)));
        let findings = run_check(&engine, &tmp).await;
        let finding = only_error(&findings);
        let hint = finding.fix_hint.to_ascii_lowercase();
        assert!(
            hint_words.iter().any(|word| hint.contains(word)),
            "error needs a safe, relevant recovery hint: {}",
            finding.fix_hint
        );
        assert!(!finding.message.contains("secret token"));
        assert!(!finding.message.contains("private endpoint"));
        assert!(!finding.fix_hint.contains("secret token"));
        assert!(!finding.fix_hint.contains("private endpoint"));
        assert_eq!(engine.calls.lock().unwrap().unscoped_forgets, 0);
    }
}

#[tokio::test]
async fn each_successful_probe_uses_a_fresh_service_namespace_and_preserves_user_memory() {
    let tmp = TempDir::new().expect("temporary config and workspace");
    let config = config_in(&tmp);
    let engine = Arc::new(ProbeEngine::new(Failure::None));
    let real_id = seed_real_memory(&engine).await;
    let _binding = engine.bind(&config);
    let ctx = DoctorCtx::from_config(config);

    for _ in 0..2 {
        let findings = MemoryRoundtripCheck.detect(&ctx).await;
        assert!(
            findings
                .iter()
                .all(|finding| finding.severity != FindingSeverity::Error),
            "a successful store/fetch/forget roundtrip passes"
        );
    }

    let calls = engine.calls.lock().unwrap();
    assert_eq!(calls.steps, ["store", "fetch", "forget_within", "store", "fetch", "forget_within"]);
    assert_eq!(calls.stored.len(), 2);
    assert_ne!(calls.stored[0].0, calls.stored[1].0, "probe IDs are unique per run");
    assert_ne!(calls.stored[0].1, calls.stored[1].1, "probe namespaces are unique per run");
    assert!(calls.stored.iter().all(|(_, namespace)| {
        namespace.to_string().starts_with("service:openhuman-doctor-")
            && !Reach::subtree(Namespace::ROOT).admits(namespace)
    }));
    assert_eq!(calls.unscoped_forgets, 0);
    assert_eq!(calls.scoped_forgets.len(), 2);
    let namespaces = calls
        .stored
        .iter()
        .map(|(_, namespace)| namespace.clone())
        .collect::<Vec<_>>();
    drop(calls);

    assert_scratch_namespaces_are_empty(&engine, &namespaces).await;
    let remaining = stored(&engine.inner, MetaFilter::default()).await;
    assert_eq!(remaining.len(), 1, "the scratch probes are gone");
    assert_eq!(remaining[0].id, real_id, "unrelated memory is preserved");
}
