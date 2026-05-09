---
description: >-
  The Intelligence tab shows what OpenHuman knows and how its memory tree is
  growing. Open it from the bottom navigation bar.
icon: brain-circuit
---

# Intelligence

### System Status

The top of the page shows the current system state (idle, ingesting, summarizing) and a **Run ingest** button to manually trigger a sync against any connected source.

### Memory Tree

The Memory tab surfaces the live state of the [Memory Tree](../features/memory-tree.md):

| Metric | What it shows |
| --- | --- |
| **Storage** | Total size of `<workspace>/memory_tree/chunks.db` and the Obsidian vault. |
| **Sources** | How many distinct sources have been ingested (one per Gmail label, Slack channel, document, etc.). |
| **Chunks** | Total ≤3k-token chunks in the store. |
| **Topics** | Number of topic trees materialized so far (per-entity summaries built from "hot" entities). |
| **First / latest memory** | Timestamps of the oldest and newest chunks. |

### Memory Graph

A force-directed visualization of entities and their relationships, drawn from the Memory Tree's entity index. The graph grows as auto-fetch pulls more data; sparse early on, denser within a few days of usage.

### Obsidian vault

A **View vault in Obsidian** button opens `<workspace>/wiki/` directly via an `obsidian://open?path=...` deep link. See [Obsidian Wiki](../features/obsidian-wiki.md). You can also open the folder in any file browser — it's just Markdown.

### Ingestion Activity

A heatmap showing ingest events over time, similar to a GitHub contribution graph. Useful for spotting periods where auto-fetch was idle (e.g. a connection broke and stopped syncing).

### Search & retrieval

A search bar over your Memory Tree. Source-scoped, topic-scoped or global queries are all supported, and any result links back to the underlying chunk file in your Obsidian vault for full provenance.

### Routing

The Intelligence tab also surfaces which model the agent is using per task — see [Automatic Model Routing](../features/model-routing.md) for the full picture of how `hint:reasoning`, `hint:fast`, etc. resolve to providers.
