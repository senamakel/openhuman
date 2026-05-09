---
description: >-
  Contributor-facing documentation for OpenHuman — how to build, test, ship,
  and extend the app and core.
icon: code-branch
---

# Developing

OpenHuman is open source under GNU GPL3 and lives at [github.com/tinyhumansai/openhuman](https://github.com/tinyhumansai/openhuman). This section is for contributors and people running OpenHuman from source.

## Where things live

| Path | What's there |
| --- | --- |
| `app/` | Yarn workspace `openhuman-app` — Vite + React frontend (`app/src/`) and the Tauri desktop host (`app/src-tauri/`). |
| `src/` | Rust crate `openhuman_core` and the `openhuman` CLI binary — domains, MCP routing, JSON-RPC. |
| `docs/` | Deep developer reference (frontend chapter docs, Tauri shell chapters, QA matrices, internals). |
| `gitbooks/` | Public-facing documentation — this site. |

The high-level shape lives in [Architecture](../technology/architecture.md). The deep developer architecture lives in [`docs/ARCHITECTURE.md`](https://github.com/tinyhumansai/openhuman/blob/main/docs/ARCHITECTURE.md).

## Start here

- **[Getting Set Up](getting-set-up.md)** — building from source, toolchain, vendored Tauri CLI, sidecar staging.
- **[Testing Strategy](testing-strategy.md)** — Vitest, cargo test, WDIO E2E. Where each test goes.
- **[E2E Testing](e2e-testing.md)** — running end-to-end specs locally and in CI.
- **[Release Policy](release-policy.md)** — release cadence, version policy, OAuth-and-installer rules.

## Building features

- **[Skills](skills.md)** — how skills are discovered, fetched, installed, initialized, executed and synced.
- **[Subconscious Loop](subconscious.md)** — background task evaluation against the workspace.
- **[Conscious Loop](conscious-loop.md)** — actionable-item extraction from the memory tree.
- **[Webview Integration](webview-integration.md)** — adding a new third-party webview-based integration.

## Working with agents

- **[Coding Harness](coding-harness.md)** — the agent's code-focused tool surface and how to extend it.
- **[Agent Observability](agent-observability.md)** — the artifact-capture layer that makes E2E tests debuggable.

## Other contributor docs

Anything not yet migrated lives under [`docs/`](https://github.com/tinyhumansai/openhuman/tree/main/docs) in the repo. Notable references:

- [`docs/ARCHITECTURE.md`](https://github.com/tinyhumansai/openhuman/blob/main/docs/ARCHITECTURE.md) — canonical architecture.
- [`docs/MEMORY_CONTEXT_WINDOW.md`](https://github.com/tinyhumansai/openhuman/blob/main/docs/MEMORY_CONTEXT_WINDOW.md) — memory subsystem internals.
- [`docs/PROMPT_INJECTION_GUARD.md`](https://github.com/tinyhumansai/openhuman/blob/main/docs/PROMPT_INJECTION_GUARD.md) — security model.
- [`docs/skills-runtime-isolation.md`](https://github.com/tinyhumansai/openhuman/blob/main/docs/skills-runtime-isolation.md) — QuickJS isolation guarantees.
- [`docs/src/`](https://github.com/tinyhumansai/openhuman/tree/main/docs/src) — frontend chapter docs.
- [`docs/src-tauri/`](https://github.com/tinyhumansai/openhuman/tree/main/docs/src-tauri) — Tauri shell chapter docs.

[`CLAUDE.md`](https://github.com/tinyhumansai/openhuman/blob/main/CLAUDE.md) is the source of truth for AI agents working on the codebase, with the same rules contributors are expected to follow.
