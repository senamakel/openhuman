use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

struct CountingDirectory {
    calls: AtomicUsize,
}

#[async_trait]
impl CallerDirectory for CountingDirectory {
    async fn module_for(&self, sender: &BusName) -> Option<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match sender.as_str() {
            ":1.1" => Some("alpha".into()),
            ":1.2" => Some("beta".into()),
            _ => None,
        }
    }
}

#[tokio::test]
async fn repeated_calls_from_one_peer_resolve_its_module_once() {
    let backend: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let env = Arc::new(Env {
        backend: Mutex::new(Some(backend)),
        profile: Mutex::new(None),
        grants: HashMap::from([("alpha", grant("notes"))]),
        fences: Mutex::new(None),
        keys_ready: AtomicBool::new(true),
    });
    let directory = Arc::new(CountingDirectory {
        calls: AtomicUsize::new(0),
    });
    let host = StorageHost::new(
        directory.clone(),
        env as Arc<dyn Environment>,
        Arc::new(Trail::default()),
    );
    for _ in 0..8 {
        host.handle(
            Object::Documents,
            "Get",
            get("notes", "missing"),
            Some(&alpha()),
            Admission::Private,
        )
        .await
        .unwrap();
    }
    assert_eq!(directory.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_detach_invalidates_the_sender_identity_cache() {
    let backend: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    let env = Arc::new(Env {
        backend: Mutex::new(Some(backend)),
        profile: Mutex::new(None),
        grants: HashMap::from([("alpha", grant("notes"))]),
        fences: Mutex::new(None),
        keys_ready: AtomicBool::new(true),
    });
    let directory = Arc::new(CountingDirectory { calls: AtomicUsize::new(0) });
    let host = StorageHost::new(directory.clone(), env as Arc<dyn Environment>, Arc::new(Trail::default()));
    for _ in 0..2 {
        host.handle(Object::Documents, "Get", get("notes", "x"), Some(&alpha()), Admission::Private).await.unwrap();
    }
    host.release_peer(&alpha());
    host.handle(Object::Documents, "Get", get("notes", "x"), Some(&alpha()), Admission::Private).await.unwrap();
    assert_eq!(directory.calls.load(Ordering::SeqCst), 2);
}

struct DelayedDirectory {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl CallerDirectory for DelayedDirectory {
    async fn module_for(&self, _sender: &BusName) -> Option<String> {
        self.entered.notify_one();
        self.release.notified().await;
        Some("alpha".into())
    }
}

#[tokio::test]
async fn an_owner_lookup_finishing_after_detach_cannot_restore_peer_state() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let host = Arc::new(StorageHost::new(
        Arc::new(DelayedDirectory { entered: entered.clone(), release: release.clone() }),
        Arc::new(Env {
            backend: Mutex::new(Some(Arc::new(MemoryStorage::new()))),
            profile: Mutex::new(None),
            grants: HashMap::from([("alpha", grant("notes"))]),
            fences: Mutex::new(None),
            keys_ready: AtomicBool::new(true),
        }),
        Arc::new(Trail::default()),
    ));
    let resolving = {
        let host = Arc::clone(&host);
        tokio::spawn(async move { host.module_for(&alpha()).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified()).await.unwrap();
    host.release_peer(&alpha());
    release.notify_one();
    assert_eq!(resolving.await.unwrap(), None);
    assert!(!host.peer_modules.lock().unwrap().contains_key(&alpha()));
}

struct EveryPeerIsAlpha;

#[async_trait]
impl CallerDirectory for EveryPeerIsAlpha {
    async fn module_for(&self, sender: &BusName) -> Option<String> {
        sender.is_unique().then(|| "alpha".to_owned())
    }
}

#[tokio::test]
async fn detaching_one_peer_preserves_another_peers_shared_watch_binding() {
    let rig = rig();
    let host = StorageHost::new(
        Arc::new(EveryPeerIsAlpha),
        Arc::clone(&rig.env) as Arc<dyn Environment>,
        Arc::clone(&rig.trail) as Arc<dyn AuditSink>,
    );
    let first = alpha();
    let second = beta();
    let mut first_signals = host.watch_signals(&first).unwrap();
    let mut second_signals = host.watch_signals(&second).unwrap();
    let watch = |sender: &BusName| {
        host.handle(
            Object::Documents,
            "WatchOpen",
            json!({"collection":"notes","filter":{"op":"all"},"interval_ms":20}),
            Some(sender),
            Admission::Private,
        )
    };
    let _first_watch = watch(&first).await.unwrap();
    let second_watch = watch(&second).await.unwrap();
    let second_watch_id = second_watch["watch_id"].as_str().unwrap();
    host.release_peer(&first);
    host.handle(
        Object::Documents,
        "Put",
        put("notes", "after-detach", json!({"v": 1})),
        Some(&second),
        Admission::Private,
    )
    .await
    .unwrap();
    let signal = tokio::time::timeout(std::time::Duration::from_secs(5), second_signals.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signal.watch_id, second_watch_id);
    assert!(first_signals.try_recv().is_err());
}

/// Holds alpha's schema preparation until the test releases it.
#[derive(Debug)]
struct BlockingStorage {
    inner: MemoryStorage,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl StorageBackend for BlockingStorage {
    fn driver(&self) -> &'static str { "memory" }
    fn capabilities(&self) -> tinystoragedrivers::Capabilities { self.inner.capabilities() }
    fn database(&self, name: &str) -> tinystoragedrivers::Result<Arc<dyn StorageBackend>> {
        self.inner.database(name)
    }
    fn for_scope(&self, scope: &Scope) -> tinystoragedrivers::Result<tinystoragedrivers::ScopedStorage> {
        let scoped = self.inner.for_scope(scope)?;
        Ok(tinystoragedrivers::ScopedStorage::new(
            scope.clone(),
            "memory",
            Arc::new(BlockingDocuments {
                inner: Arc::clone(scoped.documents()),
                blocked: scope.as_str().ends_with("alpha@local"),
                entered: Arc::clone(&self.entered),
                release: Arc::clone(&self.release),
            }),
            Arc::clone(scoped.streams()),
            Arc::clone(scoped.blobs()),
        ))
    }
}

#[derive(Debug)]
struct BlockingDocuments {
    inner: Arc<dyn tinystoragedrivers::DocumentStore>,
    blocked: bool,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl tinystoragedrivers::DocumentStore for BlockingDocuments {
    fn capabilities(&self) -> tinystoragedrivers::Capabilities { self.inner.capabilities() }
    async fn ensure_collection(&self, spec: &tinystoragedrivers::CollectionSpec) -> tinystoragedrivers::Result<()> {
        if self.blocked {
            self.entered.notify_one();
            self.release.notified().await;
        }
        self.inner.ensure_collection(spec).await
    }
    async fn get(&self, c: &str, id: &str) -> tinystoragedrivers::Result<Option<tinystoragedrivers::Versioned<Value>>> { self.inner.get(c, id).await }
    async fn put(&self, c: &str, id: &str, doc: Value, p: tinystoragedrivers::Precondition) -> tinystoragedrivers::Result<tinystoragedrivers::Version> { self.inner.put(c, id, doc, p).await }
    async fn delete(&self, c: &str, id: &str, p: tinystoragedrivers::Precondition) -> tinystoragedrivers::Result<bool> { self.inner.delete(c, id, p).await }
    async fn query(&self, c: &str, q: &tinystoragedrivers::Query) -> tinystoragedrivers::Result<tinystoragedrivers::Page<tinystoragedrivers::Versioned<Value>>> { self.inner.query(c, q).await }
    async fn count(&self, c: &str, f: &tinystoragedrivers::Filter) -> tinystoragedrivers::Result<u64> { self.inner.count(c, f).await }
    async fn delete_where(&self, c: &str, f: &tinystoragedrivers::Filter) -> tinystoragedrivers::Result<u64> { self.inner.delete_where(c, f).await }
    async fn claim(&self, c: &str, f: &tinystoragedrivers::Filter, s: &[tinystoragedrivers::Sort], p: &Value) -> tinystoragedrivers::Result<Option<tinystoragedrivers::Versioned<Value>>> { self.inner.claim(c, f, s, p).await }
    async fn drop_collection(&self, c: &str) -> tinystoragedrivers::Result<()> { self.inner.drop_collection(c).await }
    async fn collections(&self, p: &str) -> tinystoragedrivers::Result<Vec<String>> { self.inner.collections(p).await }
    async fn atomic_batch(&self, ops: Vec<tinystoragedrivers::WriteOp>) -> tinystoragedrivers::Result<Vec<tinystoragedrivers::WriteResult>> { self.inner.atomic_batch(ops).await }
}

#[tokio::test]
async fn one_module_binding_does_not_block_another_modules_first_call() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let backend: Arc<dyn StorageBackend> = Arc::new(BlockingStorage {
        inner: MemoryStorage::new(),
        entered: entered.clone(),
        release: release.clone(),
    });
    let env = Arc::new(Env {
        backend: Mutex::new(Some(backend)),
        profile: Mutex::new(None),
        grants: HashMap::from([("alpha", grant("notes")), ("beta", grant("other"))]),
        fences: Mutex::new(None),
        keys_ready: AtomicBool::new(true),
    });
    let directory = Directory(HashMap::from([
        (":1.1".into(), "alpha".into()),
        (":1.2".into(), "beta".into()),
    ]));
    let host = Arc::new(StorageHost::new(
        Arc::new(directory),
        env as Arc<dyn Environment>,
        Arc::new(Trail::default()),
    ));

    let alpha_host = Arc::clone(&host);
    let alpha_call = tokio::spawn(async move {
        alpha_host.handle(Object::Documents, "Get", get("notes", "x"), Some(&alpha()), Admission::Private).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("alpha reaches schema preparation");

    let beta_host = Arc::clone(&host);
    let beta_call = tokio::spawn(async move {
        beta_host.handle(Object::Documents, "Get", get("other", "x"), Some(&beta()), Admission::Private).await
    });
    let beta_result = tokio::time::timeout(std::time::Duration::from_secs(5), beta_call)
        .await
        .expect("beta's first bind should finish while alpha is preparing schema")
        .unwrap();
    assert!(beta_result.is_ok());
    release.notify_one();
    assert!(alpha_call.await.unwrap().is_ok());
}

#[tokio::test]
async fn a_slow_old_backend_cannot_replace_a_new_backend_binding() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let old_backend: Arc<dyn StorageBackend> = Arc::new(BlockingStorage {
        inner: MemoryStorage::new(),
        entered: entered.clone(),
        release: release.clone(),
    });
    let env = Arc::new(Env {
        backend: Mutex::new(Some(Arc::clone(&old_backend))),
        profile: Mutex::new(None),
        grants: HashMap::from([("alpha", grant("notes"))]),
        fences: Mutex::new(None),
        keys_ready: AtomicBool::new(true),
    });
    let host = Arc::new(StorageHost::new(
        Arc::new(Directory(HashMap::new())),
        Arc::clone(&env) as Arc<dyn Environment>,
        Arc::new(Trail::default()),
    ));
    let scope = module_scope(None, &Scope::local(), "alpha").unwrap();
    let first = {
        let host = Arc::clone(&host);
        let scope = scope.clone();
        tokio::spawn(async move { host.bind("alpha", scope).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified()).await.unwrap();
    let new_backend: Arc<dyn StorageBackend> = Arc::new(MemoryStorage::new());
    *env.backend.lock().unwrap() = Some(Arc::clone(&new_backend));
    host.bind("alpha", scope.clone()).await.unwrap();
    release.notify_one();
    let _ = first.await.unwrap().unwrap();
    let bindings = host.bindings.lock().await;
    assert!(Arc::ptr_eq(&bindings.get(&("alpha".into(), scope.as_str().into())).unwrap().backend, &new_backend));
}
