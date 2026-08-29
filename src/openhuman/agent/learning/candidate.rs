//! Learning candidates — observations the stability detector may promote into
//! durable profile facts.
//!
//! The definitions **moved to `tinymemory_core::learning_candidate`** during
//! the memory extraction. The queue is a process-global that the memory side
//! *pushes into* — the Composio provider-profile sync emits an identity
//! candidate on every run — while the agent's learning loop drains it. Two
//! copies of that global, one per crate, would mean the producer and the
//! consumer never seeing the same queue.
//!
//! It could not go in `tinymemory-api`, which stays dependency-light;
//! the queue needs `parking_lot`. [`EvidenceRef`] *is* in the contract crate,
//! because the memory store persists it — see
//! [`tinymemory_api::host::EvidenceRef`].
//!
//! Every existing `agent::learning::candidate::…` path keeps resolving.
//!
//! # The two halves, named apart (#5560)
//!
//! The glob below carried both, which made a contract type and a process-global
//! look like one engine import. They are not the same ask:
//!
//! - the **taxonomy** ([`FacetClass`], [`CueFamily`], [`LearningCandidate`],
//!   [`EvidenceRef`]) is defined in `tinymemory-api` and merely re-exported by
//!   the engine crate, so naming the contract is the same item under a shorter
//!   chain, and these paths survive `tinymemory-core` leaving the build with no
//!   edit at any of the ~30 call sites that spell them;
//! - the **buffer** ([`Buffer`], `global()`) is a `static`, and a `static` is
//!   not a payload. It stays where it is defined.

// The taxonomy, on the contract crate. Named on `tinymemory_api` rather than
// through `memory::api`, which deliberately re-exports only what crosses the
// module bus — `learning` does not (see `memory::api`'s docs), and the whole
// point of this file is that the *producer* on the far side of that bus and the
// consumer here are not sharing a queue.
pub use tinymemory_api::host::EvidenceRef;
pub use tinymemory_api::learning::{CueFamily, FacetClass, LearningCandidate};

// The buffer, still the engine's. `tinymemory-api` is dependency-light by
// contract and the queue needs `parking_lot`, so this cannot follow the types
// down — and it is the line that keeps this file on the direct-reference
// allowlist. An explicit `use` shadows a glob, so the four names above win over
// the identical ones this would supply.
pub use tinymemory_core::learning_candidate::*;
