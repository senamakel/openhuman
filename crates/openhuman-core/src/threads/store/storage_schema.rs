//! The `threads` owner's storage schema: what the host keeps about a
//! conversation beside the conversation records TinyAgents owns
//! (`conversations.*`).
//!
//! | v1 | v2 | scope |
//! |---|---|---|
//! | `threads_transcript_keys` (id = thread id) | `threads.transcript_keys` (id `Key::of([thread_id])`) | tenant |
//!
//! The importer marker `threads_import` is not a collection any more: the
//! legacy JSONL import is a `tsd.migrations` entry
//! ([`crate::storage::import_state`]).

use anyhow::Result;
use tinystoragedrivers::CollectionSpec;

use crate::storage::schema_support::{
    key_of, map_legacy_collection, LegacySourceRecord as SourceRecord,
    LegacyTargetRecord as TargetRecord,
};

pub(super) const TRANSCRIPT_KEYS: &str = "threads.transcript_keys";
pub(super) const LEGACY_TRANSCRIPT_KEYS: &str = "threads_transcript_keys";
/// Converts legacy transcript-key rows into the current owner shape.
pub(crate) fn upgrade_records(
    records: Vec<crate::storage::import::Record>,
) -> Result<Vec<crate::storage::import::Record>> {
    map_legacy_collection(records, LEGACY_TRANSCRIPT_KEYS, transcript_keys_v2)
}

pub(super) fn spec() -> CollectionSpec {
    CollectionSpec::new(TRANSCRIPT_KEYS)
}

/// Collections owned by the thread host alongside TinyAgents' transcript store.
pub(crate) fn collection_specs() -> Vec<CollectionSpec> {
    vec![spec()]
}

fn transcript_keys_v2(batch: Vec<SourceRecord>) -> Result<Vec<TargetRecord>> {
    batch
        .into_iter()
        .map(|record| {
            Ok(
                TargetRecord::new(TRANSCRIPT_KEYS, key_of(&record.id)?, record.body)
                    .created_ms(record.meta.created_ms),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "storage_schema_tests.rs"]
mod tests;
