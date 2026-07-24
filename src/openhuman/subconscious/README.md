# subconscious

The subconscious is OpenHuman's **Deep Reflection Layer**: a scheduled loop
that observes how the user's connected memory changed, prepares local context,
and decides whether to act.

- **`memory`** — the user's connected memory sources (Gmail / Slack / Notion /
  folders). It observes a `memory_diff` against a baseline checkpoint. Changed
  windows prefer hosted Medulla reflection and execute a bounded set of local
  actions (to-dos, goals, `notify_user`, blocking delegation).

`subconscious.engine = "auto"` is the default. It tries hosted Medulla and falls
back to the local decision agent only when unavailability is known before the
initial run request is submitted. `engine = "local"` bypasses hosted execution.
The legacy draft value `"medulla"` deserializes as `"auto"`.

## The generic tick (`instance.rs`)

Each tick body is a [`tinyagents` `CompiledGraph`](../tinyagents) — the same durable
runtime the orchestration wake path runs on:

```text
START ─► observe ─┬─(quiet)──────────────────────────► commit ─► done ─► END
                  └─(changed)─► prepare ─► reflect ─┬─(ok)──► commit ─► done ─► END
                                                    └─(err)────────────► done ─► END
```

The node handlers delegate 1:1 to the world's [`SubconsciousProfile`](profile.rs) —
the profile **is** the injected runtime. Everything world-agnostic lives in the
runner **once**, so every world gets it for free:

- per-instance tick lock + 5s acquisition skip,
- generation/supersede counter (checked in the `commit` node **and** post-run, so a
  superseded tick never advances state or commits),
- `TICK_TIMEOUT` (30 min) wall clock,
- local-provider gate + rate-cap halt when `engine = "local"`,
- tool-capability classifier (TAURI-RUST-ADC),
- advance-baseline-only-on-success, quiet-tick short-circuit,
- `SqliteCheckpointer` resume of an interrupted tick.

## Memory profile (`profiles/memory.rs`)

A profile implements `observe → prepare_context → reflect → commit` + `origin`:

| Method | Memory behavior |
| --- | --- |
| `observe` | diff connected sources vs the baseline checkpoint |
| `prepare_context` | run the read-only local `context_scout` over the diff |
| `reflect` | hosted Medulla (`flavor: "openhuman"`) or safe local fallback → `Acted` |
| `commit` | re-checkpoint the world baseline after quiet or successful reflection |
| `origin` | tainted iff the diff carried external content |

Hosted Medulla receives exactly `notify_user`, `update_task`, `goals_list`,
`goals_add`, `goals_edit`, and `spawn_subagent`. These tools execute on-device.
Delegation is forced blocking and runs with a root parent context.

The submission boundary is deliberately strict: missing credentials, disabled
or unreachable hosted orchestration, and plan ineligibility are preflight
failures and may fall back locally. Once `/orchestration/v1/run` is submitted,
any transport, continuation, timeout, or terminal-cycle error holds the
baseline and does not run the same window locally.

## Persistence

SQLite at `<workspace>/subconscious/subconscious.db` (per-user workspace). State
keys are **namespaced per instance** (`"<instance>:<key>"`):

- `subconscious_state` — REAL KV: `memory:last_tick_at`.
- `subconscious_state_text` — TEXT KV: `memory:baseline_checkpoint_id`.
- Legacy single-engine keys (`last_tick_at`, `baseline_checkpoint_id`) migrate to
  the `memory:`-namespace via an idempotent, old-version-tolerant UPDATE in the DDL
  batch.
- Tick graph checkpoints: `<workspace>/subconscious/graph_checkpoints.db`, thread
  `subconscious:<instance>:<tick_id>`.

Legacy task/log/escalation/reflection tables are retained for back-compat with
existing DBs but are no longer written or read.

## Key files

| File | Role |
| --- | --- |
| `mod.rs` | Export-focused; re-exports the factory, instance, profile types, session, source_chunk, schemas. |
| `profile.rs` | `SubconsciousProfile` trait + serde `Observation`/`Reflection`. |
| `instance.rs` | `SubconsciousInstance` — the generic tinyagents-graph runner + scheduler/circuit-breaker shell + `status()`/`is_due()`. |
| `hosted.rs` | Hosted Medulla reflection, bounded on-device tool catalogue, trusted origin, and blocking delegation wrapper. |
| `profiles/memory.rs` | `MemoryProfile` + `memory_instance()` — world-diff render, engine selection, local fallback, and baseline. |
| `provider.rs` | Shared subconscious provider routing, rate-cap halt signature, and permanent-error classifiers (world-agnostic). |
| `factory.rs` | `SubconsciousKind` + `make_subconscious` + `enabled_kinds` — the bootstrap set. |
| `registry.rs` | Keyed `HashMap<Kind, Arc<SubconsciousInstance>>`; `get_or_init_instance`, `registered_instances`, `bootstrap_after_login`, user-switch reset. Spawns the heartbeat loop + opt-in trigger orchestrator. |
| `heartbeat/` | Periodic scheduler + event planner; each interval fans out, ticking every registered instance whose cadence has elapsed, concurrently. |
| `store.rs` | SQLite persistence + DDL; instance-namespaced `get/set_last_tick_at`, `get/set_baseline_checkpoint_id` + legacy-key migration. |
| `schemas.rs` | RPC controllers (`subconscious.status` / `subconscious.trigger`). |
| `session.rs`, `source_chunk.rs`, `user_thread.rs`, `agent/` | Unchanged roles (opt-in trigger session, chunk hydration, `notify_user`, the slim agent def). |

## RPC / controllers

Namespace `subconscious` (`openhuman.subconscious_<function>`):

| Function | Purpose |
| --- | --- |
| `status` | Legacy top-level fields mirror the **memory** instance; `instances[]` lists every registered world (each tagged with `instance`). Read entirely from SQLite / the small state mutex — never the tick lock. |
| `trigger` | Fire the memory tick; `"all"` remains a compatibility alias. Spawned; returns immediately. |

## Notes / gotchas

- **Instances bootstrap post-login** (`registry::bootstrap_after_login`) against the
  per-user workspace. The heartbeat loop is the clock that drives every world's tick.
- **`status` never touches the tick mutex** — it reads SQLite + the small state
  mutex, since the tick lock is held for a full tick.
- **State only advances on success.** A failed reflect leaves `last_tick_at` and the
  baseline/cursor in place (re-observe the same window next tick). A superseded tick
  discards its result and skips commit.
- **Quiet ticks short-circuit before reflect** — no LLM call when `observe` is empty;
  the baseline is still refreshed via `commit`.
- **Taint:** hosted and local reflection use `SubconsciousTainted` for windows
  containing external content. Delegated agent turns inherit that origin.
- **The local Medulla child is separate.** `medulla_local` remains a developer
  diagnostic/RPC surface and does not drive subconscious ticks.
