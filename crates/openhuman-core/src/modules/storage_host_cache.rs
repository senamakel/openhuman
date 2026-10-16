//! Sender identity and bounded module/scope binding caches for the storage host.

use super::*;
use std::collections::HashSet;

const MAX_BINDINGS: usize = 256;

/// A per-peer lookup cell. It serializes concurrent first calls from one
/// sender without making unrelated senders wait for broker owner lookups.
pub(super) struct PeerModule {
    module: tokio::sync::Mutex<Option<String>>,
}

pub(super) enum BindError {
    NoBackend,
    NoGrant,
    BadGrant,
    BindingLimit,
    Backend(StorageKind),
}

impl BindError {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::NoBackend => "no-backend",
            Self::NoGrant => "no-grant-declared",
            Self::BadGrant => "invalid-grant",
            Self::BindingLimit => "binding-cache-full",
            Self::Backend(_) => "backend-error",
        }
    }

    pub(super) fn into_host(self) -> HostError {
        match self {
            Self::NoBackend => super::failure(StorageKind::Unavailable),
            Self::NoGrant => HostError::Denied("no-grant-declared"),
            Self::BadGrant => HostError::Denied("invalid-grant"),
            Self::BindingLimit => super::failure(StorageKind::Unavailable),
            Self::Backend(kind) => super::failure(kind),
        }
    }
}

impl StorageHost {
    /// Resolves a sender once for its broker peer lifetime. The cache entry is
    /// detached from the host before an owner lookup completes, so a late
    /// result cannot restore state after the peer's detach event.
    pub(super) async fn module_for(&self, sender: &BusName) -> Option<String> {
        let entry = {
            let mut peers = self.peer_modules.lock().ok()?;
            Arc::clone(peers.entry(sender.clone()).or_insert_with(|| {
                Arc::new(PeerModule {
                    module: tokio::sync::Mutex::new(None),
                })
            }))
        };
        let mut cached = entry.module.lock().await;
        if let Some(module) = cached.as_ref() {
            return Some(module.clone());
        }
        let resolved = self.directory.module_for(sender).await;
        let mut peers = self.peer_modules.lock().ok()?;
        let still_live_entry = peers
            .get(sender)
            .is_some_and(|current| Arc::ptr_eq(current, &entry));
        if !still_live_entry {
            return None;
        }
        match resolved {
            Some(module) => {
                *cached = Some(module.clone());
                Some(module)
            }
            None => {
                peers.remove(sender);
                None
            }
        }
    }

    /// Invalidates the peer identity and drops idle bindings after detach.
    /// A binding feeding another live peer's watch remains in the cache.
    pub(super) fn release_peer(&self, sender: &BusName) {
        if let Ok(mut peers) = self.peer_modules.lock() {
            peers.remove(sender);
        }
        self.release_relay(sender);
        self.prune_idle_bindings();
    }

    pub(super) fn peer_candidates(&self) -> HashSet<BusName> {
        let mut peers = self
            .peer_modules
            .lock()
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default();
        if let Ok(relays) = self.relays.lock() {
            peers.extend(relays.keys().cloned());
        }
        peers
    }

    /// Bindings are shared by module and scope. Keep those feeding a live
    /// caller's watch, and drop idle bindings so profile churn stays bounded.
    fn prune_idle_bindings(&self) {
        let Ok(mut bindings) = self.bindings.try_lock() else {
            return;
        };
        // Keep the relay guard until after retain: a concurrent first watch
        // either publishes its relay before this snapshot or holds a strong
        // binding reference that protects the entry until it does.
        if let Ok(relays) = self.relays.lock() {
            bindings.retain(|_, binding| {
                Arc::strong_count(binding) > 1
                    || relays.values().any(|relay| {
                        relay
                            .fed_by
                            .as_ref()
                            .is_some_and(|fed| fed.ptr_eq(&Arc::downgrade(binding)))
                    })
            });
        }
    }

    /// Builds outside the shared lock, then publishes only if this backend is
    /// still installed. Concurrent builders reuse the first completed binding.
    pub(super) async fn bind(
        &self,
        module: &str,
        scope: Scope,
    ) -> Result<Arc<Binding>, BindError> {
        let key = (module.to_string(), scope.as_str().to_string());
        loop {
            let backend = self.environment.backend().ok_or(BindError::NoBackend)?;
            if let Some(existing) = self.bindings.lock().await.get(&key) {
                if Arc::ptr_eq(&existing.backend, &backend) {
                    return Ok(Arc::clone(existing));
                }
            }
            let built = self.build_binding(module, scope.clone(), Arc::clone(&backend)).await?;

            let mut bindings = self.bindings.lock().await;
            if let Some(existing) = bindings.get(&key) {
                if Arc::ptr_eq(&existing.backend, &backend) {
                    return Ok(Arc::clone(existing));
                }
            }
            let still_current = self
                .environment
                .backend()
                .is_some_and(|current| Arc::ptr_eq(&current, &backend));
            if !still_current {
                drop(bindings);
                continue;
            }

            // Replacing this key does not grow the map, even when every entry
            // is pinned by a live watch or call.
            if !bindings.contains_key(&key) && bindings.len() >= MAX_BINDINGS {
                let Ok(relays) = self.relays.lock() else {
                    return Err(BindError::BindingLimit);
                };
                let evict = bindings.iter().find_map(|(candidate_key, candidate)| {
                    let feeding_watch = relays.values().any(|relay| {
                        relay
                            .fed_by
                            .as_ref()
                            .is_some_and(|fed| fed.ptr_eq(&Arc::downgrade(candidate)))
                    });
                    (!feeding_watch && Arc::strong_count(candidate) == 1)
                        .then(|| candidate_key.clone())
                });
                if let Some(evict) = evict {
                    bindings.remove(&evict);
                } else {
                    return Err(BindError::BindingLimit);
                }
            }
            bindings.insert(key.clone(), Arc::clone(&built));
            return Ok(built);
        }
    }

    async fn build_binding(
        &self,
        module: &str,
        scope: Scope,
        backend: Arc<dyn StorageBackend>,
    ) -> Result<Arc<Binding>, BindError> {
        let grant = self.environment.grant(module).ok_or(BindError::NoGrant)?;
        grant.validate().map_err(|reason| {
            tracing::error!(
                target: "openhuman::storage_host",
                module,
                reason,
                "[storage_host] refused an invalid storage grant"
            );
            BindError::BadGrant
        })?;
        let storage = backend
            .for_scope(&scope)
            .map_err(|_| BindError::Backend(StorageKind::Backend))?;
        for spec in grant.specs() {
            storage
                .documents()
                .ensure_collection(spec)
                .await
                .map_err(|_| BindError::Backend(StorageKind::Backend))?;
        }
        let mut server = StorageServer::new(storage.clone());
        if !grant.secret_prefixes.is_empty() {
            // A key-provider failure is retryable, so refuse the bind rather
            // than cache a server that silently lacks secrets.
            let keys = self.environment.keys().map_err(|_| {
                tracing::warn!(
                    target: "openhuman::storage_host",
                    module,
                    "[storage_host] no master key; refusing to bind a grant with secrets"
                );
                BindError::Backend(StorageKind::Backend)
            })?;
            server = server.with_secrets(Arc::new(DocumentSecrets::new(&storage, keys)));
        }
        Ok(Arc::new(Binding {
            backend,
            grant,
            server,
        }))
    }
}

