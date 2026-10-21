//! Storage connectivity and interrupted-layout checks.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tinystoragedrivers::StorageConfig;

use crate::doctor::contract::{DoctorCheck, DoctorCtx, DoctorTier, Finding};

pub(super) fn checks() -> Vec<Arc<dyn DoctorCheck>> {
    vec![Arc::new(StorageConnect), Arc::new(StorageLayout)]
}

struct StorageConnect;

#[async_trait]
impl DoctorCheck for StorageConnect {
    fn id(&self) -> &str {
        "storage/connect"
    }

    fn category(&self) -> &str {
        "storage"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Connect
    }

    fn is_blocking(&self) -> bool {
        true
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        self.detect_blocking(ctx).unwrap_or_default()
    }

    fn detect_blocking(&self, ctx: &DoctorCtx) -> Option<Vec<Finding>> {
        let Some(url) = crate::storage::url_from(None, ctx.config()) else {
            return Some(vec![Finding::skipped(
                self.id(),
                "Storage is not configured for this selected profile.",
                "Configure storage for the selected profile and rerun doctor.",
            )]);
        };
        let parsed = match StorageConfig::parse(&url) {
            Ok(parsed) => parsed,
            Err(_) => {
                return Some(vec![Finding::error(
                    self.id(),
                    "The selected storage configuration is invalid.",
                    "Review the storage URL in the selected configuration.",
                )]);
            }
        };
        let path = match parsed {
            StorageConfig::Sqlite { path } => path,
            StorageConfig::Memory => {
                return Some(vec![Finding::pass(
                    self.id(),
                    "The selected in-memory storage driver is available.",
                )]);
            }
            StorageConfig::MongoDb { .. }
            | StorageConfig::File { .. }
            | StorageConfig::Composite { .. } => {
                return Some(vec![Finding::skipped(
                    self.id(),
                    "The selected storage driver has no safe read-only connectivity probe.",
                    "Use a storage driver with a read-only doctor probe or run its own connectivity check.",
                )]);
            }
            _ => {
                return Some(vec![Finding::skipped(
                    self.id(),
                    "The selected storage driver has no safe read-only connectivity probe.",
                    "Use a storage driver with a read-only doctor probe or run its own connectivity check.",
                )]);
            }
        };
        match tinystoragedrivers::sqlite::SqliteStorage::probe_read_only(&path) {
            Ok(_) => Some(vec![Finding::pass(
                self.id(),
                "The selected SQLite storage database is readable.",
            )]),
            Err(_) => Some(vec![Finding::error(
                self.id(),
                "The selected SQLite storage database could not be read.",
                "Check that the configured database exists, is readable, and is not busy.",
            )]),
        }
    }
}

struct StorageLayout;

#[async_trait]
impl DoctorCheck for StorageLayout {
    fn id(&self) -> &str {
        "storage/layout"
    }

    fn category(&self) -> &str {
        "storage"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    fn is_blocking(&self) -> bool {
        true
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        self.detect_blocking(ctx).unwrap_or_default()
    }

    fn detect_blocking(&self, ctx: &DoctorCtx) -> Option<Vec<Finding>> {
        let Some(target) = crate::storage::layout::Target::from_config(ctx.config()) else {
            return Some(vec![Finding::pass(
                self.id(),
                "The selected configuration has no desktop layout migration state.",
            )]);
        };
        match crate::storage::layout::state::LayoutState::load(&target.profile_dir) {
            Ok(Some(state)) => Some(vec![Finding::new(
                self.id(),
                crate::doctor::FindingSeverity::Warn,
                "A storage layout migration is incomplete.",
                "Stop OpenHuman and resume the storage layout migration from Settings or the CLI.",
            )]),
            Ok(None) => Some(vec![Finding::pass(
                self.id(),
                "No interrupted storage layout migration was found.",
            )]),
            Err(_) => Some(vec![Finding::error(
                self.id(),
                "The storage layout migration state could not be read.",
                "Check permissions for the selected profile directory.",
            )]),
        }
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
