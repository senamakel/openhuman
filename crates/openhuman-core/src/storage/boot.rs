//! Prepare the host's known storage scopes before installing a backend.
//!
//! Tiny Storage Drivers v2 has no global schema registry or scope enumerator.
//! Collection declarations belong to each record owner. The host asks each
//! linked crate for its declarations and prepares them on the shared local and
//! cluster scopes; tenant scopes are prepared when their owner is created.

use std::sync::Arc;

use tinystoragedrivers::{Scope, Storage, StorageBackend, StorageError};

/// What a boot preparation run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BootReport {
    /// Known scopes prepared.
    pub scopes: usize,
    /// Collection declarations ensured across those scopes.
    pub collections: usize,
}

/// Ensures each linked crate's collections in the host's shared scopes.
///
/// Per-profile and per-agent scopes are prepared when their owner is created;
/// no driver-specific scope enumeration is required or used.
///
/// # Errors
/// Returns a collection validation, scope binding, or preparation error.
/// Preparation is idempotent and may be retried on the next boot.
pub async fn prepare_and_migrate(
    backend: &Arc<dyn StorageBackend>,
) -> Result<BootReport, StorageError> {
    let owners = super::schema::collection_owners()?;
    let scopes = [Scope::new("local")?, Scope::new("cluster")?];
    let mut report = BootReport::default();
    for scope in scopes {
        let storage = backend.for_scope(&scope)?;
        for owner in &owners {
            for collection in &owner.collections {
                storage.ensure_collection(collection).await?;
                report.collections += 1;
            }
        }
        report.scopes += 1;
    }
    tracing::info!(
        target: "openhuman::storage",
        scopes = report.scopes,
        collections = report.collections,
        "[storage][boot] owner collections prepared"
    );
    Ok(report)
}

/// Prepares a tenant or module scope before its owner starts writing there.
///
/// # Errors
/// Returns the first owner declaration or backend error.
pub async fn prepare_scope(storage: &Storage) -> Result<usize, StorageError> {
    let owners = super::schema::collection_owners()?;
    let mut count = 0;
    for owner in &owners {
        for collection in &owner.collections {
            storage.ensure_collection(collection).await?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
#[path = "boot_tests.rs"]
mod tests;
