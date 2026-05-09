---
description: >-
  TokenJuice — a rule overlay that compacts verbose tool output before it ever
  enters LLM context. Sweeping through thousands of emails stays cheap.
icon: file-zipper
---

# Smart Token Compression (TokenJuice)

LLM tokens are expensive — and verbose tool output is where most of them go to die. A `git status` in a busy repo, a `cargo build` log, a 600-message email thread, a `docker ps -a` against a real cluster — each of these can balloon a context window for almost no information gain.

OpenHuman ships with **TokenJuice**, a port of [vincentkoc/tokenjuice](https://github.com/vincentkoc/tokenjuice) integrated directly into the tool-execution path. Before any tool result reaches the model, TokenJuice runs the output through a rule overlay that strips the noise and keeps the signal.

## Three-layer rule overlay

Rules are JSON, and they merge in this order — later layers override earlier ones:

| Layer | Path | Purpose |
| --- | --- | --- |
| **Builtin** | shipped with the binary | sensible defaults for git, npm, cargo, docker, kubectl, ls, etc. |
| **User** | `~/.config/tokenjuice/rules/` | your personal overrides — apply across every project |
| **Project** | `.tokenjuice/rules/` | repo-specific overrides — checked in, shared with the team |

Each rule names a tool/command pattern and a reduction strategy (truncate, dedup lines, fold whitespace, drop matching regexes, summarize sections, …). New rules are just JSON files; no recompile required.

## Why this matters for memory

TokenJuice is what makes [auto-fetch](auto-fetch.md) economically viable. When the Gmail provider syncs a page of 200 messages, TokenJuice compacts each canonicalized email *before* it enters the model that builds summaries. The same applies to GitHub diffs, Slack channel dumps, and any other firehose source.

Concretely: ingesting your last six months of email through a frontier model costs single-digit dollars instead of hundreds.

## Where it lives in the pipeline

```
tool call result
      │
      ▼
TokenJuice (classify → match rule → reduce)
      │
      ▼
LLM context
```

Implementation: `src/openhuman/tokenjuice/` (`classify.rs`, `reduce.rs`, `rules/compiler.rs`, `tool_integration.rs`).

## Inspecting and overriding

- Drop a JSON file in `~/.config/tokenjuice/rules/` to add or override a rule globally.
- Drop one in `.tokenjuice/rules/` inside a repo to do the same per-project.
- Start the core with `RUST_LOG=openhuman_core::openhuman::tokenjuice=debug` to see what's matching and how much output is being trimmed.

## See also

- [Native Tools](native-tools.md) — most heavy tool output flows through TokenJuice.
- [Memory Tree](memory-tree.md) — the downstream consumer of compressed output.
