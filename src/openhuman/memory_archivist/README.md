# memory_archivist

Captures conversation turns to disk as Obsidian-compatible markdown.
Placeholder module that will replace
`memory_store::unified::fts5` once the harness + STM recall callers
migrate to its API.

## On disk

```text
<content_root>/episodic/<session_id>/<seq:06>.md
```

Each file:

```markdown
---
session_id: <session>
seq: <0-based>
timestamp_ms: <epoch ms>
role: user | assistant | system | tool
cost_microdollars: <u64>
lesson: "<optional>"
tool_calls_json: "<optional json blob>"
---

<turn body>
```

Bodies are immutable after first write — same contract as
`content::atomic::write_if_new`.

## API

| Function | Replaces |
| --- | --- |
| `record_turn(config, ArchivedTurn) -> ArchivedTurn` | `fts5::episodic_insert` |
| `session_entries(config, session_id) -> Vec<ArchivedTurn>` | `fts5::episodic_session_entries` |

Cross-session search is not implemented here yet — the right answer is
to grep the md files once the grep tooling lands. Until then, callers
that need cross-session lookup keep using `fts5::episodic_cross_session_search`.

## Layout

| Path | Role |
| --- | --- |
| [`mod.rs`](mod.rs) | Module root + re-exports. |
| [`types.rs`](types.rs) | `ArchivedTurn` — serde mirror of the legacy `EpisodicEntry`. |
| [`store.rs`](store.rs) | `record_turn`, `session_entries`, YAML compose/parse, atomic write. |

## Layer rules

- Depends on `memory_store::content::atomic::write_if_new` for the write
  contract. Nothing else from memory_store.
- No SQLite. No async. No upward deps.
