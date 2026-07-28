# Moving `agent/` into TinyAgents

**Status:** spec + plan. Direction decided by the maintainer on 2026-07-28
after two rounds of pushback; this document plans the move rather than
re-arguing it. The open questions below are *how*, not *whether*.
**Scope:** relocate `src/openhuman/agent/` (152 files, 70,530 LOC) into
`vendor/tinyagents` as a generic agent runtime, with OpenHuman's coupling
expressed as trait injection.
**Supersedes:** the "permanent host" dispositions for `builder/`, `turn/`,
`runtime.rs`, and `types.rs` in
[`2026-07-28-agent-session-transcript-to-tinyagents-design.md`](2026-07-28-agent-session-transcript-to-tinyagents-design.md)
§6 (verified 2026-07-28: those four rows carry the literal "Permanent host"
verdict at §6), and the `agent/ remainder — STAYS` row in
[`../tinyagents-migration-plan-2026-07-22.md`](../tinyagents-migration-plan-2026-07-22.md)
§7. Both are reopened by this decision; the reopen rows are recorded in
[`../tinyagents-drift-ledger.md`](../tinyagents-drift-ledger.md) (section
"Agent-Relocation Reopen Rows (2026-07-28)").

> **Corrections (2026-07-28).** Every count below was re-derived from the
> worktree on 2026-07-28 with
> `rg -o 'crate::openhuman::<domain>::' src/openhuman/agent --no-filename | wc -l`
> (outbound) and a word-boundary `agent::<symbol>` grep over `src/` +
> `app/src-tauri/src/` excluding `src/openhuman/agent/` (inbound). Changes from
> the first draft: outbound domain count 45 → **48**; inbound domain count
> 48 → **50** (47 counting code references only, excluding doc-comment-only
> mentions); `tinyagents` 91 → 90, `tools` 84 → 83, `context` 52 → 51,
> `approval` 10 → 9, `subconscious` 8 → 7, `web_chat` 6 → 5, `tool_status`
> 5 → 4, `memory_store` 41 → 39; `util` promoted out of the "≤ 4 refs"
> remainder bucket (it has 8) and `migrations` dropped from it (0 refs);
> inbound `agent::harness::*` 275 → 300, `agent::triage` 24 → 34, `agent::bus`
> 14 → 25; the Phase 4 exit figure ~2,000 → **547**; "~20–25k LOC stays"
> confirmed by measurement at **22,121 LOC**. The headline `152 files,
> 70,530 LOC` is exact and unchanged. No argument in this document changes as
> a result — every correction moves a number, not a conclusion, and the three
> inbound corrections all move *upward*, reinforcing §6's "inbound coupling is
> the real cost".

---

## 1. Why this is feasible (the enabling fact)

The earlier objection was dependency direction: `agent/` reaches 48 OpenHuman
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

### 2.1 Outbound — what `agent/` imports (48 domains)

By reference count:

| Refs | Domain | Becomes |
| ---: | --- | --- |
| 195 | `config` (`Config` 53, `AgentConfig` 45, `MemoryConfig` 28, `ContextConfig` 24, …) | **crate config structs**, populated host-side. The single largest blocker — see §4.1 |
| 90 | `tinyagents` (the seam) | dissolves — becomes internal |
| 83 | `tools` | existing `Tool<State>` + `SharedToolAdapter` |
| 72 | `inference` | existing `ChatModel<State>` + a `ModelResolver` trait |
| 57 + 39 + 16 + 11 + 5 + 3 | `memory`, `memory_store`, `memory_tree`, `agent_memory`, `memory_tools`, `memory_conversations` | **`MemoryProvider`** trait |
| 51 | `context` | **`ContextComposer`** trait |
| 34 + 22 | `profiles`, `agent_registry` | **`DefinitionRegistry`** trait |
| 28 | `composio` | host `Tool` impls — no new trait |
| 25 + 9 + 6 + 4 + 2 | `security`, `approval`, `agent_tool_policy`, `sandbox`, `prompt_injection` | **`SecurityGate`** trait |
| 22 + 1 | `skills`, `skill_runtime` | host `Tool` impls |
| 21 | `todos` | already crate `graph::todos` (parent spec DS-1) |
| 19 + 7 + 4 | `tokenjuice`, `cost`, `scheduler_gate` | **`BudgetGate`** trait |
| 11 | `learning` | **`LearningSink`** trait |
| 7 | `subconscious` | host impl behind `LearningSink` |
| 5 + 4 | `web_chat`, `channels` | **`ProgressSink`** trait |
| 4 | `tool_status` | **`ToolOutcomeClassifier`** trait |
| 5 | `thread_goals` | host impl behind `ContextComposer` |
| 5 | `embeddings` | existing `EmbeddingModel` |
| 5 | `agent_orchestration` | existing `graph::orchestration` |
| 3 | `agent_experience` | **`ExperienceStore`** trait |
| 8 | `util` | inlined generics — no trait |
| remainder (`app_state` 3, `session_db` 2, `file_state` 2, `task_sources` 2, `mcp_registry` 2, `session_import` 2, `tinycortex` 2, `tool_timeout` 2, `threads` 1, `credentials` 1, `cwd_jail` 1, `memory_goals` 1, `memory_sync` 1) | ≤ 3 refs each | host impls or inlined generics |

**~10 new traits** cover 48 domains, because most domains reach `agent/` through
one of a few conceptual seams.

### 2.2 Inbound — what imports `agent/` (50 domains)

This is the half the earlier analysis under-weighted, and it is the larger risk.
50 domains under `src/openhuman/` name `agent::`; 47 of them do so from code
rather than a doc comment. By symbol:

| Refs | Symbol | Note |
| ---: | --- | --- |
| 300 | `agent::harness::*` | the bulk; moves down |
| 58 | `agent::turn_origin` | product enum — **stays host** |
| 43 | `agent::messages` (`ChatMessage`) | durable DTO — **stays host** (WP-1) |
| 34 | `agent::triage` | product — **stays host** |
| 23 | `agent::progress` (`AgentProgress`) | UI contract — **stays host**, produced via `ProgressSink` |
| 16 | `agent::prompts` | `SOUL.md`/`IDENTITY.md` — **stays host** |
| 15 | `agent::host_runtime` | **stays host** by definition |
| 25 | `agent::bus` | event-bus glue — **stays host** |
| 12 | `agent::task_board` | already crate `graph::todos` |
| 13 | `agent::message_convert` | boundary adapter — **stays host** |
| 12 | `agent::hooks` | trait defs move; impls stay |
| 9 | `agent::error`, 10 `agent::cost`, 8 `agent::progress_tracing`, 7 `agent::tool_policy`, 7 `agent::pformat`, 7 `agent::task_dispatcher`, 4 `agent::stop_hooks` | mixed; see §3 |

**Consequence:** `agent/` does not empty out. **22,121 LOC stays** (measured
2026-07-28 over the §3 "stays in OpenHuman" file list — inside the originally
estimated 20–25k band) as
the host adapter layer (`ChatMessage`, `AgentProgress`, `turn_origin`, prompts,
triage, bus, host_runtime, message_convert, the trait impls). The deliverable is
"the runtime moves down and OpenHuman keeps an adapter", not "the directory
disappears".

### 2.3 Honest cost

48 outbound domains to invert, 50 inbound consumers to repoint, ~29k LOC of
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
traits**. Measured 22,121 LOC including tests (2026-07-28).

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

**Execution status (2026-07-28): Phases 0 and 1 are COMPLETE. Phases 2–7 were
deliberately NOT executed.** The program is parked at the Phase 1/2 boundary,
which is a legitimate stopping point: nothing has moved, no host file changed,
and the crate carries only additive, inert seams. Phase 2 is the gate — it is
the only phase with on-disk transcript risk and requires a shadow-read soak
release before it can start, so it must be scheduled deliberately rather than
picked up as the "next" task.

**Phase 0 — Ledger + trait catalogue (no code). ✅ COMPLETE (2026-07-28).**
Reopen the superseded rows (§ header). Write the ~10 trait signatures up as an
upstream design doc. Nothing moves until the trait catalogue is accepted
upstream — otherwise the first mover defines the seams by accident.
*Note:* the original wording said "an upstream RFC in `vendor/tinyagents/docs/`".
That violates the crate's documented convention — `vendor/tinyagents/docs/spec/README.md`
and `vendor/tinyagents/AGENTS.md` forbid standalone spec files dropped directly
in `docs/` and require a per-topic file under `docs/modules/<surface>/` linked
from that module's `README.md`, capped at 500 lines. The catalogue therefore
landed as `vendor/tinyagents/docs/modules/harness/host.md`, not as an RFC file.
*Exit (met):* trait catalogue documented upstream in the crate's own format;
ledger rows reopened in `docs/tinyagents-drift-ledger.md`; the plan's ground-truth
numbers re-derived from the tree (see the Corrections note in the header).

**Phase 1 — Land the traits upstream, empty. ✅ COMPLETE (2026-07-28).**
Add the traits + no-op/in-memory default impls to the crate. No host change.
*Exit (met):* the ten host-capability seams (`MemoryProvider`, `ContextComposer`,
`SecurityGate`, `BudgetGate`, `DefinitionRegistry`, `ExperienceStore`,
`LearningSink`, `ProgressSink`, `ToolOutcomeClassifier`, `ModelResolver`) plus
inert default impls are committed in `vendor/tinyagents` on branch
`agent-to-tinyagents`. **The submodule gitlink bump is a separate, later step and
is intentionally not part of the host commits for Phase 0/1** — the host tree is
unchanged apart from these docs.
*Design note carried into Phase 4:* the plan's §1 framing that `State` is "the
pattern already in use" is only two-thirds true. Of the 18 harness extension
traits, only 7 are generic over `State` (`ChatModel`, `Tool`, `Middleware`,
`ModelMiddleware`, `ToolMiddleware`, `ModelBaseCall`, `ToolBaseCall`); the 11
*capability* traits closest to the new seams (`ChatHistory`, `Store`,
`AppendStore`, `Summarizer`, `EmbeddingModel`, `VectorStore`, `ResponseCache`,
`WorkspaceIsolation`, `HarnessEventJournal`, `HarnessStatusStore`,
`EventListener`) are plain non-generic object-safe traits. Every OpenHuman
construction and impl today is `AgentHarness<()>` / `impl Tool<()>` — `State` is
unexercised in the host. Whichever shape Phase 4 adopts, adopting a non-unit
`State` is a signature change across ~30 existing `<()>` impls and every
`invoke(&(), …)` call site, and that cost is not counted in §2.3.

**Phase 2 — Transcript to crate `JsonlChatHistory` (transcript spec Option B).**
⛔ NOT STARTED — deliberately. Do this early and alone: it is the only phase with
on-disk risk. One release of shadow-read parity, mismatch logged never panicked,
legacy `DDMMYYYY/` and `read_transcript_legacy_md` paths covered.
*Exit:* resume works across upgrade on a real workspace; parity soak clean, **and
each of the three semantics named in the transcript spec's §3.1 gap table is
individually demonstrated over the crate-owned format: compaction-replacement
records, skippable interrupted partials, and the display-order (dual) read path.**
The transcript spec calls that gap table "the single most important constraint in
this document" — a generic `ChatHistory` impl that satisfies naive round-trip
parity but drops those three silently corrupts model context on any compacted
thread, so they are exit criteria in their own right, not sub-cases of "parity".

**Phase 3 — Config mapping (§4.1 Option A).** ⛔ NOT STARTED.
Introduce crate config structs + a host `session_config_from(&Config)` mapper.
Repoint `agent/` internals to the crate structs *in place*, before moving.
*Exit:* zero `crate::openhuman::config::` references inside the code slated to
move.

**Phase 4 — Implement the traits host-side, still in place.** ⛔ NOT STARTED.
`MemoryProvider`, `ContextComposer`, `SecurityGate`, `BudgetGate`,
`DefinitionRegistry`, `ExperienceStore`, `LearningSink`, `ProgressSink`,
`ToolOutcomeClassifier`, `ModelResolver`. `agent/` calls them instead of
reaching into domains directly. **This phase delivers most of the architectural
value with none of the relocation risk** — after it, `agent/`'s outbound
coupling is ~10 traits instead of 48 domains, and the program can legitimately
stop here.
*Exit:* `rg -o "crate::openhuman::" src/openhuman/agent/harness/session/` down
from its 2026-07-28 baseline of **547** occurrences to the adapter layer only.
(For scale: the whole of `harness/` is 932 and the whole of `agent/` is 1,287 —
an earlier draft of this plan cited ~2,000 for `session/`, which no scoping of
that grep reproduces.)

**Phase 5 — Relocate, module family at a time.** ⛔ NOT STARTED.
Order by inbound coupling, lowest first: `artifact_offload` → `run_queue` →
`parse`/`tool_calling` (merges with DS-5b) → `subagent_runner` → `session/turn`
→ `session/{builder,runtime,types}`. Each family: move to
`vendor/tinyagents/src/harness/`, re-export from the host adapter for one
release, then repoint consumers.
*Exit per family:* crate tests green; host `cargo check` both worlds; the
family's tests live upstream.

**Phase 6 — Collapse the seam and the adapter.** ⛔ NOT STARTED.
`src/openhuman/tinyagents/` dissolves into the host adapter layer. Delete the
compatibility re-exports.
*Exit:* `agent/` is the adapter layer only; parent spec's DS-0 re-export gate
allowlist is seam-free.

**Phase 7 — Exit gate.** ⛔ NOT STARTED.
Full `scripts/test-rust-with-mock.sh`, `cargo test --all-features` in both
vendored crates, slim disabled build **and** `cargo test --lib
--no-default-features --features tokenjuice-treesitter core::`, `pnpm
rust:check`, deletion-ledger totals reconciled, architecture docs rewritten.

---

## 6. Risks

- **Inbound coupling is the real cost, not outbound.** 50 domains import
  `agent::`. Phase 5's per-family re-export window is what keeps that tractable;
  skipping it turns every family move into a 50-domain atomic commit.
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
| Inversion | 48 outbound domains → **~10 capability traits** |
| Reality check | `agent/` does not empty — 22,121 LOC stays as the host adapter (`ChatMessage`, `AgentProgress`, prompts, triage, bus, trait impls) |
| Gating decisions | config mapping (§4.1 → Option A); transcript format goes crate-owned (§4.2 → transcript spec Option B) |
| Highest-value / lowest-risk phase | **Phase 4** — trait injection in place. Cuts coupling 45 → 10 without moving a file; a legitimate stopping point |
| Highest-risk phase | **Phase 2** — on-disk transcript format, isolated and sequenced first |
| Honest cost | multi-quarter program; every phase leaves the tree green and shippable |

