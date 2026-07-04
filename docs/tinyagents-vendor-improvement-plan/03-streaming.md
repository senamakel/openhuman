# 03 — Streaming Improvements (vendor crate work)

Goal: the harness exposes a real caller-consumable stream, sub-agent deltas
propagate to the parent with lineage attribution, and tool calls get explicit
lifecycle events — so OpenHuman's `AgentProgress` layer becomes a thin
projection and the C4 progress_tracing deletion has full-fidelity input.

## Current state (from 01)

- Provider SSE is genuinely incremental (`openai/mod.rs:1020-1073`) and
  tool-call arg fragments stream (`ToolCallDelta`, `:796-799`).
- But `invoke_streaming` returns a completed `AgentRun`
  (`agent_loop/mod.rs:195-241`); consumers must attach an `EventSink` or
  middleware to see deltas.
- Sub-agents run the non-streaming path (`subagent/mod.rs:535`, `:179-189`)
  — a child's tokens/thinking/tool activity are invisible until it returns.
- No tool-call started/completed events; `MockModel::stream` replays a
  completed response (`model/types.rs:536-549`).

## Step 1 — `invoke_stream`: a Stream-returning entry point

- New API: `AgentHarness::invoke_stream(...) ->
  (impl Stream<Item = AgentStreamItem>, JoinHandle<Result<AgentRun>>)` (or a
  single stream whose terminal item carries the `AgentRun`).
- `AgentStreamItem` (new enum, `harness/stream/`): `TurnStarted`,
  `ModelDelta(ModelDelta)` (text + reasoning + tool_call, post doc 02),
  `ToolCallStarted { call_id, tool_name }`, `ToolCallArgsDelta`,
  `ToolCallCompleted { call_id, outcome }`, `AssistantMessage`,
  `UsageUpdated`, `RunCompleted/Failed` — each stamped with
  `run_id / parent_run_id / root_run_id / depth`.
- Implementation: a bounded `mpsc` fed from the existing emit points in
  `invoke_model_streaming_once` (`agent_loop/mod.rs:929-994`) and the tool
  execution section (`:526-659`). The `EventSink` path stays; this is a
  convenience projection over the same events, not a parallel system.
- Backpressure: bounded channel + documented drop-oldest vs block policy
  (default: block; the loop already awaits network).

## Step 2 — Sub-agent delta propagation

- `SubAgentTool::call` switches to the streaming child path and forwards
  child `AgentStreamItem`s into the parent's stream/sink with the child's
  `run_id` + inherited `root_run_id` and `depth+1`
  (`subagent/mod.rs:535` → `SubAgent::invoke_stream`).
- Recursion-safe: items flow through the parent's channel; no unbounded
  fan-in (children are executed serially today; revisit with doc 04 §4).
- Closes goal.md §3's "attribute every delta to parent/root run id".

## Step 3 — Tool-call lifecycle events

- Emit `ToolCallStarted` as soon as the terminal `Completed` (or the first
  named `ToolCallDelta`, with doc 02's tool-name-on-start) identifies the
  tool — this is what OpenHuman's UI needs to show "running X…" before args
  finish streaming.
- Emit `ToolCallCompleted` with outcome + duration after the wrap-onion tool
  call returns.

## Step 4 — Test/mock fidelity

- `MockModel::stream` gains scripted incremental emission (list of
  `ModelStreamItem`s with optional delays) so streaming tests are truly
  incremental (`model/types.rs:536-549`).
- Tests: caller consumes `invoke_stream` and observes interleaved model/tool
  items; nested sub-agent test asserts child deltas arrive with correct
  lineage before the parent's final message.

## Step 5 — OpenHuman follow-through

1. `run_turn_via_tinyagents_shared` adapters consume `invoke_stream` (or the
   sink projection) and map 1:1 onto `AgentProgress` — deleting the
   `ProviderDelta` bridge in `session/tool_progress.rs` (252).
2. `SubagentThinkingDelta`/subagent progress becomes lineage-filtered
   projection — removes bespoke child-progress plumbing in
   `subagent_runner/ops/runner.rs`.
3. C4 (journal-backed web progress) gains the missing fidelity: journals +
   live stream share one event vocabulary, enabling the
   `progress_tracing.rs` + `langfuse.rs` deletion (~2k).

Effort: **L** crate-side (Steps 1–4), **M** OpenHuman follow-through.
Depends on: doc 02 for reasoning-in-deltas (can land in either order; both
touch `invoke_model_streaming_once`, so sequence the merges).
