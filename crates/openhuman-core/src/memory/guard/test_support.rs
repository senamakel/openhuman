//! A recording fake [`MemoryProvider`] for the guard's tests.
//!
//! `NullMemoryProvider` cannot serve here: it advertises only the mandatory
//! three and returns `None` from every `as_*` accessor, so a guard built over
//! it would have no family decorators at all — which is precisely what the
//! interesting tests are about. This fake implements **all thirteen** families
//! and records what actually reached it, so a test can assert both "the driver
//! saw the value the guard rewrote" and "the driver saw nothing at all".

#![cfg(test)]

mod fixtures;

mod core_and_docs_impls;
mod provider_and_sync_impls;
mod retrieval_and_ingest_impls;

pub use fixtures::{
    document, embedded_policy, entry, export_record, external_policy, guarded, guarded_with,
    namespace_hit, namespace_summary, RecordingProvider,
};
