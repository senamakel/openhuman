//! Capability advertisement for the pinned `tinymemory` module release.
//!
//! See [`super`] for why capabilities are answered statically rather than
//! discovered from the loaded module on every call.

use tinymemory_api::capabilities::{Capabilities, Capability};

/// The release whose capability set [`ARTIFACT_CAPABILITIES`] was read from.
///
/// Checked against the registry pin by `the_capability_list_matches_the_pinned_release`,
/// so bumping the pin without re-reading the list is a red test rather than a
/// silent over-claim.
pub(crate) const ARTIFACT_CAPABILITIES_PIN: &str = "1.16.0";

/// The capability families the **pinned artifact** actually serves.
///
/// Deliberately not `Capabilities::all()`. `Capability::ALL` is what the
/// *contract crate this host compiles against* declares; the loaded `cdylib` is
/// a specific release and may serve fewer families.
///
/// Re-read at tag `v1.13.3`. v1.13.0 added a `MemoryEvent` variant and two
/// additive audit fields, v1.13.1 fixed the module's source-registry path,
/// v1.13.2 fixed the `Embed` wire order, and v1.13.3 fixed folder-source path
/// resolution; none of those touched families. tinymemory#110 (in v1.13.2)
/// did add `Scoring`
/// (`ExtractEntities`, `EmbedText`, `EmbedderSlug`), which the artifact serves
/// and which `as_scoring` below forwards, so it is advertised here in the same
/// change, the way `Episodic` arrived with `as_episodic`.
///
/// Read at tag `v1.3.0`. Unchanged from v1.2.0 — the release added members
/// within existing families (`retry_failed`, the diagnostics trio,
/// `backfill_in_progress`), not families — verified with
/// `git diff v1.2.0..v1.3.0 -- crates/tinymemory-api/src/capabilities.rs`
/// returning empty. v1.2.0 is where four of the five families that v1.0.1
/// lacked arrived: `People`, `Chunks`, `Retrieval` and `Profile` all have bus
/// members there, so the under-claim that made them unreachable is over.
///
/// **`Episodic` is here in the same change that implements `as_episodic`**, as
/// the previous version of this comment required. The pinned module declares
/// the episodic methods (`InsertTurn`, `SessionTurns`, `OpenSegment`, …) and
/// [`ModuleMemoryProvider`] now forwards all of them, so the advertisement is
/// honest in both directions — the archivist writes its turns and segments
/// through this family.
///
/// **Widen this only together with the `version` bump in
/// [`super::registry`].** `the_capability_list_matches_the_pinned_release`
/// fails if the two drift.
pub(crate) const ARTIFACT_CAPABILITIES: &[Capability] = &[
    Capability::Core,
    Capability::Recall,
    Capability::Ingest,
    Capability::Documents,
    Capability::Tree,
    Capability::Entities,
    Capability::Graph,
    Capability::Diff,
    Capability::Goals,
    Capability::ToolMemory,
    Capability::Sources,
    Capability::Maintenance,
    Capability::Portability,
    // Arrived in v1.2.0. Verified against the module's declared `methods` list
    // at that tag rather than against the contract crate, which is always ahead
    // of whatever is pinned.
    Capability::People,
    Capability::Chunks,
    Capability::Retrieval,
    Capability::Profile,
    Capability::Episodic,
    // Arrived in v1.7.0 — the sync-execution and coding-session families that
    // let the host stop reaching into the engine for them. Verified against the
    // module's declared `methods` list at that tag, which serves all ten.
    Capability::SourceSync,
    Capability::CodingSessions,
    // Arrived in v1.13.2 (tinymemory#110): entity extraction, text embedding
    // and embedder identification, served by the module's engine and forwarded
    // by `MemoryScoring for ModuleMemoryProvider` below.
    Capability::Scoring,
    // Re-read at tag `v1.15.1` (tinymemory#142), one commit past v1.15.0. The
    // CortexDB adapter now asks `v1/scopes/list` for every scope instead of
    // accepting the engine's undocumented first fifty, which had been
    // truncating namespace enumeration — a live instance holding 93 listed 50,
    // and everything downstream of `namespace_summaries` reported success on
    // the subset. Behaviour inside a hosted adapter, not the module's surface:
    // `git diff v1.15.0..v1.15.1 -- crates/tinymemory-api/src/capabilities.rs
    // crates/tinymemory-bus/src/capabilities.rs crates/tinymemory-bus/src/names.rs`
    // is empty and `crates/tinymemory-module/` moves only its `Cargo.lock`, so
    // the module declares the same members it did and the list below is
    // unchanged — only the pin advances.
    //
    // Re-read at tag `v1.15.0` (tinymemory#141, openhuman#6025). The connector
    // sink now embeds a whole `accept_source_items` batch together and the
    // vendored engine claims a due `reembed_backfill` ahead of the extraction
    // backlog (tinycortex#168). Behaviour inside `Sources`/`Maintenance`, no
    // new bus member and no new family: `git diff v1.14.1..v1.15.0 --
    // crates/tinymemory-bus/src/capabilities.rs crates/tinymemory-bus/src/names.rs`
    // is empty, so the list below is unchanged and only the pin moves.
    //
    // Re-read at tag `v1.14.1` (tinymemory#136 + #137, openhuman#6012). It adds a bus
    // *member*, `BackfillConnectorTrees`, and no capability: `Capability` is the
    // family enum, and the member is a method inside `Maintenance`, which this
    // build already advertises. `git diff v1.13.8..v1.14.1 --
    // crates/tinymemory-bus/src/capabilities.rs` is empty, so nothing below moves.
    //
    // Re-read at tag `v1.13.8` (tinymemory#134, openhuman#6007): the connector
    // sync path now routes its items into the memory-tree ingest funnel, and
    // `forget_source` sweeps the per-item tree rows it creates. Behaviour inside
    // `Sources`/`Maintenance`, not a new family —
    // `git diff v1.13.7..v1.13.8 -- crates/tinymemory-api/src/capabilities.rs`
    // returns empty, so the list below is unchanged and only the pin moves.
    //
    // v1.13.7 (tinymemory#125 + #127): the typed ingestion round and the
    // answer surface, served and advertised by the pinned artifact.
    Capability::DocumentIngest,
    Capability::ConversationIngest,
    Capability::LearningIngest,
    Capability::EventIngest,
    Capability::Answer,
];

/// Escape hatch for a locally-built module.
///
/// Set `OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES=1` when the loaded
/// library was built from `vendor/tinymemory/crates/tinymemory-module` rather
/// than downloaded from the pinned release — that build serves the whole
/// contract, and pinning it to the older list would hide families it does have.
/// Deliberately **not** keyed off `TINYMEMORY_TEST_MODULE`: CI sets that to the
/// downloaded `v1.0.1` artifact, so keying off it would switch the guard off in
/// exactly the lane that must exercise it.
pub(crate) fn assume_full_capabilities() -> bool {
    matches!(
        std::env::var("OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// The set this build will advertise for the module driver.
pub(crate) fn artifact_capabilities() -> Capabilities {
    capabilities_for(assume_full_capabilities())
}

/// The advertised set for a given override state.
///
/// Split out from [`artifact_capabilities`] so the pinned-artifact invariants
/// can be asserted on the `false` branch directly. Reading the environment
/// inside the assertion would make those tests fail for anyone who has
/// `OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES=1` exported — a documented,
/// supported configuration — and mutating the variable from a test would race
/// the rest of the binary.
pub(crate) fn capabilities_for(assume_full: bool) -> Capabilities {
    if assume_full {
        return Capabilities::all();
    }
    ARTIFACT_CAPABILITIES.iter().copied().collect()
}

/// Whether the pinned artifact serves `capability`. Drives the optional
/// `as_*()` accessors so they agree with [`artifact_capabilities`].
pub(crate) fn artifact_serves(capability: Capability) -> bool {
    assume_full_capabilities() || ARTIFACT_CAPABILITIES.contains(&capability)
}
