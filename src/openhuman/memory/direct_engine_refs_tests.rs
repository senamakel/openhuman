//! Enforcement lint: the set of production files that call `tinymemory-core`
//! **directly**, around the module seam, must not grow.
//!
//! # Why a second path to memory is a correctness problem, not a size problem
//!
//! `memory::binding` says it plainly: "the built-in driver is the compiled
//! TinyMemory TinyBus module. The host no longer exposes an embedded engine
//! class for memory." Every call that goes over that bus is round-tripped
//! through [`crate::openhuman::memory::api::wire`]'s error table — the one
//! `modules/memory.rs` keeps shared "because reimplementing the mapping here is
//! what would let a `PathEscape` arrive as an `Invalid`, silently reclassifying
//! a sandbox escape as a caller mistake" — and is filtered by the capability
//! set `ModuleMemoryProvider::verify` cross-checks against the module's own
//! answer.
//!
//! A direct `tinymemory_core::…` call gets neither. It is a second, unpoliced
//! door into the same subsystem, and two doors into one capability is a
//! capability whose behaviour can diverge. `tinymemory-core` also stays linked
//! into the shipped binary — 1.44 MB of `.text`, the 7th largest crate — for as
//! long as any of these remain, which is the visible symptom rather than the
//! disease (#5560).
//!
//! # This lint is a ratchet, not an invariant
//!
//! Same shape and same reasoning as [`super::bypass_allowlist_tests`], and as
//! `INTENTIONALLY_NOT_FORWARDED` in `scripts/lib/feature-forwarding.mjs`: the
//! current direct callers are enumerated in [`ALLOWED`] with a classification
//! and a reason each, and **that list may shrink but must never grow**. A lint
//! that was red on day one would be `#[ignore]`d within a week; a green ratchet
//! converges.
//!
//! # The classification, and why most of the list cannot move yet
//!
//! Each entry carries a [`Verdict`], which is the inventory the migration is
//! driven from:
//!
//! - [`Verdict::SeamExpressible`] — the existing `MemoryProvider` surface
//!   already covers this. These are the ones to migrate; a non-empty set here
//!   is a to-do list, not a steady state.
//! - [`Verdict::NeedsWiderSeam`] — the call wants something the thirteen
//!   capability families do not expose. **These are blocked upstream, not
//!   here.** `modules::registry` pins the TinyMemory module to a released,
//!   SHA-256-verified artifact (v1.0.1 at the time of writing), so a new bus
//!   method is a `tinymemory` release plus a registry re-pin before it is a
//!   host change. Adding the trait method alone would produce a driver that
//!   answers `Unsupported` — strictly worse than the direct call it replaced,
//!   because the failure moves from compile time to run time.
//! - [`Verdict::HostSide`] — not a driver call at all. Re-export shims,
//!   host-seam installation, and inert type imports. These are correct as they
//!   stand and are counted only so "deliberate" stays distinguishable from
//!   "forgotten".
//!
//! ## The concrete gaps, for whoever picks the upstream work up
//!
//! **This list was drained on 2026-08-23 and the shape of the problem changed.**
//! It used to enumerate four things the seam could not express — retrieval
//! filters, chunk reads, an entity-kind filter, and source listing — plus the
//! people domain and the `source_scope` task-local. Every one of those now has
//! a home:
//!
//! - **Retrieval filters, chunk reads, entity-kind search** — `MemoryRetrieval`
//!   (`fast_retrieve`, `cover_window`, `retrieve_source`, `retrieve_children`,
//!   `retrieve_leaves`, `recall_namespace_scored`, `search_entities`) and
//!   `MemoryChunks` (`list_chunks`, `get_chunk`, `chunk_detail`,
//!   `storage_kinds`, `chunk_embeddings`).
//! - **The people domain** — `MemoryPeople`, seven methods.
//! - **Profile/facets** — `MemoryProfile`, eleven methods, which
//!   `memory::guard`'s docs separately described as a missing "fourteenth
//!   family".
//! - **`source_scope`** — not a seam gap at all. It is host policy; it lives in
//!   `memory::source_scope`, and the scope crosses the bus as a `SourceScope`
//!   value on every scoped method.
//!
//! `ModuleMemoryProvider` implements all of these bar `as_episodic`, and
//! `MemoryGuard` wraps all fifteen families.
//!
//! **So what blocks the migration is release lag, not seam width.**
//! `modules::registry` pins a SHA-256-verified artifact, and the five families
//! above shipped in no release until v1.2.0. Before migrating a call site onto
//! one, check the *tag* rather than the vendored source — the submodule is
//! routinely ahead of what is pinned. A method the pinned artifact does not
//! serve answers `Unsupported` at run time, which is strictly worse than the
//! direct call it replaced.
//!
//! What genuinely has no bus representation yet, and is the next upstream ask:
//! the ingest `queue`, the `chat` runtime seam, the composio sync pipelines,
//! and **recency recall**. The first three live inside `tinymemory-core` and
//! would each need a design pass, not just a trait method.
//!
//! Recency recall is the subtle one, and it is worth spelling out because the
//! obvious migration is wrong. `MemoryRetrieval::recall_namespace_scored` looks
//! like the twin for `memory.recall_context` and `memory.recall_memories`, and
//! it is not:
//!
//! - `recall_namespace_scored` resolves to
//!   `query_namespace_hits_excluding_session(ns, query, limit, exclude)` — the
//!   **query-ranked** path.
//! - Both handlers call `recall_namespace_memories(ns, limit)` — a distinct
//!   **recency** path. `recall_namespace_context_data` is just that plus a
//!   rendered `context_text` wrapper.
//!
//! Passing an empty query to the scored method does not degrade to recency; it
//! runs the ranking path with nothing to rank against. The two share a prefix
//! (`load_documents_for_scope` + `kv_records_for_scope`) and diverge after it,
//! so the swap compiles, returns plausible hits, and quietly changes what the
//! user gets back. `memory.query_namespace` *is* safely expressible, because it
//! has a real query — pass `exclude_session_id: None` there to preserve the
//! current no-exclusion behaviour, since an RPC handler is not an agent turn.
//!
//! The upstream ask is a `RecallNamespaceRecent`-shaped method on
//! `MemoryRetrieval`.
//!
//! # Known weaknesses, stated rather than hidden
//!
//! - **The lint sees text, not types.** A reference reached through a
//!   re-export under another name is invisible to it — and the memory tree is
//!   full of those on purpose: `memory/mod.rs` re-exports twenty-five engine
//!   modules, and ~687 `memory::store::…` / `memory::tree::…` paths elsewhere
//!   resolve into the crate through them. **This lint deliberately does not
//!   count those.** It counts the sites that *name* the crate, because those
//!   are the ones a migration edits. The re-export surface is a separate,
//!   larger problem tracked in the issue, and pretending this number covers it
//!   would be the worst outcome.
//! - **By-path test files are out of scope** (`*_tests.rs`, `tests.rs`,
//!   `test_support/`), matching the sibling lint. Several inline
//!   `#[cfg(test)]` modules do name the crate (`query::drill_down`,
//!   `query::fetch_leaves`, `query::query_source` each assert a tool's result
//!   against a direct engine call); those files are listed, and the entry says
//!   so.
//! - **Comment lines are skipped**, so the many doc comments that reference
//!   `tinymemory_core::…` by path do not inflate the count.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Why a file may name the engine crate today.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Verdict {
    /// The existing `MemoryProvider` surface covers this. Migrate it.
    SeamExpressible,
    /// Blocked on a wider bus surface, which means an upstream `tinymemory`
    /// release and a `modules::registry` re-pin.
    NeedsWiderSeam,
    /// Not a driver call: a re-export shim, host-seam installation, or an
    /// inert type import.
    HostSide,
    /// The file's engine dependency **predates this branch** and was hidden
    /// behind `memory/mod.rs`'s re-export facade; deleting that facade made it
    /// textually visible to this lint without changing what the file does.
    ///
    /// It is a deliberately unflattering label. These are not audited, and the
    /// bucket exists so that "nobody has looked at this yet" cannot be mistaken
    /// for one of the three considered verdicts above. Draining it means
    /// re-classifying each entry as one of those three, not deleting the
    /// variant.
    FacadeRevealed,
}

/// The literal this lint searches for. A single needle, deliberately: the
/// question is "does this file name the engine crate", not "which item".
const NEEDLE: &str = "tinymemory_core::";

/// `(repo-relative path, verdict, why it names the engine today)`.
///
/// Adding an entry is a decision, not a way to silence the lint. Sorted by
/// path — [`scan`] returns a `BTreeSet`, so keeping the literal in the same
/// order makes diffs readable.
const ALLOWED: &[(&str, Verdict, &str)] = &[
    // ── Two reveals, one category ───────────────────────────────────────────
    //
    // Most `FacadeRevealed` entries below were surfaced by two changes that
    // *renamed* a dependency rather than adding one. **Neither grew the number
    // of real engine dependencies by a single call.**
    //
    // 1. **Deleting the `memory/mod.rs` re-export facade.** This lint was
    //    calibrated against a tree where that module re-exported ~24 engine
    //    names, so a file writing `crate::openhuman::memory::UnifiedMemory` did
    //    not match the `tinymemory_core::` needle. Those files were direct
    //    engine users the whole time; the facade just spelled the dependency
    //    differently.
    //
    // 2. **The `memory` Cargo gate's retarget.** Gating the family meant that
    //    always-compiled callers reaching the engine *through* a `memory::…`
    //    shim — the Composio provider catalogue, the conversation store
    //    `threads` uses, the `rpc_models` request types — had to name the crate
    //    directly or stop compiling with the gate off. Routing an always-on
    //    caller through a gated shim to reach an ungated crate was the bug; the
    //    retarget is the fix, and it makes twenty files honest about a
    //    dependency they already had.
    //
    // Both are `FacadeRevealed` rather than one of the three considered
    // verdicts because they have not been audited individually. See the note on
    // that variant. The reason string on each entry says which reveal it came
    // from.
    (
        "src/bin/library_profile/scenarios/cold_phases.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/bin/library_profile/scenarios/memory_ingest.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/core/memory_cli.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/core/runtime/context.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/core/runtime/services.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/lib.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/experience/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/experience/store.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/hook_impl.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/lifecycle.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/recap.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/test_constructors.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/tree_ingest.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/archivist/types.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/artifact_offload/policy.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/session/builder/factory.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/session/turn/context.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/session/turn/core.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/subagent_runner/ops/runner.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/harness/tool_result_artifacts/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/learning/candidate.rs",
        Verdict::HostSide,
        "re-export shim for learning_candidate types",
    ),
    (
        "src/openhuman/agent/learning/linkedin_enrichment.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/learning/startup.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/tinyagents/host/agent_memory.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/tinyagents/thread_context.rs",
        Verdict::HostSide,
        "re-export shim for the thread-id task-local",
    ),
    (
        "src/openhuman/agent/tools/remember_preference.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/agent/tools/save_preference.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/channels/controllers/ops/connect.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/channels/runtime/startup.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/channels/tests/memory.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/config/migration_helpers/core.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/config/ops/model.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/config/schema/types.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/desktop/app_state/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/desktop/provider_surfaces/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/desktop/provider_surfaces/schemas.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/flows/builder_tools.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/flows/memory_tools.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/flows/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/flows/tinyflows/caps/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/flows/tinyflows/caps/tools/composio.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/flows/tinyflows/memory_adapter.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/hosted/orchestration/effect_executor.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/inference/embeddings/provider_trait.rs",
        Verdict::HostSide,
        "re-export shim for TinyAgentsEmbeddingProvider, which cannot live host-side",
    ),
    (
        "src/openhuman/inference/embeddings/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/integrations/composio/catalog.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/composio/identity.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/composio/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/composio/ops/memory_cleanup.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/integrations/composio/ops/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/integrations/composio/ops/providers_ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/integrations/composio/providers/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/composio/schemas.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/filter.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/pipeline.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/store.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/integrations/task_sources/types.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/memory/conversations/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::conversations::*",
    ),
    (
        "src/openhuman/memory/diff/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::diff::*",
    ),
    (
        "src/openhuman/memory/guard/policy.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/host_impls.rs",
        Verdict::HostSide,
        "installs the eight host seams (embedding, chat, composio, config, nlp, scheduler gate, shutdown, error reporter); mirrored over the bus by modules/memory_host.rs",
    ),
    (
        "src/openhuman/memory/mod.rs",
        Verdict::HostSide,
        "the re-export block itself — twenty-five engine modules under their historical paths",
    ),
    (
        "src/openhuman/memory/ops/documents.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/ops/guard.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/ops/helpers.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/ops/learn.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/ops/sync.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/ops/test_support.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/people/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::people::*",
    ),
    (
        "src/openhuman/memory/read_rpc/admin.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/read_rpc/chunks.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/read_rpc/entities.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/read_rpc/graph.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/read_rpc/mod.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/read_rpc/vault.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sources/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sources::*",
    ),
    (
        "src/openhuman/memory/sources/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sources/schemas.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/store_golden.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sync/composio/bus.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sync/composio/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sync::composio::*",
    ),
    (
        "src/openhuman/memory/sync/composio/providers/context_ext.rs",
        Verdict::NeedsWiderSeam,
        "extends the engine's ProviderContext; the sync pipeline is engine-internal and has no capability family",
    ),
    (
        "src/openhuman/memory/sync/composio/providers/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sync::composio::providers::*",
    ),
    (
        "src/openhuman/memory/sync/composio/providers/slack/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sync::composio::providers::slack::*",
    ),
    (
        "src/openhuman/memory/sync/composio/providers/slack/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sync/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sync::*",
    ),
    (
        "src/openhuman/memory/sync/sync_status/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::sync::sync_status::*",
    ),
    (
        "src/openhuman/memory/sync/sync_status/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/sync_events_bridge.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tool_memory/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tool_memory::*",
    ),
    (
        "src/openhuman/memory/tools/diff.rs",
        Verdict::NeedsWiderSeam,
        "sources::{get_source, list_sources}; MemorySourceSink is accept_source_items + forget_source, with no list door",
    ),
    (
        "src/openhuman/memory/tools/flavour.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tools/forget.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tools/recall.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tools/store.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tree/health/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tree::health::*",
    ),
    (
        "src/openhuman/memory/tree/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tree::*",
    ),
    (
        "src/openhuman/memory/tree/retrieval/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tree::retrieval::*",
    ),
    (
        "src/openhuman/memory/tree/retrieval/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tree/tree/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tree::tree::*",
    ),
    (
        "src/openhuman/memory/tree/tree/rpc.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/memory/tree/tree_runtime/mod.rs",
        Verdict::HostSide,
        "re-export shim: pub use tinymemory_core::tree::tree_runtime::*",
    ),
    (
        "src/openhuman/platform/doctor/core.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/security/approval/store.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/security/credentials/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/threads/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/threads/schemas.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/threads/tools.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
    (
        "src/openhuman/tools/registry/ops.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the facade deletion made it visible",
    ),
    (
        "src/openhuman/web_chat/run_task.rs",
        Verdict::FacadeRevealed,
        "engine dependency predates this branch; the `memory` gate's retarget made it visible",
    ),
];

/// True for source files the lint deliberately does not scan.
///
/// By-path only, matching [`super::bypass_allowlist_tests`] — see that module
/// for why inline `#[cfg(test)]` blocks are left in scope rather than
/// brace-tracked.
fn is_test_path(path: &Path) -> bool {
    if path.components().any(|c| c.as_os_str() == "test_support") {
        return true;
    }
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name == "tests.rs" || name.ends_with("_tests.rs"),
        None => false,
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") && !is_test_path(&path) {
            out.push(path);
        }
    }
}

/// Every repo-relative path in this crate's `src` that names the engine crate
/// outside a comment.
fn scan() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);

    let mut found = BTreeSet::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        for line in text.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(NEEDLE) {
                found.insert(rel.clone());
                break;
            }
        }
    }
    found
}

fn allowed_set() -> BTreeSet<String> {
    ALLOWED
        .iter()
        .map(|(path, _, _)| (*path).to_string())
        .collect()
}

fn render(paths: impl IntoIterator<Item = String>) -> String {
    paths.into_iter().map(|p| format!("\n  {p}")).collect()
}

/// A scanner that silently found nothing would turn every other test here into
/// a rubber stamp, so refuse to pass vacuously.
///
/// `memory/mod.rs` is the most stable pin available: it is the re-export block
/// itself, so it names the crate by construction. If the scanner stops seeing
/// it, the scanner is broken — fix it, do not relax this assertion.
#[test]
fn direct_reference_scanner_is_not_vacuous() {
    let found = scan();
    assert!(
        found.contains("src/openhuman/memory/mod.rs"),
        "scanner found no direct engine reference in memory/mod.rs, which is the re-export block; \
         the scanner is broken"
    );
    assert!(
        found.len() > 20,
        "scanner found only {} files; expected the full direct-reference surface",
        found.len()
    );
}

/// **The ratchet.** A new file naming `tinymemory_core::` fails here.
///
/// If the new call is genuinely unavoidable, add it to [`ALLOWED`] with a
/// [`Verdict`] and a reason. If it is not, route it through
/// `CoreContext::memory()` and the `MemoryProvider` seam.
#[test]
fn no_new_files_call_the_engine_directly() {
    let found = scan();
    let allowed = allowed_set();
    let unexpected: Vec<String> = found.difference(&allowed).cloned().collect();
    assert!(
        unexpected.is_empty(),
        "new direct `tinymemory_core::` reference(s) — route these through the MemoryProvider seam, \
         or add them to ALLOWED with a Verdict and a reason:{}",
        render(unexpected)
    );
}

/// The staleness half. An allowlist that outlives its entries rots into dead
/// strings that document nothing — the same failure `INTENTIONALLY_NOT_FORWARDED`
/// guards against. A migrated file must be *removed* from the list, so the
/// count is always the real one.
#[test]
fn allowlist_has_no_stale_entries() {
    let found = scan();
    let allowed = allowed_set();
    let stale: Vec<String> = allowed.difference(&found).cloned().collect();
    assert!(
        stale.is_empty(),
        "ALLOWED names file(s) that no longer reference the engine — delete these entries so the \
         ratchet reflects reality:{}",
        render(stale)
    );
}

/// Every entry carries a reason, and no path is listed twice. A blank reason is
/// an allowlist entry that documents nothing, which is what the list exists to
/// prevent.
#[test]
fn allowlist_entries_are_well_formed() {
    let mut seen = BTreeSet::new();
    for (path, _, reason) in ALLOWED {
        assert!(
            !reason.trim().is_empty(),
            "{path} is allowlisted with no reason"
        );
        assert!(
            seen.insert(*path),
            "{path} is listed twice; one entry per file"
        );
    }
}

/// The migration to-do list must be empty, and stay empty by being *worked*
/// rather than re-labelled.
///
/// A [`Verdict::SeamExpressible`] entry says "the seam already covers this and
/// nobody moved it". That is a bug with a known fix, so it fails here rather
/// than sitting in a list nobody reads. Downgrading an entry to
/// [`Verdict::NeedsWiderSeam`] to silence this is the one edit that would make
/// the lint lie — [`no_new_files_call_the_engine_directly`] would still pass,
/// and the gap would vanish from view.
#[test]
fn nothing_is_left_migratable() {
    let pending: Vec<&str> = ALLOWED
        .iter()
        .filter(|(_, verdict, _)| *verdict == Verdict::SeamExpressible)
        .map(|(path, _, _)| *path)
        .collect();
    assert!(
        pending.is_empty(),
        "these files can already be expressed through MemoryProvider and should be migrated: {pending:?}"
    );
}

/// The blocked set is the upstream ask, so it must be non-empty for as long as
/// the engine is still linked — and empty when it is not.
///
/// This is the test that makes "`tinymemory-core` left the build" self-proving:
/// on the day the crate is dropped, [`scan`] returns nothing, `ALLOWED` empties,
/// and this assertion is what forces the module docs above to be rewritten
/// rather than left describing a world that no longer exists.
#[test]
fn the_blocked_set_matches_the_engine_still_being_linked() {
    let blocked = ALLOWED
        .iter()
        .filter(|(_, verdict, _)| *verdict == Verdict::NeedsWiderSeam)
        .count();
    let host_side = ALLOWED
        .iter()
        .filter(|(_, verdict, _)| *verdict == Verdict::HostSide)
        .count();
    assert!(
        blocked > 0 || host_side > 0,
        "nothing references tinymemory-core any more — drop the path dependency from Cargo.toml, \
         remove its cargo-machete `ignored` entry, ratchet scripts/kernel-floor.limits, and rewrite \
         this module's docs (#5560)"
    );
}
