# Retiring `tinycortex` from OpenHuman — the remaining surface

*Status: inventory, 2026-08-23. Companion to
[`2026-08-13-memory-module-port.md`](2026-08-13-memory-module-port.md).*

## What this document is for

The goal is that OpenHuman links **only `tinymemory-api`** and reaches memory
over the TinyMemory TinyBus module, so `tinycortex` and `tinymemory-core` both
leave the build and `vendor/tinycortex` stops being a submodule of this repo.

Two rounds of that work have landed. This document is the honest remainder: what
is left, why each item is left, and which repository has to move first. It exists
because the previous round's note — recorded in
`memory/direct_engine_refs_tests.rs` — concluded that *"what blocks the migration
is release lag, not seam width"*. **That was true of the five families v1.2.0
shipped and is not true in general.** For the rest, the seam genuinely does not
exist, and re-deriving that finding costs a day.

## Where the number stands

Measured on this branch, production files only (`*_tests.rs`, `tests.rs`,
`test_support/` and `src/bin/` excluded):

| | files | references |
| --- | ---: | ---: |
| name `tinymemory_core::` | 98 | ~360 |
| name `tinycortex::` / `tinycortex_api::` directly | 41 | ~130 |

The two overlap: `tinymemory_core::tinycortex::…` is a re-export, so a file can
appear in both.

## The rule that decides where each item goes

A capability belongs **on the bus** when it needs the store. It belongs **in the
host** when it is arithmetic, policy, or vocabulary the caller already holds.
Getting this backwards in either direction is expensive:

- Routing host arithmetic over the bus buys a serialization hop and nothing else.
- Leaving store access host-side keeps a second, unpoliced door into the
  subsystem — the argument `direct_engine_refs_tests` opens with.

Two items have already moved the host way rather than the bus way, and they are
the pattern to copy: `util::redact` (a six-line SHA-256 log redactor) and
`memory::ranking` (`hybrid_score`, `WeightProfile`, `cosine_similarity`,
`mmr_select`). Both are ported verbatim with parity tests pinned to **literal**
expected values, never to a call back into the engine — a test written the other
way stops compiling on exactly the day it would matter.

## A. Needs new bus surface — upstream `tinymemory` first

Each of these is a `tinymemory` PR, then a release, then a `modules::registry`
re-pin, and only then a host change. Adding the trait method alone produces a
driver that answers `Unsupported`: the failure moves from compile time to run
time, which is strictly worse than the direct call it replaced.

### A1. `MemoryQueue` — the ingest job queue (~36 refs)

The largest single blocker, and the one that gates the ingest RPC surface.
Methods in use: `enqueue`, `store::count_by_status`,
`store::count_failed_unrecoverable`, `store::requeue_failed`, `drain_until_idle`,
`wake_workers`, `backfill_in_progress`, `ensure_reembed_backfill`,
`requeue_failed_after_provider_change`, `start`, plus the `NewJob` / `JobStatus`
/ `ExtractChunkPayload` / `FlushStalePayload` payload types.

Production callers: `memory/tree/tree/rpc.rs`, `memory/read_rpc/admin.rs`,
`memory/sync_events_bridge.rs`, `security/credentials/ops.rs`,
`config/ops/model.rs`, `config/migrations/migrate_legacy_embedding_provider.rs`,
`inference/embeddings/rpc.rs`, `core/runtime/services.rs`.

Note the shape: several callers *enqueue* work and then *poll* for drain. A
family that only enqueues leaves the polling half behind.

### A2. `MemoryConversations` — threads and messages (~25 refs)

`get_messages`, `list_threads`, `purge_threads`, `append_message`,
`CreateConversationThread`, `ConversationThread`, `ConversationMessage`. Today
`memory/conversations/mod.rs` is a `pub use tinymemory_core::conversations::*;`
shim, and `threads/ops.rs` and the channel adapters reach through it.

### A3. Source **registry** — widen `MemorySourceSink` (~20 refs)

`MemorySourceSink` today has exactly two methods, `accept_source_items` and
`forget_source`. The registry half — `list_sources`, `get_source`, upsert,
enable/disable — has no bus representation at all, so `memory/tools/diff.rs`,
`memory/sources/rpc.rs` and `memory/sources/reconcile.rs` cannot move.

The same module also needs the coding-session ingest surface
(`coding_session_status`, `ingest_coding_sessions`, `CodingSessionIngestRequest`
/ `Response`, `CodingSessionSourceStatus`), the sync audit log (`read_audit_log`,
`SyncAuditEntry`), and `estimate_cost_usd`.

### A4. Composio sync pipelines (~25 refs)

`run_composio_connection`, `load_composio_sync_state`,
`HOST_SYNC_STATE_NAMESPACE`, `HostSyncAdapter`, `sync::sync_status::*`,
`sync::composio::providers::{ProviderContext, ComposioUsageHandle}`. This is a
pipeline, not a getter — it needs a design pass on what crosses the bus and what
stays a host callback, not just a trait method.

### A5. `MemoryChat` — the chat host seam (~17 refs)

`chat::{ChatPrompt, ChatProvider, StaticChatProvider, build_chat_provider,
build_chat_runtime, test_override}` and `chat_host::{ChatHost, set_chat_host}`.
Consumed by the whole `agent/harness/archivist/` family. Note this is a seam the
**host implements for the engine**, so its bus direction is the reverse of the
families above — the same shape as `host::nlp` and `host::embedding_host`, which
already live in `tinymemory-api`.

### A6. Preferences (8 refs)

`load_general_preferences`, `recall_situational_preferences`,
`STANDING_PREFS_LIMIT`, `USER_PREF_GENERAL_NAMESPACE`. Read on the hot path in
`agent/harness/session/turn/{core,context}.rs`.

### A7. Recency recall on `MemoryRetrieval` (2 handlers)

Carried over verbatim from the previous round, because the obvious migration is
wrong and someone will try it again:

- `recall_namespace_scored` resolves to
  `query_namespace_hits_excluding_session(ns, query, limit, exclude)` — the
  **query-ranked** path.
- `memory.recall_context` and `memory.recall_memories` call
  `recall_namespace_memories(ns, limit)` — a distinct **recency** path.

Passing an empty query to the scored method does not degrade to recency; it runs
the ranking path with nothing to rank against. The two share a prefix
(`load_documents_for_scope` + `kv_records_for_scope`) and diverge after it, so
the swap compiles, returns plausible hits, and quietly changes what the user
gets. The ask is a `RecallNamespaceRecent`-shaped method.

### A8. Smaller, one method each

- `MemoryDiff::snapshot_count_for_source` — `memory/tools/diff.rs` opens a
  `Ledger` directly for a count today.
- Persona compilation — `compile_flavoured_root`, `flavoured_root_abs_path`,
  `PersonaFacet`, `tree::store::get_tree_by_scope`. `MemoryProfile` covers
  *facets* (`ProfileFacet`) but not rendering a flavoured root, and
  `PersonaFacet` is a different type.
- `ingest_pipeline::{ingest_chat, ingest_document, ingest_email,
  ingest_document_with_scope}` and the `ingest::canonicalize::{chat, document,
  email}` input types. `MemoryIngest` has `ingest_document` and `ingest_chat`
  over a generic `IngestItem`; there is **no `ingest_email`**, and the
  canonicaliser input shapes are the engine's.

### A9. The chunk readers need the writer, and the writer needs a wider `IngestOutcome`

**Six revisions.** The earlier five are kept below in one line each, because
every one of them reads as correct and the next person will land on one.

The symptom throughout: migrating `memory_tree_list_chunks` / `get_chunk` onto
`MemoryChunks` compiles cleanly and breaks four `ingest_document` round-trips,
which read back 0 chunks.

| # | Claim | Verdict |
| --- | --- | --- |
| 1 | "the `config` argument selects the store" | true, not the cause |
| 2 | "so honour the argument" — `guard_for_config` over `binding::for_workspace` | built it; still 0 chunks |
| 3 | "the handlers need an injection seam" | necessary, not sufficient |
| 4 | "the writer migration is a behaviour change (chunking moves into the driver)" | **false** — same pipeline both ways |
| 5 | "the blocker is email" | true but not the first blocker |
| 6 | **the blocker is `IngestOutcome`** | **this one** |

**Why the zero happens at all.** `MemorySubsystemConfig::default()` names driver
`"tinycortex"`, `binding::admit` aliases it to the `tinymemory` module, no
artifact exists in a unit test, so the binding falls back as documented and the
guard wraps the **null driver** while fixtures write through `ingest_pipeline`
to the engine's SQLite.

**Revision 4 was wrong and the correction matters**, because the expensive
version of this story would deter the work. Migrating the writer does *not*
relocate chunking or entity scoring:
`TinycortexProvider::ingest_document` calls
`tinymemory_core::ingest_pipeline::ingest_document_with_scope` — the same
pipeline the host calls today. Its `title: String::new()` drops
`DocumentInput.title`, but `canonicalise` reads `title` only in its emptiness
guard and deliberately keeps it out of the content, so the only behavioural
delta there is a document with an empty body and a non-empty title. `provider`
is inert.

**The blocker is that `IngestOutcome` is narrower than `IngestResult`.** The RPC
returns `IngestResult` (= tinycortex's `IngestSummary`) with six fields; the bus
carries three.

| `IngestResult` | `IngestOutcome` | Survives the bus? |
| --- | --- | --- |
| `chunks_written` | `written` | yes |
| `chunk_ids` | `ids` | yes |
| `source_id` | — | caller already knows it |
| `chunks_dropped` | folded into `skipped` | **only when not already-ingested** |
| `already_ingested` | folded into `skipped` | **no — becomes ambiguous** |
| `extract_jobs_enqueued` | — | **no** |

The adapter collapses two distinct facts into one number:
`skipped = if already_ingested { 1 } else { chunks_dropped }`. That is the exact
ambiguity `already_ingested` exists to prevent — `wipe_all_clears_ingest_gate`
in `read_rpc_tests.rs` pins a shipped bug where a wiped source could never
re-ingest because the gate stayed claimed, and the symptom was "0 chunks
written" that only `already_ingested` distinguished from a normal no-op.
Migrating the writer today would put that back on the wire as a public RPC
response narrowing.

**So A9 needs two upstream additions**, and it is worth grouping them into one
`tinymemory` change:

1. `IngestOutcome` gains `already_ingested` and `extract_jobs_enqueued`, and
   keeps `skipped` meaning dropped-chunks only.
2. An email ingest method — `ingest_rpc` dispatches Chat/Email/Document and
   `MemoryIngest` has only `ingest_document` and `ingest_chat`. Already listed
   here as the missing `ingest_email`.

**What is already done, and is in this PR.** The test-side half no longer
blocks anything: `memory::guard::in_memory::InMemoryChunks` +
`guarded_in_memory_chunks()` is a storing provider implementing `MemoryIngest`
and `MemoryChunks`, so a write and a read that both cross a real `MemoryGuard`
reach the same store. `InMemoryProvider` could not serve this — it implements
the mandatory three only, so `as_chunks()` is `None` and the guard answers
`Unsupported`. Three tests pin the round trip, the `source_id` dedupe, and
scope-before-limit narrowing. When the two upstream additions land, the handler
migration is a seam plus these fixtures, not a research problem.

**What stays true regardless:** do not make those four tests pass by binding a
global client first. They are the only thing that goes red when a reader moves
to the guard while the writer stays on the store.

### A10. Richer chunk listing for `read_rpc` — a design pass, not a method

`memory/read_rpc/chunks.rs` builds **raw SQL against the engine's schema**:
`mem_tree_chunks` joined to `mem_tree_entity_index`, with a `total` count
alongside the page. `ChunkQuery` has no entity filter and `list_chunks` returns
no total, so this cannot move as-is. Either the query type grows those two
things, or the read surface changes shape.

## B. Stays host-side — do not "fix" these into bus calls

- **`store::safety::{sanitize_text, has_likely_secret, sanitize_json,
  canonical_identifier}`** (11 production sites, most of them *outside* the
  memory tree — the approval store, artifact offload, tool results). These are
  `regex`-based and `tinymemory-api` forbids `regex`; the crate's manifest
  carries that invariant with a `cargo tree` guard. Repointing them at
  `tinycortex` to shrink the ratchet would move the number without moving the
  substance. They should be ported into the host the way `util::redact` was.
- **`memory::ranking`** — already done. Both search tools fetch through the
  guard first, so what remains is arithmetic over values the caller holds.
- **`memory::source_scope`** — host policy, not a seam gap. It crosses the bus
  as a `SourceScope` *value* on every scoped method, because a task-local set by
  the host cannot be read inside the module's `cdylib`, which has its own
  statics.
- **`global::{client, client_if_ready, init, active_workspace_dir}`** — the
  per-workspace singleton. Some of the 30 references are genuinely host-side
  workspace resolution and should stay; the rest should resolve through
  `binding` / `active_memory_guard` instead. This is host cleanup, not upstream
  work.

## C. Order of operations

1. **A1 (queue) and A2 (conversations)** first — they are the biggest, and they
   unblock the ingest and thread surfaces that everything else reads from.
2. **A3 + A4 together** — the source registry and the sync pipelines are one
   subsystem in practice; splitting them means two releases to get one working
   path.
3. **A5 + A6** — both are agent-harness hot-path reads.
4. **A9 needs one `tinymemory` change carrying two additions** — a wider
   `IngestOutcome` (`already_ingested`, `extract_jobs_enqueued`) and an email
   ingest method. v1.2.0 already serves the four methods themselves, so this is
   a contract widening rather than new plumbing. The host-side test fixtures
   are already in place (`guarded_in_memory_chunks`), so once it lands the
   migration is writer-plus-readers in one change.
5. **A7, A8, A10** — small or design-bound; can ride any release.
6. Only then: delete the `tinycortex` and `tinymemory-core` dependencies, drop
   `vendor/tinycortex`, and write the shed back into
   `scripts/kernel-floor.limits`.

Each release step is a `tinymemory` tag plus a `modules::registry` re-pin with
digests taken **verbatim from the release's `checksum.toml`**, never recomputed
from a local build.

## D. The two ratchets are the to-do list

`memory/direct_engine_refs_tests.rs` (engine references) and
`memory/bypass_allowlist_tests.rs` (unguarded driver reach) each enumerate the
current callers with a verdict and a reason. **Both may shrink and must never
grow**, and each has a staleness half: a migrated file must be *deleted* from the
list, not left behind as a comment. Keeping them green is what makes this
converge instead of drifting.
