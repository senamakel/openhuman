# Moving `agent/` into TinyAgents

**Status:** spec + plan. Direction decided by the maintainer on 2026-07-28
after two rounds of pushback; this document plans the move rather than
re-arguing it. The open questions below are *how*, not *whether*.
**Scope:** relocate `src/openhuman/agent/` (152 files, 70,530 LOC) into
`vendor/tinyagents` as a generic agent runtime, with OpenHuman's coupling
expressed as trait injection.
**Supersedes:** the "permanent host" dispositions for `builder/`, `turn/`,
`runtime.rs`, and `types.rs` in
`2026-07-28-agent-session-transcript-to-tinyagents-design.md` §6, and the
`agent/ remainder — STAYS` row in `tinyagents-migration-plan-2026-07-22.md` §7.
Those rows are reopened by this decision and must be updated in the ledger.

---

## 1. Why this is feasible (the enabling fact)

The earlier objection was dependency direction: `agent/` reaches 45 OpenHuman
domains, so relocating it appeared to force a GPL-redistributed crate to import
Composio, `SecurityPolicy`, and `memory_store`.

That objection assumes the code moves *as written*. It does not have to,
because **the crate is already generic over a host-supplied state type**:

```rust
pub struct AgentHarness<State: Send + Sync, Ctx: Send + Sync = ()> { … }
pub trait Tool<State: Send + Sync>: Send + Sync { … }
pub trait ChatModel<State: Send + Sync>: Send + Sync { … }
pub trait Middleware<State: Send + Sync, Ctx: Send + Sync = ()>: Send + Sync { … }
```

`State` is the injection vehicle. A relocated agent runtime does not import
`crate::openhuman::memory` — it is generic over a `State` that *provides*
memory, and OpenHuman supplies the impl. The crate already ships **18 extension
traits** on exactly this pattern (`ChatModel`, `Tool`, `ChatHistory`, `Store`,
`AppendStore`, `Summarizer`, `EmbeddingModel`, `VectorStore`, `ResponseCache`,
`WorkspaceIsolation`, `HarnessEventJournal`, `HarnessStatusStore`,
`EventListener`, `Middleware`, `ModelMiddleware`, `ToolMiddleware`,
`ModelBaseCall`, `ToolBaseCall`). This move adds ~10 more of the same kind. It
is not a new architecture; it is more of the one already in use.

The GPL/crates.io concern also narrows correctly: the constraint is that no
*OpenHuman product logic* is published, not that no agent runtime is. Traits and
a generic loop are publishable; `provider_role_for`'s `subconscious` routing and
the `integrations_agent` dispatcher override are not — they become host impls.

---

## 2. Ground truth: the coupling to invert

### 2.1 Outbound — what `agent/` imports (45 domains)

By reference count:

| Refs | Domain | Becomes |
| ---: | --- | --- |
| 195 | `config` (`Config` 53, `AgentConfig` 45, `MemoryConfig` 28, `ContextConfig` 24, …) | **crate config structs**, populated host-side. The single largest blocker — see §4.1 |
| 91 | `tinyagents` (the seam) | dissolves — becomes internal |
| 84 | `tools` | existing `Tool<State>` + `SharedToolAdapter` |
| 72 | `inference` | existing `ChatModel<State>` + a `ModelResolver` trait |
| 57 + 41 + 16 + 11 + 5 + 3 | `memory`, `memory_store`, `memory_tree`, `agent_memory`, `memory_tools`, `memory_conversations` | **`MemoryProvider`** trait |
| 52 | `context` | **`ContextComposer`** trait |
| 34 + 22 | `profiles`, `agent_registry` | **`DefinitionRegistry`** trait |
| 28 | `composio` | host `Tool` impls — no new trait |
| 25 + 10 + 6 + 4 + 2 | `security`, `approval`, `agent_tool_policy`, `sandbox`, `prompt_injection` | **`SecurityGate`** trait |
| 22 + 1 | `skills`, `skill_runtime` | host `Tool` impls |
| 21 | `todos` | already crate `graph::todos` (parent spec DS-1) |
| 19 + 7 + 4 | `tokenjuice`, `cost`, `scheduler_gate` | **`BudgetGate`** trait |
| 11 | `learning` | **`LearningSink`** trait |
| 8 | `subconscious` | host impl behind `LearningSink` |
| 6 + 4 | `web_chat`, `channels` | **`ProgressSink`** trait |
| 5 | `tool_status` | **`ToolOutcomeClassifier`** trait |
| 5 | `thread_goals` | host impl behind `ContextComposer` |
| 5 | `embeddings` | existing `EmbeddingModel` |
| 5 | `agent_orchestration` | existing `graph::orchestration` |
| 3 | `agent_experience` | **`ExperienceStore`** trait |
| remainder (`util`, `app_state`, `session_db`, `file_state`, `task_sources`, `mcp_registry`, `threads`, `session_import`, `migrations`, `tinycortex`, `tool_timeout`) | ≤ 4 refs each | host impls or inlined generics |

**~10 new traits** cover 45 domains, because most domains reach `agent/` through
one of a few conceptual seams.

### 2.2 Inbound — what imports `agent/` (48 domains)

This is the half the earlier analysis under-weighted, and it is the larger risk.
By symbol:

| Refs | Symbol | Note |
| ---: | --- | --- |
| 275 | `agent::harness::*` | the bulk; moves down |
| 58 | `agent::turn_origin` | product enum — **stays host** |
| 43 | `agent::messages` (`ChatMessage`) | durable DTO — **stays host** (WP-1) |
| 24 | `agent::triage` | product — **stays host** |
| 22 | `agent::progress` (`AgentProgress`) | UI contract — **stays host**, produced via `ProgressSink` |
| 14 | `agent::prompts` | `SOUL.md`/`IDENTITY.md` — **stays host** |
| 14 | `agent::host_runtime` | **stays host** by definition |
| 14 | `agent::bus` | event-bus glue — **stays host** |
| 12 | `agent::task_board` | already crate `graph::todos` |
| 12 | `agent::message_convert` | boundary adapter — **stays host** |
| 11 | `agent::hooks` | trait defs move; impls stay |
| 8 | `agent::error`, 8 `agent::cost`, 7 `agent::tool_policy`, 7 `agent::progress_tracing`, 7 `agent::pformat`, 6 `agent::task_dispatcher`, 4 `agent::stop_hooks` | mixed; see §3 |

**Consequence:** `agent/` does not empty out. Roughly **20–25k LOC stays** as
the host adapter layer (`ChatMessage`, `AgentProgress`, `turn_origin`, prompts,
triage, bus, host_runtime, message_convert, the trait impls). The deliverable is
"the runtime moves down and OpenHuman keeps an adapter", not "the directory
disappears".

### 2.3 Honest cost

45 inbound domains to invert, 48 outbound consumers to repoint, ~29k LOC of
tests to migrate or re-home, a cross-repo change in two Cargo worlds, and an
on-disk/behavioural surface (transcript format, progress events, cost
accounting) that users depend on. **This is a multi-quarter program, not a
refactor.** §5 sequences it so every phase is independently valuable and the
program can be halted at any phase boundary without leaving the tree broken.

---

## 3. Disposition

### Moves into `tinyagents` (generic over `State`)

| Host area | Prod LOC | Lands as |
| --- | ---: | --- |
| `harness/session/{runtime,types,builder}` — session lifecycle & assembly | ~3,700 | `harness::session` — `Session<State>` + builder over capability traits |
| `harness/session/turn/*` — turn orchestration shell | ~4,476 | `harness::session::turn` — generic loop + `TurnPreparation` pipeline |
| `harness/subagent_runner/` | ~5,541 | merges into existing `harness::subagent` + `graph::orchestration` |
| `harness/session/transcript.rs` + `turn_checkpoint.rs` | ~2,100 | `harness::memory::JsonlChatHistory` (this is Option B of the transcript spec — **the move decision selects B**) |
| `harness/{parse,definition,definition_loader,tool_filter,required_output,graph,agent_graph,fork_context}.rs` | ~3,300 | `harness::{tool_calling, definition, graph}` — merges with #55/#57 |
| `harness/artifact_offload/`, `tool_result_artifacts/` | ~1,400 | `harness::artifacts` |
| `harness/run_queue/`, `harness/memory_context*.rs` | ~1,000 | `harness::runtime`, behind `MemoryProvider` |
| `task_dispatcher/`, `dispatcher.rs` (parse half), `pformat.rs`, `stop_hooks.rs`, `hooks.rs` (trait defs) | ~3,000 | `harness::{tool_calling, hooks}` |
| `progress_tracing/` | ~3,186 | deleted, not moved — crate observability already covers it (parent spec DS-5) |

### Stays in OpenHuman as the adapter layer

`messages.rs` (`ChatMessage`), `message_convert.rs`, `progress.rs`
(`AgentProgress`), `turn_origin.rs`, `prompts/`, `triage/`, `bus.rs`,
`host_runtime.rs`, `error.rs`, `cost.rs`, `tool_policy.rs`, `multimodal.rs`,
`agent/tools/`, `archivist/`, `schemas.rs`, plus **every impl of the ~10 new
traits**. Estimated ~20–25k LOC including tests.

---

## 4. The two decisions that gate everything

### 4.1 Config (195 refs — the real blocker)

`agent/` reads `Config`, `AgentConfig` (742-line schema), `MemoryConfig`,
`ContextConfig` directly. A generic runtime cannot import OpenHuman's config
schema. Options:

- **A — Crate-owned config structs.** The crate defines `SessionConfig`,
  `TurnConfig`, `ToolConfig`; OpenHuman maps its schema into them at build time.
  Explicit, versionable, and mirrors how `MemoryConfig` is derived for TinyCortex
  (`tinycortex/config.rs::memory_config_from`). **Recommended.**
- **B — `ConfigProvider` trait** with ~40 getters. Avoids a mapping layer but
  turns every config read into a virtual call and makes the trait a dumping
  ground.
- **C — Generic `State` carries config.** Least code, worst discoverability;
  every crate-side read needs a bound.

Pick A. It is the pattern the org already uses successfully one crate over.

### 4.2 `ChatMessage` and the transcript format

Moving the session runtime down forces the transcript decision to **Option B**
of the transcript spec: the `session_raw` JSONL format becomes crate-owned
public API, and `ChatMessage`'s durable fields must survive as crate `Message` +
a `raw` passthrough (the `ToolResult::raw` precedent). This is the change with
real user-visible risk — existing installs have live transcripts and resume must
keep working. Phase 2 exists solely to de-risk it.

---

## 5. Phased plan

Each phase is independently valuable and leaves the tree green. Stop-anywhere is
a hard requirement, not a nicety.

**Phase 0 — Ledger + trait catalogue (no code).** — *in progress (2026-08-02)*
Reopen the superseded rows (§ header). Write the ~10 trait signatures as an
upstream RFC in `vendor/tinyagents/docs/`. Nothing moves until the trait
catalogue is accepted upstream — otherwise the first mover defines the seams by
accident.
*Exit:* accepted RFC; ledger rows reopened.

Draft landed: [`vendor/tinyagents/docs/spec/host-capability-traits-rfc.md`](../../vendor/tinyagents/docs/spec/host-capability-traits-rfc.md)
— all ten signatures, grounded in measured reference counts. **Not yet
accepted**; it carries four open questions that block Phase 1, the hard one
being a name collision: `tinyflows` 0.5.1 shipped its own, unrelated
`MemoryProvider` trait, and both crates are in OpenHuman's dependency graph.

**Phase 1 — Land the traits upstream, empty.**
Add the traits + no-op/in-memory default impls to the crate. No host change.
*Exit:* crate `cargo test --all-features` green; version bump; both lockfiles.

**Phase 2 — Transcript to crate `JsonlChatHistory` (transcript spec Option B).**
Do this early and alone: it is the only phase with on-disk risk. One release of
shadow-read parity, mismatch logged never panicked, legacy `DDMMYYYY/` and
`read_transcript_legacy_md` paths covered.
*Exit:* resume works across upgrade on a real workspace; parity soak clean.

**Phase 3 — Config mapping (§4.1 Option A).**
Introduce crate config structs + a host `session_config_from(&Config)` mapper.
Repoint `agent/` internals to the crate structs *in place*, before moving.
*Exit:* zero `crate::openhuman::config::` references inside the code slated to
move.

**Phase 4 — Implement the traits host-side, still in place.**
`MemoryProvider`, `ContextComposer`, `SecurityGate`, `BudgetGate`,
`DefinitionRegistry`, `ExperienceStore`, `LearningSink`, `ProgressSink`,
`ToolOutcomeClassifier`, `ModelResolver`. `agent/` calls them instead of
reaching into domains directly. **This phase delivers most of the architectural
value with none of the relocation risk** — after it, `agent/`'s outbound
coupling is ~10 traits instead of 45 domains, and the program can legitimately
stop here.
*Exit:* `grep -c "crate::openhuman::" src/openhuman/agent/harness/session/` down
from ~2,000 refs to the adapter layer only.

**Phase 5 — Relocate, module family at a time.**
Order by inbound coupling, lowest first: `artifact_offload` → `run_queue` →
`parse`/`tool_calling` (merges with DS-5b) → `subagent_runner` → `session/turn`
→ `session/{builder,runtime,types}`. Each family: move to
`vendor/tinyagents/src/harness/`, re-export from the host adapter for one
release, then repoint consumers.
*Exit per family:* crate tests green; host `cargo check` both worlds; the
family's tests live upstream.

**Phase 6 — Collapse the seam and the adapter.**
`src/openhuman/tinyagents/` dissolves into the host adapter layer. Delete the
compatibility re-exports.
*Exit:* `agent/` is the adapter layer only; parent spec's DS-0 re-export gate
allowlist is seam-free.

**Phase 7 — Exit gate.**
Full `scripts/test-rust-with-mock.sh`, `cargo test --all-features` in both
vendored crates, slim disabled build **and** `cargo test --lib
--no-default-features --features tokenjuice-treesitter core::`, `pnpm
rust:check`, deletion-ledger totals reconciled, architecture docs rewritten.

---

## 6. Risks

- **Inbound coupling is the real cost, not outbound.** 48 domains import
  `agent::`. Phase 5's per-family re-export window is what keeps that tractable;
  skipping it turns every family move into a 48-domain atomic commit.
- **On-disk transcript risk (Phase 2)** is the only user-visible data risk in
  the program. It is deliberately isolated and sequenced first.
- **`AgentProgress` is a UI contract.** It stays host-side and is produced
  through `ProgressSink`. If it drifts into the crate, the frontend timeline,
  cost footer, and citation chips break in ways unit tests will not catch.
- **Trait-explosion.** Ten traits is the budget. If Phase 4 needs a fifteenth,
  that is a signal a seam is wrong — re-open the RFC rather than adding it.
- **GPL/crates.io.** Publishable: traits, generic loop, tool-calling wire
  formats. Not publishable: `provider_role_for`'s `subconscious` routing, the
  `integrations_agent` override, OpenHuman prompt text, backend phrasing, key
  material. Every relocated file needs this check.
- **≥ 80% diff-coverage gate** on a program of this size — Phases 4 and 5 touch
  hundreds of files. Check `diff-cover` per slice.
- **Two Cargo worlds** — every crate bump regenerates root and
  `app/src-tauri` lockfiles (#3877).
- **`RUST_MIN_STACK=16777216`** — the subagent runner's large futures already
  overflow the default stack on Apple Silicon; Phase 5's subagent move is
  exactly where that resurfaces.
- **`GGML_NATIVE=OFF`** for local root-crate builds.

---

## 7. Summary

| | |
| --- | --- |
| Decision | move `agent/` into `tinyagents` (maintainer call, 2026-07-28) |
| Enabler | the crate is already generic over `State`; 18 extension traits use the pattern today |
| Inversion | 45 outbound domains → **~10 capability traits** |
| Reality check | `agent/` does not empty — ~20–25k LOC stays as the host adapter (`ChatMessage`, `AgentProgress`, prompts, triage, bus, trait impls) |
| Gating decisions | config mapping (§4.1 → Option A); transcript format goes crate-owned (§4.2 → transcript spec Option B) |
| Highest-value / lowest-risk phase | **Phase 4** — trait injection in place. Cuts coupling 45 → 10 without moving a file; a legitimate stopping point |
| Highest-risk phase | **Phase 2** — on-disk transcript format, isolated and sequenced first |
| Honest cost | multi-quarter program; every phase leaves the tree green and shippable |

