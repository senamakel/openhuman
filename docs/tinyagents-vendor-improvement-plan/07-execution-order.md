# 07 — Execution Order, Gates, and Reclaim

This plan runs **alongside** the old plan's C0–C7 (which continues:
wave 3 = flip `session_shadow_reads`, flip crate BudgetMiddleware to
enforcing, microcompact upstream PR). The vendor workstreams here are named
**V1–V6** to avoid collision.

## Workstreams

| WS | Doc | Theme | Effort | Blocks / unblocks |
| -- | --- | ----- | ------ | ----------------- |
| V1 | 04 §2–3 | Non-breaking perf (schema cache, incremental cache key) | S–M | none; immediate win |
| V2 | 02 | Reasoning end-to-end | L | deletes `ThinkingForwarder` (C7); feeds C3 pricing |
| V3 | 03 | invoke_stream + sub-agent propagation + tool lifecycle | L | C4 progress_tracing deletion; `tool_progress.rs` delete |
| V4 | 04 §1,4,5 | Arc'd messages (trait break), parallel tools, translation memo | M | coordinated gitlink bump with adapter changes |
| V5 | 05 | Native Anthropic + cache_control shaping | L→XL | needs V2 for thinking; enables 02.2/`reliable.rs` re-eval |
| V6 | 06 | Upstream extraction batch (dialects, multimodal, artifacts, TaskStore, hooks, ranker, triage node) | rolling | each unlocks a ledger delete; TaskStore item unlocks C6.2 |

## Suggested sequence

```
now ──► V1 (quick, non-breaking)
     ├─► V2 reasoning ──► V5 Anthropic (thinking replay) ─► 02.2 re-eval
     ├─► V3 streaming ──► C4 progress deletion
     ├─► V4 perf-breaking (single coordinated bump after V2/V3 merge)
     └─► V6 extractions (continuous, 1–2 in flight, ≤2 unmerged submodule branches)
old plan in parallel: wave 3 (C1 read flip, C3 budget flip) — independent
```

Rationale:

- V1 first: zero-risk, benefits every live turn, no API changes.
- V2 before V5: thinking blocks must exist in `ContentBlock` before the
  Anthropic adapter can replay them; V5 Step 1 (text/tools) may start early.
- V2 and V3 both touch `invoke_model_streaming_once` — land V2 first
  (smaller diff), rebase V3.
- V4's `ChatModel` trait break rides one coordinated gitlink bump so
  OpenHuman's adapter updates land atomically with it.
- C1 (sessions read flip) is **independent** of all V-work — don't serialize
  behind it; but the dialect extraction (V6.1) should merge before C1
  step 4's dispatcher/parse/pformat delete.

## Gates (each item ships only when its gate is green)

| Item | Gate |
| ---- | ---- |
| V2 merge | crate reasoning e2e tests + OpenHuman turn with thinking model shows deltas + persisted blocks + nonzero reasoning_tokens |
| ThinkingForwarder delete | V2 gitlink bump live in one release, no `[thinking]` divergence logs |
| V3 merge | nested sub-agent stream test green; `AgentProgress` projection parity vs current UI events (fixture diff) |
| progress_tracing delete | C4 journal parity AND V3 projection parity |
| V4 merge | crate bench: ≥50% allocation reduction on 500-msg synthetic; OpenHuman conformance suite green single-threaded |
| Parallel tools ON (cap>1) | approval-gate batch-parking reviewed; read-only tool families only |
| V5 first traffic | flag-gated non-turn traffic; cost/cache telemetry matches `inference/` within tolerance |
| Each V6 extraction | crate tests + local residue shrink + ledger tick + upstream PR opened |

## Expected reclaim (local, incl. tests; adds to old plan §5's ~29k)

| Source | Lines |
| ------ | ----- |
| `ThinkingForwarder` + `tool_progress.rs` + progress projection residue (V2+V3) | ~700 |
| progress_tracing stack (C4, unblocked by V3) | ~2,000 (already counted in old plan) |
| Dialect layer local delete (V6.1 + C1.4) | ~1,900+tests (counted in old plan C1) |
| `running_subagents.rs` squeeze via durable TaskStore (V6.4 + C6) | ~1,600 (counted in old plan C6) |
| Task-local context carriers via crate RunContext extensions (V6.9) | ~500 |

Net-new local reclaim beyond the old plan: **~1–2k lines** — the headline
value of V-work is not deletion but **capability and cost**: correct thought
tokens, live sub-agent streaming, prompt-cache dollars, and a faster loop on
every turn. The old plan's ~29k reclaim proceeds in parallel and several of
its gates (C4, C6.2, C1.4, C3 pricing, C7 ThinkingForwarder) become
*reachable* only because of V2/V3/V5/V6.

## Tracking

- Deletions continue to tick `../tinyagents-full-migration-plan/99-deletion-ledger.md`.
- Submodule branch queue + upstream PR status tracked in this folder's
  README as extractions ship (keep ≤2 unmerged).
- Re-audit `docs/tinyagents-sdk-gaps.md` after V2/V3/V5 land — they close
  gaps §3 (reasoning stream), part of §6 (event fidelity), §7 pricing
  inputs, and the provider half of §8.
