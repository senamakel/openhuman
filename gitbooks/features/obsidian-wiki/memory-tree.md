---
description: >-
  OpenHuman's local-first knowledge base. Ingest from your tools, canonicalize
  into Markdown, chunk, score, and fold into hierarchical summary trees.
icon: tree
---

# Memory Tree

The Memory Tree is OpenHuman's knowledge base. It is not a vector database with a thin "memory" wrapper. It is a deterministic, bucket-sealed pipeline that turns the messy stream of your day, chats, emails, documents, integration sync results, into structured, queryable, summary-backed Markdown that lives on your machine.

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

The hot path (`canonicalize → chunk → stage → fast-score → persist → enqueue extract jobs`) is fast. Heavy work, embeddings, entity extraction, sealing summary buckets, daily digests, runs in background workers out of the `jobs/` queue so the UI never blocks.

Embeddings and summary-tree building can run **on-device via Ollama** if you turn on [Local AI](../model-routing/local-ai.md); otherwise they go through the OpenHuman backend like any other model call.

## Three trees, three scopes

* **Source trees** (`tree_source/`), per-source rolling buffer (L0) that seals into L1 → L2 → … as it fills. One per Gmail label, one per Slack channel, one per uploaded document, etc.
* **Topic trees** (`tree_topic/`), per-entity summaries materialized lazily by _hotness_. The more an entity (person, project, ticker, repo) shows up, the more aggressively its topic tree is built and refreshed.
* **Global tree** (`tree_global/`), daily global digest across everything ingested that day.

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

Trees give you compression _and_ navigation. Embeddings still live inside (in `score/`) so semantic search keeps working, but the structure on top is what makes the memory feel like a brain instead of a bag of fragments.

## Triggering ingest

* **Automatic**. every active integration is auto-fetched every twenty minutes; see [Auto-fetch](../integrations/auto-fetch.md).
* **Manual**. the Memory tab in the desktop app exposes a "Run ingest" trigger per source.
* **RPC**. `openhuman.memory_tree_ingest` for advanced workflows.

## In the desktop app — the Intelligence tab

Open it from the bottom navigation bar.

**System status.** The top of the page shows the current state (idle, ingesting, summarizing) and a **Run ingest** button to manually trigger a sync against any connected source.

**Memory metrics:**

| Metric | What it shows |
| --- | --- |
| **Storage** | Total size of `<workspace>/memory_tree/chunks.db` and the Obsidian vault. |
| **Sources** | How many distinct sources have been ingested (one per Gmail label, Slack channel, document, etc.). |
| **Chunks** | Total ≤3k-token chunks in the store. |
| **Topics** | Number of topic trees materialized so far (per-entity summaries built from "hot" entities). |
| **First / latest memory** | Timestamps of the oldest and newest chunks. |

**Memory graph.** A force-directed visualization of entities and their relationships, drawn from the entity index. The graph grows as auto-fetch pulls more data, sparse early on, denser within a few days.

**Obsidian vault.** A **View vault in Obsidian** button opens `<workspace>/wiki/` directly via an `obsidian://open?path=...` deep link. You can also open the folder in any file browser.

**Ingestion activity.** A heatmap showing ingest events over time, similar to a GitHub contribution graph. Useful for spotting periods where auto-fetch was idle (e.g. a connection broke and stopped syncing).

**Search & retrieval.** A search bar over the Memory Tree. Source-scoped, topic-scoped or global queries are all supported, and any result links back to the underlying chunk file in your Obsidian vault for full provenance.

**Routing.** The Intelligence tab also surfaces which model the agent is using per task, see [Automatic Model Routing](../model-routing/README.md).

## See also

* [Obsidian Wiki](./). open the vault in Obsidian and edit it directly.
* [Auto-fetch from Integrations](../integrations/auto-fetch.md). how the tree stays fresh.
* [Smart Token Compression](../token-compression.md). what makes ingesting "everything" cheap.
* [Local AI (optional)](../model-routing/local-ai.md). opt in to keep embeddings and summary-tree building on-device.
* [Memory Tree Pipeline](memory-tree-pipeline.md). contributor-facing deep dive on the async queue, workers and tree-state machine.
