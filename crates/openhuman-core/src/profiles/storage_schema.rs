//! The `profiles` owner's storage schema: the registry of provisioned
//! profiles, kept in the shared `cluster` scope.
//!
//! | v1 | v2 | scope |
//! |---|---|---|
//! | `profiles` (`{profile_id, created_at (seconds), layout_version}`) | `profiles.registry` (`{profile_id, layout_version}`; creation time in tsd metadata) | cluster |
//! | `leases` (`{owner, endpoint, epoch, expires_at_ms, released}`) | `tsd.leases`, owner `profiles` (`{holder, epoch, expires_ms, endpoint?, released?}`) | cluster |
//!
//! The lease move is not a document step (a step writes only its own
//! owner's collections), so [`migrate_legacy_leases`] does it after the steps.
//!
//! The cluster scope is not a tenant, so boot's per-tenant migration never
//! visits it; [`migrate_cluster`] runs this owner's steps over it, the first
//! time a registry opens the shared backend.

use serde_json::Value;
use tinystoragedrivers::schema::{
    MigrateOptions, Registry, Schema, ScopeClass, SourceRecord, Step, TargetRecord,
};
use tinystoragedrivers::{CollectionSpec, Result, ScopedStorage};

use crate::storage::schema_support::{key_of, owner, Body};

/// The v2 collection: one document per profile.
pub const REGISTRY_COLLECTION: &str = "profiles.registry";
/// The v1 name.
pub(super) const LEGACY_REGISTRY_COLLECTION: &str = "profiles";
const SCHEMA: u32 = 2;

/// What the profile registry keeps and the step from the v1 collection.
#[must_use]
pub fn storage_schema() -> Schema {
    Schema::new(owner("profiles"))
        .collection_with(
            SCHEMA,
            CollectionSpec::new(REGISTRY_COLLECTION),
            ScopeClass::Cluster,
        )
        .step(
            Step::new("registry_v2", REGISTRY_COLLECTION, 1, SCHEMA, registry_v2)
                .source(LEGACY_REGISTRY_COLLECTION),
        )
}

fn registry_v2(batch: Vec<SourceRecord>) -> Result<Vec<TargetRecord>> {
    batch
        .into_iter()
        .map(|record| {
            let old = &record.body;
            let body = Body::new()
                .copy(old, "profile_id")
                .set(
                    "layout_version",
                    old.get("layout_version")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                )
                .build();
            // Unix seconds become the record's creation time.
            let created_ms = old
                .get("created_at")
                .and_then(Value::as_i64)
                .map_or(record.meta.created_ms, |secs| secs.saturating_mul(1000));
            Ok(
                TargetRecord::new(REGISTRY_COLLECTION, key_of(&record.id)?, body)
                    .created_ms(created_ms),
            )
        })
        .collect()
}

/// Runs this owner's steps over the cluster scope `scoped`. Idempotent: a
/// finished step is skipped.
///
/// # Errors
///
/// A storage error; the registry then reads the unmigrated records as absent
/// until the next open retries.
pub async fn migrate_cluster(scoped: &ScopedStorage) -> Result<()> {
    let registry = Registry::compose([storage_schema()])?;
    scoped
        .documents()
        .ensure_collection(&CollectionSpec::new(REGISTRY_COLLECTION))
        .await?;
    registry.migrate(scoped, &MigrateOptions::new()).await?;
    migrate_legacy_leases(scoped).await?;
    Ok(())
}

/// The v1 lease collection.
pub(super) const LEGACY_LEASE_COLLECTION: &str = "leases";

/// The `tsd.leases` body of a v1 lease record, or `None` for a malformed one.
fn lease_v2(old: &Value) -> Option<Value> {
    let mut body = serde_json::Map::new();
    body.insert("holder".into(), old.get("owner")?.as_str()?.into());
    body.insert("epoch".into(), old.get("epoch")?.as_u64()?.into());
    body.insert(
        "expires_ms".into(),
        i64::try_from(old.get("expires_at_ms")?.as_u64()?)
            .unwrap_or(i64::MAX)
            .into(),
    );
    if let Some(endpoint) = old.get("endpoint").and_then(Value::as_str) {
        body.insert("endpoint".into(), endpoint.into());
    }
    if old.get("released").and_then(Value::as_bool) == Some(true) {
        body.insert("released".into(), true.into());
    }
    Some(Value::Object(body))
}

/// Moves the v1 `leases` records of the cluster scope into `tsd.leases`
/// (owner `profiles`), then drops the collection. A record already in
/// `tsd.leases` wins: it is newer. Epochs carry over, so a holder fenced at
/// the old epoch stays fenced. Returns the records moved.
///
/// This is a stop-the-world cutover, like the rest of schema v2 (the registry
/// in this same scope moves the same way): every node of a deployment must
/// run this version, because an old node would acquire in a recreated
/// `leases` collection that new nodes never read.
///
/// # Errors
///
/// A storage error; the records not yet moved stay for the next open.
pub async fn migrate_legacy_leases(scoped: &ScopedStorage) -> Result<usize> {
    use tinystoragedrivers::{Capability, DocumentStoreExt, Key, Precondition, Query};
    let docs = scoped.documents();
    // A driver that cannot list collections is asked for the records
    // directly, so a live legacy lease is never silently ignored.
    if docs
        .capabilities()
        .contains(Capability::CollectionEnumeration)
        && !docs
            .collections("")
            .await?
            .iter()
            .any(|name| name == LEGACY_LEASE_COLLECTION)
    {
        return Ok(0);
    }
    tinystoragedrivers::primitives::ensure(scoped).await?;
    let mut moved = 0;
    for legacy in docs
        .query_all(LEGACY_LEASE_COLLECTION, &Query::all())
        .await?
    {
        let Some(body) = lease_v2(&legacy.doc) else {
            log::warn!("[profiles][lease] skipping a malformed legacy lease record");
            continue;
        };
        let id = Key::of([crate::storage::lease::LEASE_OWNER, legacy.id.as_str()])?;
        match docs
            .put(
                tinystoragedrivers::primitives::LEASES,
                id.as_str(),
                body,
                Precondition::Absent,
            )
            .await
        {
            Ok(_) => moved += 1,
            Err(error) if error.kind() == tinystoragedrivers::ErrorKind::Conflict => {}
            Err(error) => return Err(error),
        }
    }
    docs.drop_collection(LEGACY_LEASE_COLLECTION).await?;
    log::info!("[profiles][lease] moved {moved} legacy lease records to tsd.leases");
    Ok(moved)
}

#[cfg(test)]
#[path = "storage_schema_tests.rs"]
mod tests;
