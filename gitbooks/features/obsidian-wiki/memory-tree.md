---
description: >-
  OpenHuman's local-first knowledge base. Ingest from your tools, canonicalize
  into Markdown, chunk, score, and fold into hierarchical summary trees.
icon: tree
---

# Memory Tree

The Memory Tree is OpenHuman's knowledge base. It is not a vector database with a thin "memory" wrapper. It is a deterministic, bucket-sealed pipeline that turns the messy stream of your day — chats, emails, documents, integration sync results — into structured, queryable, summary-backed Markdown that lives on your machine.

## What it does

Every source you connect feeds the same pipeline:



```
source adapters (chat / email / document)
        │
        ▼
canonicalize    ── normalised Markdown + provenance metadata
        │
        ▼
chunker         ── deterministic IDs, ≤3k-token bounded segments
        │
        ▼
content_store   ── atomic .md files on disk (body + tags)
        │
        ▼
store           ── SQLite persistence (chunks, scores, summaries, jobs)
        │
        ▼
score           ── signals + embeddings + entity extraction
        │
        ▼
tree_source / tree_topic / tree_global   ── per-scope summary trees
        │
        ▼
retrieval       ── search · drill_down · topic · global · fetch
```

The hot path (`canonicalize → chunk → stage → fast-score → persist → enqueue extract jobs`) is fast. Heavy work — embeddings, entity extraction, sealing summary buckets, daily digests — runs in background workers out of the `jobs/` queue so the UI never blocks.

Embeddings and summary-tree building can run **on-device via Ollama** if you turn on [Local AI](../local-ai.md); otherwise they go through the OpenHuman backend like any other model call.

## Three trees, three scopes

* **Source trees** (`tree_source/`) — per-source rolling buffer (L0) that seals into L1 → L2 → … as it fills. One per Gmail label, one per Slack channel, one per uploaded document, etc.
* **Topic trees** (`tree_topic/`) — per-entity summaries materialized lazily by _hotness_. The more an entity (person, project, ticker, repo) shows up, the more aggressively its topic tree is built and refreshed.
* **Global tree** (`tree_global/`) — daily global digest across everything ingested that day.

Retrieval can target any scope: search a single source, drill down a topic, or pull the global digest.

## Where it lives on disk

Inside your workspace (default `~/.openhuman`, or whatever `OPENHUMAN_WORKSPACE` points at):

| Path                    | What's there                                                    |
| ----------------------- | --------------------------------------------------------------- |
| `memory_tree/chunks.db` | SQLite — chunks, scores, summaries, entity index, jobs, hotness |
| `wiki/`                 | The Markdown vault — see [Obsidian Wiki](./)                    |

Everything is local. Nothing about your raw data leaves your machine unless you explicitly send a chat message that includes it.

## Why a tree, not a vector store

Vector stores answer "what is similar to this query?" Memory needs to answer more than that:

* **What happened today?** (global digest)
* **What's the latest on this person?** (topic tree, hotness-driven)
* **What did the Stripe webhook say last Tuesday at 3pm?** (source tree + provenance)

Trees give you compression _and_ navigation. Embeddings still live inside (in `score/`) so semantic search keeps working — but the structure on top is what makes the memory feel like a brain instead of a bag of fragments.

## Triggering ingest

* **Automatic** — every active integration is auto-fetched every five minutes; see [Auto-fetch](../auto-fetch.md).
* **Manual** — the Memory tab in the desktop app exposes a "Run ingest" trigger per source.
* **RPC** — `openhuman.memory_tree_ingest` for advanced workflows.

## See also

* [Obsidian Wiki](./) — open the vault in Obsidian and edit it directly.
* [Auto-fetch from Integrations](../auto-fetch.md) — how the tree stays fresh.
* [Smart Token Compression](../token-compression.md) — what makes ingesting "everything" cheap.
* [Local AI (optional)](../local-ai.md) — opt in to keep embeddings and summary-tree building on-device.
* [Memory Tree Pipeline](memory-tree-pipeline.md) — contributor-facing deep dive on the async queue, workers and tree-state machine.
