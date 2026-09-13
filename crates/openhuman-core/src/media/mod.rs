//! Media generation and image tool contracts.
//!
//! - [`generation`] — the `media_generate_*` agent tools (image/video via GMI,
//!   proxied through the TinyHumans backend)
//! - [`image`]      — image tool contracts scaffold (currently unwired, #2997)
//!
//! Gated by the `media` feature at the family root (`pub mod media;` in
//! `crates/openhuman-core/src/lib.rs`), because both children are wholly gated. It is a
//! **surface-only** gate: media generation is backend-proxied over the shared
//! `IntegrationClient`/`reqwest`, and [`image`] is a dependency-free contract
//! layer, so no exclusive dependency is shed. No controller/store/subscriber is
//! tagged `DomainGroup::Media` — this family is agent-tools-only.

pub mod generation;
pub mod image;
