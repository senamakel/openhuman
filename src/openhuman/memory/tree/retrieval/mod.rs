//! Host layer over [`tinymemory_core::tree::retrieval`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::tree::retrieval::…` path resolving.

// Engine-owned, and the names are a trap worth naming: `RetrievalHit`,
// `QueryResponse`, `NodeKind` and `EntityMatch` here are **not** the contract's
// `RetrievalHit`, `RetrievalResponse`, `RetrievalNodeKind` and `EntityMatch`.
// Repointing this shim at `memory::api::provider` is a wire change and not a
// path swap. Probe the item identity before assuming a carve-out is free.
//
// **The field that used to prove that has changed; the conclusion has not.**
// This comment said the contract's hit "has no field for" `tree_kind`. It has
// one as of the pinned v1.7.0 — `tree_kind: Option<String>`, `#[serde(default,
// skip_serializing_if)]` — so re-derive the difference from the types rather
// than from this sentence. What still separates them, checked against
// `tinymemory-bus`'s `provider::retrieval` and tinycortex's
// `memory::retrieval::types`:
//
// - `tree_kind` is an **open string** on the contract and the closed
//   `TreeKind` enum on the engine, so a hit crossing between them needs a
//   conversion that can fail in one direction and is lossy in neither;
// - the engine's hit is non-optional there — a bare leaf reports
//   `TreeKind::Source` via `leaf_tree_placeholder`, where the contract encodes
//   the same absence as `None`. Those are different answers to "which tree did
//   this come from", and a swap would silently relabel every unsealed leaf.
//
// So this is still a wire change, and one whose translation has to be written
// down at the handler rather than assumed by the type system.
//
// What keeps this line alive (#5560): `fast_retrieve` + `FastRetrieveOptions`
// (memory::agent::ops, memory::schema::handlers), `types::{NodeKind,
// QueryResponse, RetrievalHit}` (read_rpc::chunks, the sub-agent runner),
// `source::{SourceQuery, query_source_scoped}` (read_rpc::chunks) and
// `search::search_entities` — plus `{query_source, search_entities}` from the
// memory e2e suites. Every one names a shape the contract does not carry, so
// this is the same wire-change ask as the paragraph above, not a routing one.
pub use tinymemory_core::tree::retrieval::*;

pub mod rpc;
pub mod schemas;

/// Chunk-staging fixtures for [`rpc`]'s inline tests.
///
/// Test-only, and in a directory both memory lints skip by path — see the
/// module's own docs for why a genuinely dev-only engine reference had to move
/// out of the inline `#[cfg(test)]` block to be classified as one.
#[cfg(test)]
pub(crate) mod test_support;

// The controller aggregators this domain's RPC surface defines. Aliased
// exactly as the pre-extraction module exported them.
pub use schemas::{
    all_controller_schemas as all_retrieval_controller_schemas,
    all_registered_controllers as all_retrieval_registered_controllers,
};
