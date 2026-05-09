---
description: >-
  Deep dive into the Memory Tree's async pipeline — how leaves get from "user
  sent a message" to "global digest summarized for the day".
icon: diagram-project
---

# Memory Tree Pipeline

The user-facing pitch of the [Memory Tree](memory-tree.md) is simple: connect a source, the agent gets persistent memory of it. The pipeline that delivers on that pitch is **not** simple — it spans an HTTP-triggered ingest path, a job queue inside SQLite, a pool of background workers, three independent summary trees, and a daily UTC scheduler. This page walks through the whole thing.

The diagram below is the source of truth. Every box maps to code under `src/openhuman/memory/tree/`.

{% file src="../../.gitbook/assets/memory-tree-pipeline (1).excalidraw" %}
Memory Tree Async Pipeline — leaf ingestion → jobs queue → workers → source / topic / global tree building.
{% endfile %}

## The six lanes

The pipeline has six conceptual lanes. Read left to right (1 → 4) for the hot path; bottom row (5 → 6) is independent background flow and the leaf state machine.

### 1. Ingest

Entry point: a JSON-RPC call (or in-process equivalent) carrying chat / email / document content.

```
canonicalize
  → chunk_markdown
  → score_chunks_fast
  → upsert_chunks_tx
  → lifecycle_status = pending_extraction
  → persist fast score rows
  → enqueue extract_chunk per chunk
  → wake_workers()
```

Hot-path requirements:

* **Deterministic.** The `chunk_id` is a hash of `(source_kind, source_id, position, body_hash)`. Re-running ingest on identical input never produces duplicates.
* **Fast.** No LLM calls in this lane. `score_chunks_fast` uses cheap heuristics; deeper scoring runs out of the worker pool.
* **Bounded write.** Everything happens inside one SQLite transaction so a partial ingest can't leave dangling rows.

Code: `src/openhuman/memory/tree/ingest.rs`, `chunker.rs`, `score/fast.rs`.

### 2. Queue

Storage: SQLite at `<workspace>/memory_tree/chunks.db`. Tables:

| Table                   | What's there                                                     |
| ----------------------- | ---------------------------------------------------------------- |
| `mem_tree_chunks`       | The chunks themselves — body hash, provenance, lifecycle status. |
| `mem_tree_score`        | Per-chunk score rows (fast + deep).                              |
| `mem_tree_entity_index` | Entity → chunk lookup for topic-tree hotness.                    |
| `mem_tree_jobs`         | The job queue (see below).                                       |
| `mem_tree_trees`        | Per-scope tree metadata (source / topic / global).               |
| `mem_tree_buffers`      | L0 buffers (unsealed leaves) per tree.                           |
| `mem_tree_summaries`    | Sealed summaries (L1/L2/...) per tree.                           |

`mem_tree_jobs` columns that matter:

| Column                                | Purpose                                                                                        |
| ------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `kind`                                | One of `extract_chunk`, `append_buffer`, `seal`, `topic_route`, `digest_daily`, `flush_stale`. |
| `payload_json`                        | Job-specific args.                                                                             |
| `dedupe_key`                          | Coalesces duplicate enqueues — re-running an idempotent job costs one row, not N.              |
| `status`                              | `pending` / `running` / `done` / `failed`.                                                     |
| `attempts` / `last_error`             | Retry bookkeeping.                                                                             |
| `available_at_ms` / `locked_until_ms` | Scheduling and worker leasing.                                                                 |

Code: `src/openhuman/memory/tree/store.rs`, `jobs/queue.rs`.

### 3. Workers

Bootstrap: `jobs::start(workspace_dir)` is called once at process startup. It:

* Calls `recover_stale_locks()` — any job whose `locked_until_ms` is in the past becomes `pending` again. Crashes don't strand work.
* Spawns **3 worker tasks** (configurable, but 3 is the default and what production runs).
* Wires a `tokio::sync::Notify` so the ingest path can wake workers immediately, with a **5-second polling fallback** so a missed notify doesn't strand work.
* Holds a shared `Semaphore(3)` for LLM-bound steps so concurrent embedding / summarization calls can't blow past the configured budget.

Each worker pulls a job, runs the right handler, and updates the row. Handlers:

| Handler         | Job kind        | What it does                                                                                             |
| --------------- | --------------- | -------------------------------------------------------------------------------------------------------- |
| `extract_chunk` | `extract_chunk` | Deep score + entity extraction. Decides `admitted` vs `dropped` based on the score.                      |
| `append_buffer` | `append_buffer` | Adds an admitted leaf to the source (or topic) tree's L0 buffer. May trigger a seal.                     |
| `seal`          | `seal`          | Compresses L0 buffer into an L1 summary; cascades up through L2/L3/... if the parent buffer is now full. |
| `topic_route`   | `topic_route`   | Routes a leaf into per-entity topic trees, gated by the curator hotness check.                           |
| `digest_daily`  | `digest_daily`  | Builds the global daily digest node.                                                                     |
| `flush_stale`   | `flush_stale`   | Force-seals buffers that have been sitting too long.                                                     |

Code: `src/openhuman/memory/tree/jobs/{worker.rs, handlers/}`.

### 4. Tree state

Three independent trees are built from the same leaf stream.

**Source tree** — one per source. New leaves land in the L0 buffer. When the buffer fills (or `flush_stale` fires), `seal` writes an L1 summary; if the L1 buffer fills, the cascade continues up.

**Topic tree** — one per high-hotness entity. The `topic_route` handler runs a curator check (is this entity hot enough to deserve its own tree?) and, if it passes, calls `append_buffer` against the topic's tree.

**Global tree** — one tree, growing one node per UTC day. The `digest_daily` handler builds yesterday's daily node and `append_daily_and_cascade` walks it up the global hierarchy.

Code: `src/openhuman/memory/tree/{tree_source, tree_topic, tree_global}/`.

### 5. Scheduler / background

A separate scheduler loop runs independently of the ingest path:

* **UTC daily tick.** At 00:00 UTC each day, enqueue `digest_daily(yesterday)` and `flush_stale(today)`. Both go through the same `mem_tree_jobs` pipeline workers consume.
* **`flush_stale`** scans every tree's buffers for ones older than the configured TTL and enqueues force-seal jobs.

The scheduler **does not** run summarizers itself. Everything goes through the queue, so retries, dedupe, and stale-lock recovery all stay centralized.

Code: `src/openhuman/memory/tree/jobs/scheduler.rs`.

### 6. Leaf lifecycle

Each chunk moves through a small state machine:

```
pending_extraction ──► admitted ──► buffered ──► sealed
                  ╲
                   ──► dropped
```

* `extract_chunk` decides `admitted` vs `dropped` based on the deep score.
* `append_buffer` moves admitted leaves into a buffer — `buffered`.
* `seal` writes the buffer's contents into a summary and marks each leaf `sealed`.
* `dropped` leaves stop here. Their chunk row stays for provenance, but no buffer / summary references them.

This is why retrieval can show provenance without re-running the pipeline: the chunk row plus its terminal lifecycle status is enough.

## Why a queue instead of in-process futures

Three reasons:

1. **Crash safety.** A worker panic, a process kill, a power loss — none of them lose admitted-but-not-yet-sealed work. The job row is durable in SQLite; the next start picks it up.
2. **Retries with backoff.** `attempts` + `available_at_ms` + `last_error` give us per-job retry without ad-hoc retry loops in business logic.
3. **One throttle for LLM cost.** All summarization paths share a single semaphore, so a burst of new sources can't accidentally fan out to 50 concurrent embeddings calls.

## Touching this code

A few rules to keep the pipeline coherent:

* **All paths go through `mem_tree_jobs`.** Don't add a feature that does its own background scheduling. If you need a periodic step, enqueue a job; if you need a one-shot async step, enqueue a job.
* **Idempotent handlers.** Every handler must be safe to run twice on the same `(kind, payload, dedupe_key)`. Workers retry on transient errors; you can't assume "called once".
* **No LLM calls in the ingest hot path.** Anything that needs a model goes into the queue.
* **Workspace-scoped.** Tests reset state by creating a fresh `OPENHUMAN_WORKSPACE`. Don't reach outside the workspace dir.

## See also

* [Memory Tree (user-facing)](memory-tree.md) — the product surface this pipeline powers.
* [Memory Context Window](/broken/pages/l8k1mXW6gnYt3tp2WIGd) — how much of the resulting state lands in each agent turn.
* [Local AI](../local-ai.md) — opt-in path for running embeddings + summarization on-device.
