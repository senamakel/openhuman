//! [`DocumentLeases`]: leases as compare-and-swapped documents.
//!
//! One record per key in collection [`LEASE_COLLECTION`] of the scope the
//! caller binds (normally [`CLUSTER_SCOPE`], see [`DocumentLeases::cluster`]).
//! Every write carries a precondition (`Absent` for a fresh key, `Version`
//! otherwise), so two nodes racing for a key can never both win, provided
//! the driver's CAS holds across processes
//! ([`super::driver_has_cross_process_cas`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tinystoragedrivers::{CollectionSpec, ErrorKind, Precondition, Scope, Version};

use super::lease::{
    decide, validate_key, LeaseError, LeaseGrant, LeaseRecord, LeaseStore, Takeover, CLUSTER_SCOPE,
    LEASE_COLLECTION,
};
use super::{DocumentStore, StorageBackend, StorageError};

/// Acquire attempts before a key that keeps changing is reported as a
/// storage conflict.
const ACQUIRE_ATTEMPTS: usize = 16;

/// Leases kept as documents. See the module docs.
pub struct DocumentLeases {
    docs: Arc<dyn DocumentStore>,
    node: String,
    endpoint: Option<String>,
    ttl_ms: u64,
    /// The epoch this instance holds per key, which makes a repeated acquire
    /// re-entrant and lets a restarted node (same id, fresh instance) see its
    /// own leftover record as unclean.
    held: Mutex<HashMap<String, u64>>,
    /// Serializes `acquire` on this instance so an older completion cannot
    /// overwrite newer held state.
    acquire_gate: tokio::sync::Mutex<()>,
    declared: tokio::sync::OnceCell<()>,
}

impl std::fmt::Debug for DocumentLeases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentLeases")
            .field("node", &self.node)
            .field("endpoint", &self.endpoint)
            .field("ttl_ms", &self.ttl_ms)
            .finish_non_exhaustive()
    }
}

impl DocumentLeases {
    /// Leases in `docs` (already bound to the shared scope) for `node`,
    /// reachable at `endpoint`, each grant lasting `ttl` past its last
    /// acquire or renew.
    pub fn new(
        docs: Arc<dyn DocumentStore>,
        node: impl Into<String>,
        endpoint: Option<String>,
        ttl: Duration,
    ) -> Self {
        Self {
            docs,
            node: node.into(),
            endpoint,
            // A sub-millisecond TTL rounds up so a grant never expires at
            // the instant it is issued.
            ttl_ms: u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX).max(1),
            held: Mutex::new(HashMap::new()),
            acquire_gate: tokio::sync::Mutex::new(()),
            declared: tokio::sync::OnceCell::new(),
        }
    }

    /// [`Self::new`] over `backend`'s [`CLUSTER_SCOPE`].
    ///
    /// # Errors
    ///
    /// When the backend refuses the scope.
    pub fn cluster(
        backend: &dyn StorageBackend,
        node: impl Into<String>,
        endpoint: Option<String>,
        ttl: Duration,
    ) -> Result<Self, StorageError> {
        let scoped = backend.for_scope(&Scope::new(CLUSTER_SCOPE)?)?;
        Ok(Self::new(
            Arc::clone(scoped.documents()),
            node,
            endpoint,
            ttl,
        ))
    }

    /// This node's id.
    pub fn node(&self) -> &str {
        &self.node
    }

    async fn declare(&self) -> Result<(), StorageError> {
        self.declared
            .get_or_try_init(|| async {
                self.docs
                    .ensure_collection(&CollectionSpec::new(LEASE_COLLECTION))
                    .await
            })
            .await
            .map(|_| ())
    }

    fn held_epoch(&self, key: &str) -> Option<u64> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .copied()
    }

    fn set_held(&self, key: &str, epoch: Option<u64>) {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match epoch {
            Some(epoch) => held.insert(key.to_string(), epoch),
            None => held.remove(key),
        };
    }

    /// Drops the held entry only while it still names `epoch`, so a release
    /// finishing late cannot erase a newer acquisition's state.
    fn clear_held_if(&self, key: &str, epoch: u64) {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held.get(key) == Some(&epoch) {
            held.remove(key);
        }
    }

    fn record(&self, epoch: u64, expires_at_ms: u64, released: bool) -> LeaseRecord {
        LeaseRecord {
            owner: self.node.clone(),
            endpoint: self.endpoint.clone(),
            epoch,
            expires_at_ms,
            released,
        }
    }

    async fn read(&self, key: &str) -> Result<Option<(Version, LeaseRecord)>, StorageError> {
        let Some(stored) = self.docs.get(LEASE_COLLECTION, key).await? else {
            return Ok(None);
        };
        let record: LeaseRecord = serde_json::from_value(stored.doc)
            .map_err(|error| StorageError::serialization(format!("lease record {key}: {error}")))?;
        Ok(Some((stored.version, record)))
    }

    async fn write(
        &self,
        key: &str,
        record: &LeaseRecord,
        precondition: Precondition,
    ) -> Result<Version, StorageError> {
        let doc = serde_json::to_value(record)
            .map_err(|error| StorageError::serialization(error.to_string()))?;
        self.docs
            .put(LEASE_COLLECTION, key, doc, precondition)
            .await
    }

    /// The CAS write renew and release share: `Conflict` means the grant is
    /// no longer current.
    async fn rewrite(
        &self,
        grant: &LeaseGrant,
        expires_at_ms: u64,
        released: bool,
    ) -> Result<Version, LeaseError> {
        validate_key(&grant.key)?;
        if self.held_epoch(&grant.key) != Some(grant.epoch) {
            tracing::info!(
                target: "openhuman::storage::lease",
                key = %grant.key,
                node = %self.node,
                epoch = grant.epoch,
                "[lease] grant lost: not held by this instance"
            );
            return Err(LeaseError::Lost);
        }
        self.declare().await?;
        let record = self.record(grant.epoch, expires_at_ms, released);
        match self
            .write(&grant.key, &record, Precondition::Version(grant.version))
            .await
        {
            Ok(version) => Ok(version),
            Err(error) if error.kind() == ErrorKind::Conflict => {
                // The held entry stays: it names an epoch the record has
                // moved past, so it can never make a later acquire re-entrant.
                tracing::info!(
                    target: "openhuman::storage::lease",
                    key = %grant.key,
                    node = %self.node,
                    epoch = grant.epoch,
                    "[lease] grant lost: the record changed since it was written"
                );
                Err(LeaseError::Lost)
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl LeaseStore for DocumentLeases {
    async fn acquire(&self, key: &str, now_ms: u64) -> Result<LeaseGrant, LeaseError> {
        validate_key(key)?;
        self.declare().await?;
        let _gate = self.acquire_gate.lock().await;
        let expires_at_ms = now_ms.saturating_add(self.ttl_ms);
        for attempt in 0..ACQUIRE_ATTEMPTS {
            let found = self.read(key).await?;
            let (epoch, unclean) = match decide(
                found.as_ref().map(|(_, r)| r),
                &self.node,
                self.held_epoch(key),
                now_ms,
            ) {
                Takeover::Take { epoch, unclean } => (epoch, unclean),
                Takeover::Exhausted => {
                    return Err(StorageError::conflict(format!(
                        "lease {key} epoch space exhausted"
                    ))
                    .into());
                }
                Takeover::Refuse => {
                    let (_, record) = found.expect("refuse implies a record");
                    tracing::debug!(
                        target: "openhuman::storage::lease",
                        key,
                        node = %self.node,
                        owner = %record.owner,
                        epoch = record.epoch,
                        "[lease] acquire refused: held elsewhere"
                    );
                    return Err(LeaseError::Held(record));
                }
            };
            let precondition = found.as_ref().map_or(Precondition::Absent, |(version, _)| {
                Precondition::Version(*version)
            });
            let record = self.record(epoch, expires_at_ms, false);
            match self.write(key, &record, precondition).await {
                Ok(version) => {
                    self.set_held(key, Some(epoch));
                    tracing::debug!(
                        target: "openhuman::storage::lease",
                        key,
                        node = %self.node,
                        epoch,
                        previous_unclean = unclean,
                        "[lease] acquired"
                    );
                    return Ok(LeaseGrant {
                        key: key.to_string(),
                        epoch,
                        version,
                        expires_at_ms,
                        previous_unclean: unclean,
                    });
                }
                Err(error) if error.kind() == ErrorKind::Conflict => {
                    tracing::debug!(
                        target: "openhuman::storage::lease",
                        key,
                        node = %self.node,
                        attempt,
                        "[lease] acquire raced another writer; re-reading"
                    );
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(StorageError::conflict(format!(
            "lease {key} kept changing under {ACQUIRE_ATTEMPTS} attempts"
        ))
        .into())
    }

    async fn renew(&self, grant: &LeaseGrant, now_ms: u64) -> Result<LeaseGrant, LeaseError> {
        if now_ms >= grant.expires_at_ms {
            tracing::info!(
                target: "openhuman::storage::lease",
                key = %grant.key,
                node = %self.node,
                epoch = grant.epoch,
                "[lease] renew refused: grant already expired"
            );
            return Err(LeaseError::Lost);
        }
        let expires_at_ms = now_ms.saturating_add(self.ttl_ms);
        let version = self.rewrite(grant, expires_at_ms, false).await?;
        tracing::trace!(
            target: "openhuman::storage::lease",
            key = %grant.key,
            node = %self.node,
            epoch = grant.epoch,
            "[lease] renewed"
        );
        Ok(LeaseGrant {
            version,
            expires_at_ms,
            ..grant.clone()
        })
    }

    async fn release(&self, grant: LeaseGrant) -> Result<(), LeaseError> {
        self.rewrite(&grant, grant.expires_at_ms, true).await?;
        self.clear_held_if(&grant.key, grant.epoch);
        tracing::debug!(
            target: "openhuman::storage::lease",
            key = %grant.key,
            node = %self.node,
            epoch = grant.epoch,
            "[lease] released"
        );
        Ok(())
    }

    async fn holder(&self, key: &str) -> Result<Option<LeaseRecord>, LeaseError> {
        validate_key(key)?;
        self.declare().await?;
        Ok(self.read(key).await?.map(|(_, record)| record))
    }
}

#[cfg(test)]
#[path = "lease_documents_tests.rs"]
mod tests;
