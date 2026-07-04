# 00 — Harness Audit: what remains in OpenHuman, what can migrate

Ground truth as of 2026-07-04, main + waves 1–2 (#4473, #4483).

Scope totals (incl. tests):

| Tree | LOC | Notes |
| ---- | --- | ----- |
| `src/openhuman/agent/` | ~56,951 | this audit's core scope |
| `src/openhuman/agent_orchestration/` | ~25,749 | lifecycle glue over SDK graphs |
| `src/openhuman/tinyagents/` | ~12,185 | the live adapter seam (all turns route here) |
| `src/openhuman/inference/` | ~52,955 | provider layer; does NOT wrap tinyagents |

Classification legend: **(a)** thin adapter over tinyagents · **(b)**
migratable/duplicative generic harness logic · **(c)** product policy, stays ·
**(d)** unclear.

## 1. Where execution already lives on tinyagents

Chat turns (`harness/session/turn/core.rs` → `turn/graph.rs`), channel/CLI
turns (`harness/graph.rs`, entry `agent/bus.rs` `agent.run_turn`), and
sub-agent turns (`harness/subagent_runner/ops/graph.rs`) all route through
`run_turn_via_tinyagents_shared` in the `src/openhuman/tinyagents/` adapter.
Middleware, stop hooks, unknown-tool policy, and context compression are on
SDK middleware. The legacy `run_tool_call_loop` is deleted.

Direct `tinyagents::` references inside `agent/` are sparse by design — the
adapter module owns the seam. Highest densities: `subagent_runner/ops/runner.rs`
and `ops/graph.rs` (15 refs each), `turn/core.rs` (13).

## 2. Remaining subsystems, ranked by migratable size

1. **Session/transcript shell — `harness/session/`, ~13.2k LOC.** Transcript
   read/write + dialect-suffix compat (`transcript.rs` 1347+978 tests),
   builder assembly (`builder/factory.rs` 1312 — product, stays), turn IO
   (`session_io.rs`), migration (373), progress projection
   (`tool_progress.rs` 252). Duplicates crate `Store`/`AppendStore`/
   `StoreChatHistory`. Old-plan C1 covers the cutover; dual-writes are ON,
   shadow reads landed (flag OFF). **Biggest single unlock (~9–11k incl.
   session_db/subagent_sessions/session_import).**
2. **Sub-agent runner — `harness/subagent_runner/`, ~6.1k LOC.** Prepare →
   filter tools → run child → checkpoint → extract → mirror. Duplicates
   `SubAgent`/`SubAgentSession`/`SubAgentTool` + `graph::subagent_node`.
   `extract_tool.rs` (612) + `handoff.rs` (287) are generic (old-plan C5/C6);
   `tool_prep.rs` allowlist half is product.
3. **Orchestration lifecycle glue — `agent_orchestration/`, ~3.7k of it.**
   `running_subagents.rs` **1931** (target ≤300; watch channels, abort/wait/
   steer, tombstones — belongs on a durable `TaskStore`, see 06),
   `spawn_parallel_graph.rs` 1764 (dup of `map_reduce`).
4. **Progress/streaming projection — ~2.9k LOC.** `progress.rs` (303,
   `AgentProgress` incl. `ThinkingDelta`/`SubagentThinkingDelta`/
   `ToolCallArgsDelta`), `session/tool_progress.rs` (252, bridges
   `ProviderDelta` → `AgentProgress`), `progress_tracing.rs` + `langfuse.rs`
   + tests (~2k). Duplicates crate `AgentEvent`/`HarnessEventJournal`/
   Langfuse exporters. Deletion gated on old-plan C4 journal parity **and**
   on crate streaming fixes (docs 02/03 here) so nothing regresses in the UI.
5. **Tool-call dialects — ~1.9k LOC.** `dispatcher.rs` (609) `ToolDispatcher`
   trait (native/XML/P-Format), `harness/parse.rs` (833) permissive XML/JSON
   parser with arg-key drift recovery, `pformat.rs` (499) compact positional
   `name[a|b]` calls (~80% token cut, OpenHuman-invented). Live path is
   native; these survive for prompt-guided dialects + transcript compat.
   Extraction candidate (06) so the C1 read-cutover becomes a pure delete.
6. **Multimodal assembly — `multimodal.rs` 1690 (+1020 tests).**
   `[IMAGE:]`/`[FILE:]` markers → provider content blocks, mime allowlist,
   PDF extraction, fetch gating, truncation budgets. ~1550 generic (06).
7. **Cost — `cost.rs` 343.** Per-turn USD + tier pricing + budget stop-hook
   feed. Blocked on crate `Usage` having no money field and
   `reasoning_tokens` never being populated (see 01/02). Old-plan C3 flip
   criteria stand.
8. **Tool policy/filter — `tool_policy.rs` 524 + `harness/tool_filter.rs`
   299.** Overlaps crate `ToolPolicy`; residual gap is free-form
   model-visible `ToolSchema` metadata (sdk-gaps §1).

## 3. Confirmed product — keep in OpenHuman

- `triage/` (3.8k) — trigger classification; *invokes* the migrated loop.
  ~800-line generic evaluator core is an extraction candidate (06), envelope/
  escalation/events stay.
- `task_dispatcher/` (1.6k) — deterministic card dispatch (CAS claim →
  autonomous turn). Mechanics shrink onto `graph::todos` (old-plan C2).
- `archivist/` (1.4k + 1k tests) — episodic indexing/lessons; memory policy.
- `prompts/` (~4k) — section assembly/rendering; product voice.
- `agent/tools/` (~4k) — product tools (`run_workflow`, preferences,
  `delegate_to_personality`, todo CRUD → C2 projections).
- `builder/factory.rs`, definitions (`definition.rs` 846,
  `definition_loader.rs`, `builtin_definitions.rs`), `memory_context.rs`,
  `memory_protocol.rs`, credentials/sandbox context, `debug/`, `library/`,
  `bus.rs`, `schemas.rs`.
- `inference/` (53k) — stays for credential ownership, billing
  classification, OAuth, local/Ollama. Largest *parallel* duplication of a
  tinyagents capability in the tree, but per the standing verdict
  (`reliable.rs` 900) not forced; revisit only after a native Anthropic
  provider (05) proves out.

## 4. Streaming & reasoning today (OpenHuman side)

Provider deltas (`inference::provider::ProviderDelta::{TextDelta,
ThinkingDelta, ToolCallArgsDelta}`) are bridged in
`session/tool_progress.rs:226` into `AgentProgress`, a per-request
`mpsc::Sender` channel distinct from the DomainEvent bus. Reasoning reaches
the UI **only** through this OpenHuman-side path (plus the `ThinkingForwarder`
shim), because the crate drops reasoning (01 §3). Fixing 02/03 in the vendor
crate lets `AgentProgress` become a thin projection of crate `AgentEvent`s
and unblocks deleting `ThinkingForwarder`, `tool_progress.rs`, and eventually
the `progress_tracing` stack.

## 5. Delta vs the old plan's inventory

The C0–C7 verdicts hold. New/raised items from this sweep:

- `running_subagents.rs` keeps growing (1931; was flagged at the same size on
  2026-07-03 — enforce the ≤300 target when C6 lands or it accretes further).
- The dialect layer's delete gate ("no live path parses provider text") is
  reachable only after C1 phase 3 **and** either upstreaming P-Format/XML
  parsing (06) or accepting native-only transcripts.
- `inference/provider/reliable.rs` verdict unchanged, but 05 (native
  Anthropic in-crate) is the first concrete step that could eventually make
  routing non-turn provider calls through the crate harness attractive.
