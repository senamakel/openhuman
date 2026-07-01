# TinyAgents Harness Migration Audit

Date: 2026-07-01

Scope: current `openhuman-5` checkout on branch `issue/4249-finish-tinyagents-migration`.

This is a documentation-only audit of how much of the OpenHuman agent harness is
actually migrated to TinyAgents, and which remaining OpenHuman files are good
candidates to port, collapse, or keep as product-specific adapters.

## Bottom Line

The core turn loop is migrated. The live chat turn, channel/CLI turn, and
sub-agent turn all route through `src/openhuman/tinyagents::run_turn_via_tinyagents_shared`.
That means the model/tool iteration loop, TinyAgents middleware stack, event
bridge, stop hooks, context compression, unknown-tool recovery, and tool policy
boundary are on the TinyAgents harness path.

The repository still has a large OpenHuman harness shell around that path:

| Area                                 | Rust files |  Lines | Current role                                                                                                                                          |
| ------------------------------------ | ---------: | -----: | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/openhuman/agent/harness/`       |         83 | 30,521 | Agent session assembly, transcript compatibility, prompt/tool filtering, context product logic, sub-agent build pipeline, leftover generic loop seams |
| `src/openhuman/tinyagents/`          |         13 |  6,481 | TinyAgents adapters, middleware, event bridge, graph helpers, checkpoint adapter                                                                      |
| `src/openhuman/agent_orchestration/` |         58 | 21,704 | Product orchestration tools, durable workflow/team/session state, graph-backed fanout/delegation wrappers                                             |

The migration should not be read as "OpenHuman can delete `agent/harness/` now."
The correct read is: the execution core is on TinyAgents, but much of the
surrounding runtime was preserved for product compatibility. Some of that should
stay in OpenHuman. Some is generic runtime behavior that can be ported upstream
or re-expressed through TinyAgents once SDK gaps close.

## What Is Migrated

### Turn execution chokepoints

- `src/openhuman/agent/harness/session/turn/core.rs` routes the main chat turn
  through `super::graph::run_chat_turn_graph`, which is a thin wrapper over
  `run_turn_via_tinyagents_shared`.
- `src/openhuman/agent/harness/graph.rs` routes channel/CLI turns through
  `run_turn_via_tinyagents_shared`.
- `src/openhuman/agent/harness/subagent_runner/ops/graph.rs` routes sub-agent
  turns through `run_turn_via_tinyagents_shared`.
- `src/openhuman/tinyagents/mod.rs` owns the shared harness assembly:
  OpenHuman provider adapter, tool adapters, event bridge, middleware, steering,
  early-exit hooks, stop hooks, model/tool call caps, and outcome capture.

Practical status: the old in-tree model/tool loop is no longer the live
execution engine for those three entry points.

### Middleware and policy

Already migrated into TinyAgents middleware or the TinyAgents adapter seam:

- Approval and security gating at the tool boundary.
- CLI/RPC-only tool denial.
- Channel permission ceiling and tool policy checks.
- Unknown-tool recovery via an internal sentinel.
- Non-object tool-argument recovery.
- Cost budget pre-checks before model calls.
- Repeated tool failure halting.
- Context compression and message trimming via TinyAgents middleware.
- OpenHuman tool-output budgets and payload summarizer hooks as TinyAgents
  tool middleware.
- Stop-hook checks via `src/openhuman/tinyagents/stop_hooks.rs`.

This is real migration, but not complete ownership transfer: OpenHuman still
keeps rich policy metadata outside TinyAgents because the SDK tool schema does
not yet carry all OpenHuman policy fields.

### Graph-backed orchestration

The codebase has several real TinyAgents graph uses:

- `src/openhuman/tinyagents/delegation.rs`: durable plan -> execute -> review ->
  finalize graph.
- `src/openhuman/agent_orchestration/workflow_runs/graph.rs`: workflow phase DAG
  scheduler graph.
- `src/openhuman/agent_orchestration/agent_teams/graph.rs`: member execution
  conditional-routing graph.
- `src/openhuman/tinyagents/orchestration.rs`: reusable `run_parallel_fanout`
  helper, used by `spawn_parallel_agents` and workflow phase fanout.
- `src/openhuman/tinyagents/topology.rs`: topology export for fixed graph
  structures.
- `src/openhuman/agent_orchestration/running_subagents.rs`: detached sub-agent
  lifecycle is mirrored into TinyAgents `InMemoryTaskStore`.

Practical status: graph adoption is meaningful but uneven. The larger product
tools still own validation, persistence, cancellation semantics, compatibility
events, and result formatting.

## What Is Still Mostly OpenHuman-Owned

### Session and transcript shell

Largest remaining harness surface: `src/openhuman/agent/harness/session/`
at roughly 13.2k lines.

Keep in OpenHuman for now:

- Session transcript migration and compatibility.
- Product-level `AgentBuilder` configuration.
- Prompt section assembly and prompt-cache stability decisions.
- Memory/context injection policy.
- Post-turn hooks, transcript persistence, and OpenHuman-specific history shape.
- Tool dispatcher compatibility for persisted native/XML/P-format transcript
  suffixes.

Good TinyAgents candidates:

- Generic run status and event journal ownership.
- Generic checkpoint/cap handling.
- Generic transcript event projection once TinyAgents journals can reconstruct a
  run with redaction and replay.
- Generic prompt-cache layout events.

### Sub-agent build pipeline

Remaining surface: `src/openhuman/agent/harness/subagent_runner/`
at roughly 6.1k lines.

Keep in OpenHuman:

- Agent definition lookup and allowlist enforcement.
- Prompt assembly for archetypes.
- Parent context, memory context, action root, sandbox, and toolkit filtering.
- Worker-thread mirroring and transcript compatibility.
- Integrations-agent preflight and handoff cache behavior.

Good TinyAgents candidates:

- A first-class `SubAgentSession` abstraction under the OpenHuman registry
  adapter.
- Parent/root run lineage and usage rollup.
- Safe steering, waiting, cancellation, and recursion depth semantics.
- Generic early-exit pause and resumable checkpoint handling.

### Detached sub-agent registry

Remaining surface: `src/openhuman/agent_orchestration/running_subagents.rs`
at roughly 1.0k lines.

Current state: it uses TinyAgents `InMemoryTaskStore` as a typed lifecycle
ledger, but still owns watch channels, abort handles, wait/steer/cancel control,
tombstones, ownership checks, and session lookup.

Good TinyAgents candidates:

- Durable `TaskStore` support.
- Typed wait/steer/cancel APIs.
- Parent/root run tree queries.
- Cancellation requests and hard abort lifecycle events.

Keep in OpenHuman until the SDK grows those pieces:

- Desktop restart/resume compatibility.
- Existing JSON-RPC/tool response shapes.
- Durable OpenHuman session and worker-thread ledgers.

### Parallel agents and workflow fanout

Remaining surfaces:

- `src/openhuman/agent_orchestration/tools/spawn_parallel_agents.rs`
- `src/openhuman/agent_orchestration/workflow_runs/engine.rs`
- `src/openhuman/tinyagents/orchestration.rs`

Current state: fanout execution uses a TinyAgents graph helper, but
`spawn_parallel_agents` is not yet a first-class graph tool. It still owns
validation, worktree setup, overlap/stale-read checks, compatibility events, and
JSON output formatting.

Good TinyAgents candidates:

- Re-express `spawn_parallel_agents` as `validate -> dispatch -> worker ->
collect -> finalize` using graph `Send` or an SDK map/reduce helper.
- Move deterministic ordering, cancellation boundaries, graph status, and child
  lineage into TinyAgents graph state.
- Keep result formatting and OpenHuman worktree policy in a thin wrapper.

### Per-agent graph selectors

There are 32 `src/openhuman/agent_registry/agents/*/graph.rs` files and all
currently return `AgentGraph::Default`.

This is mostly scaffolding, not migrated behavior. It proves the extension seam
exists, but there are no bespoke per-agent TinyAgents graphs yet.

Good candidates for first real custom graphs:

- `orchestrator`: planning/delegation/parallelism policy can become explicit
  graph routing instead of prompt-only convention.
- `researcher`: search -> read -> synthesize -> cite can be bounded and
  checkpointed.
- `tool_maker`: detect missing capability -> generate -> validate -> expose can
  become a graph with explicit review gates.

Candidate cleanup: replace boilerplate default `graph.rs` files with registry
defaults once at least one real custom graph proves the API shape.

## Porting Candidates By Priority

### P0: Update stale active docs and comments

Evidence: active architecture docs still describe the old `engine::run_turn_engine`
loop in sections below the TinyAgents status callout.

Action:

- Rewrite active sections to describe TinyAgents as the live loop.
- Move the removed in-house loop details into a short historical appendix.
- Sweep code comments that still say `run_turn_engine`, `run_tool_call_loop`, or
  `run_inner_loop` when they now mean the TinyAgents harness path.

Why first: stale docs make it hard to tell whether remaining files are live
runtime, compatibility shell, or historical residue.

### P1: Make event/status journals canonical

Current OpenHuman files:

- `src/openhuman/tinyagents/observability.rs`
- `src/openhuman/agent/harness/engine/progress.rs`
- `src/openhuman/session_db/run_ledger/*`
- `src/core/event_bus/*`

Target shape:

- TinyAgents journals/status stores become the internal source for model/tool
  events, usage, graph steps, child run lineage, and resumable inspection.
- `AgentProgress` and `DomainEvent` become compatibility projections.

Risk: this crosses UI streaming, cost footer, run ledgers, and desktop
reconnect behavior. It needs focused parity tests before deletion.

### P2: Port detached task lifecycle beyond `InMemoryTaskStore`

Current OpenHuman files:

- `src/openhuman/agent_orchestration/running_subagents.rs`
- `src/openhuman/agent_orchestration/tools/{wait_subagent,steer_subagent,close_subagent,continue_subagent}.rs`

Target shape:

- TinyAgents owns typed task lifecycle, wait/steer/cancel semantics, parent/root
  run lineage, and terminal history.
- OpenHuman owns durable SQL/JSON projection and product response formatting.

Blocker: TinyAgents needs a durable task/status store, not only in-memory shape.

### P3: Re-express `spawn_parallel_agents` as a graph tool

Current OpenHuman files:

- `src/openhuman/agent_orchestration/tools/spawn_parallel_agents.rs`
- `src/openhuman/agent_orchestration/worktree.rs`
- `src/openhuman/tinyagents/orchestration.rs`

Target shape:

- A graph with nodes for validation, dispatch, worker, collect, finalize.
- Deterministic reducer state for per-task result order.
- Child run lineage and cancellation through graph status.
- Thin OpenHuman wrapper for the existing JSON result and worktree policy.

Why: this is a high-value migration because it converts a large product-visible
orchestration loop without touching the basic chat turn.

### P4: Replace generic checkpoint/progress seams

Current OpenHuman files:

- `src/openhuman/agent/harness/engine/checkpoint.rs`
- `src/openhuman/agent/harness/engine/progress.rs`
- `src/openhuman/agent/harness/subagent_runner/ops/checkpoint.rs`

Target shape:

- Model-call cap, early-exit pause, resumable checkpoint summary, and progress
  projection represented as TinyAgents status/events/middleware.

Keep only the OpenHuman-specific compatibility formatting for existing
transcripts and tool outputs.

### P5: Retire boilerplate per-agent graph selectors

Current OpenHuman files:

- `src/openhuman/agent/harness/agent_graph.rs`
- `src/openhuman/agent_registry/agents/*/graph.rs`

Target shape:

- Default graph supplied centrally.
- Only agents with custom graph behavior keep a `graph.rs`.
- Registry diagnostics show which graph each agent actually uses.

This should happen after at least one bespoke graph lands, so the cleanup does
not remove a seam before it has proved useful.

## Code That Should Probably Stay In OpenHuman

Do not port these blindly into TinyAgents:

- Agent registry definitions and built-in prompt semantics.
- Tool allowlists/denylists that encode product policy.
- Security policy, sandbox roots, approval records, credential access, and
  workspace/action-dir boundaries.
- Session transcript migration and persisted compatibility formats.
- OpenHuman model provider routing, billing classification, and credential
  ownership.
- UI-facing JSON-RPC response shapes and `DomainEvent` compatibility until every
  subscriber is moved.
- Worktree isolation policy and dirty-worktree safeguards.
- Memory stores, retrieval policy, and context source identity.

TinyAgents should own generic runtime machinery. OpenHuman should own product
semantics and compatibility boundaries.

## Main SDK Gaps Blocking More Deletion

The most important upstream gaps are already tracked in
`docs/tinyagents-sdk-gaps.md`; this audit confirms they are still the blockers
for removing more local harness code:

- Rich tool policy metadata in TinyAgents schemas.
- Recoverable unknown-tool policy without the OpenHuman sentinel.
- Reasoning/tool-argument streaming events with stable lineage.
- Durable task/status/event stores with replay, redaction, cursors, and
  cancellation.
- SQLite compatibility that does not force OpenHuman onto TinyAgents'
  `rusqlite` native-link version.
- Higher-level graph fanout/map-reduce helpers.
- Parent/root run lineage and usage/cost rollup.

## Suggested Next Documentation Pass

1. Rewrite the active `gitbooks/developing/architecture/agent-harness.md`
   sections below the status block so they describe the TinyAgents path instead
   of the removed in-house loop.
2. Add a small "Historical pre-TinyAgents loop" appendix for details that are
   still useful context.
3. Add per-folder READMEs or module docs for:
   - `src/openhuman/agent/harness/session/`: product session shell over
     TinyAgents.
   - `src/openhuman/agent/harness/subagent_runner/`: OpenHuman sub-agent build
     pipeline over TinyAgents.
   - `src/openhuman/agent_orchestration/`: product orchestration wrappers over
     TinyAgents graphs/task lifecycle.
4. Keep `docs/tinyagents-migration-spec.md` as the backlog, and use this audit
   as the current inventory snapshot.
