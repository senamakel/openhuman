# TinyAgents Vendor Improvement & Harness Migration Plan

Status: new plan (2026-07-04). Branch: `docs/tinyagents-vendor-audit-plan`.

This folder is the successor planning surface to
[`../tinyagents-full-migration-plan/`](../tinyagents-full-migration-plan/)
(whose `CONTINUATION-2026-07.md` C0–C7 workstreams remain the authoritative
*migration* backlog). What changed and why this folder exists:

1. **TinyAgents is now a vendored, editable submodule** (`vendor/tinyagents`,
   pinned at `v1.5.0` + `ac73382` MicrocompactMiddleware, patched into both
   Cargo worlds via `[patch.crates-io]`). Every "file an upstream issue and
   wait" item in the old plan is now **direct work we can do in-tree**, test
   against OpenHuman immediately, and PR upstream from the submodule.
2. A fresh ground-truth audit (2026-07-04, three parallel sweeps: harness
   inventory, crate internals, docs status) surfaced **crate-side defects and
   gaps that no prior doc captured** — dropped reasoning tokens, a
   preset-only "Anthropic" provider, observability-only prompt caching, and
   hot-loop cloning. Fixing these in the vendor crate multiplies the value of
   every migration workstream.

## Folder map

| Doc | Theme |
| --- | ----- |
| [`00-audit-harness.md`](00-audit-harness.md) | What's left in `src/openhuman/agent/` (+ orchestration/inference), classified: adapter / migratable / product |
| [`01-audit-tinyagents.md`](01-audit-tinyagents.md) | Crate capability map + concrete defects found (file:line) |
| [`02-reasoning-thought-tokens.md`](02-reasoning-thought-tokens.md) | End-to-end thought-token plan: `ContentBlock::Thinking`, signatures, deltas, usage/cost |
| [`03-streaming.md`](03-streaming.md) | Stream-returning harness entry point, sub-agent delta propagation, tool-call lifecycle events |
| [`04-performance.md`](04-performance.md) | Clone elimination, schema caching, cache-key hashing, parallel tool execution |
| [`05-anthropic-prompt-caching.md`](05-anthropic-prompt-caching.md) | Native Anthropic Messages provider + `cache_control` request shaping |
| [`06-upstream-extraction.md`](06-upstream-extraction.md) | OpenHuman logic to move INTO tinyagents (extends old-plan C5) + deletions unlocked |
| [`07-execution-order.md`](07-execution-order.md) | Sequencing, gates, dependency graph vs C0–C7, reclaim estimates |

## Headline findings (TL;DR)

**Migration headroom.** `src/openhuman/agent/` is ~57k LOC (+~26k in
`agent_orchestration/`, ~12k adapter in `src/openhuman/tinyagents/`). The turn
loop already runs on tinyagents (`run_turn_via_tinyagents_shared`); the
remaining deletable/migratable surface is ~29k LOC, dominated by the session/
transcript shell (~13k), sub-agent runner (~6k), orchestration lifecycle glue
(~3.7k), progress/tracing projection (~2.9k), and tool-call dialects (~1.9k).
See `00-audit-harness.md`.

**Crate defects that block deletions today** (all now fixable in-vendor):

- **Reasoning is dropped end-to-end**: the OpenAI SSE path hardcodes
  `reasoning: ""`, `StreamAccumulator::finish` discards accumulated
  reasoning, the loop's middleware `ModelDelta` omits it, and
  `Usage.reasoning_tokens` is never populated. This is why OpenHuman's
  `ThinkingForwarder` shim still exists.
- **"Anthropic" is an OpenAI-compat preset**, not a Messages-API adapter: no
  `cache_control` prompt caching, no thinking blocks, no
  `cache_creation_input_tokens` accounting.
- **Prompt-cache shaping is observability-only**: `cache_segments` /
  `cacheable_prefix_ids()` never reach any wire field.
- **Hot-loop cloning**: full message history cloned per turn *and* per
  retry/fallback attempt; tool schemas rebuilt per iteration; response-cache
  key re-serializes the whole transcript per call.
- **No caller-facing stream**: `invoke_streaming` returns a completed run;
  sub-agents never stream to parents; no tool-call start/complete events.
- **Tools execute strictly serially** despite a `parallel_tool_calls`
  capability flag.

**Sequencing in one line**: fix reasoning + streaming + perf in the vendor
crate first (02/03/04 — they unblock `ThinkingForwarder` deletion and improve
every live turn immediately), land the Anthropic provider (05) next, and run
upstream extraction (06) in parallel with the old plan's C1 sessions cutover.

## Rules (inherited, unchanged)

- Approval/security/sandbox/workspace/credential boundaries are inviolate.
- JSON-RPC contracts stay stable unless a migration note lands with the change.
- Adapter first → proven parity → delete; deletions are mandatory and named.
- Vendor-crate changes: commit in `vendor/tinyagents` on a feature branch,
  bump the gitlink in OpenHuman, PR the submodule diff upstream
  (precedent: `NoProgressTracker` #7, MicrocompactMiddleware `ac73382`).
- Stage files explicitly (`git add <paths>`); verify branch before commit.
- Keep every doc in this folder ≤500 lines.
