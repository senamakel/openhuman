---
description: >-
  Contributor-facing documentation for OpenHuman — how to build, test, ship, and
  extend the app and core.
icon: code-branch
---

# Overview

OpenHuman is open source under GNU GPL3 and lives at [github.com/tinyhumansai/openhuman](https://github.com/tinyhumansai/openhuman). This section is for contributors and people running OpenHuman from source.

## Where things live

| Path        | What's there                                                                                                       |
| ----------- | ------------------------------------------------------------------------------------------------------------------ |
| `app/`      | Yarn workspace `openhuman-app` — Vite + React frontend (`app/src/`) and the Tauri desktop host (`app/src-tauri/`). |
| `src/`      | Rust crate `openhuman_core` and the `openhuman` CLI binary — domains, MCP routing, JSON-RPC.                       |
| `docs/`     | Remaining deep developer reference (memory pipeline diagrams, telegram-login, sentry, agent flows, etc.).          |
| `gitbooks/` | Public-facing documentation — this site.                                                                           |

The high-level shape lives in [Architecture](../features/architecture.md). The deep developer architecture lives in [Architecture](architecture.md).

## Start here

* [**Getting Set Up**](getting-set-up.md). building from source, toolchain, vendored Tauri CLI, sidecar staging.
* [**Testing Strategy**](testing-strategy.md). Vitest, cargo test, WDIO E2E. Where each test goes.
* [**E2E Testing**](e2e-testing.md). running end-to-end specs locally and in CI.
* [**Release Policy**](release-policy.md). release cadence, version policy, OAuth-and-installer rules.

## Building features

* [**Subconscious Loop**](../features/subconscious.md). background task evaluation against the workspace.

## Working with agents

* [**Coding Harness**](coding-harness.md). the agent's code-focused tool surface and how to extend it.
* [**Agent Observability**](agent-observability.md). the artifact-capture layer that makes E2E tests debuggable.

## Other contributor docs

Anything not yet migrated lives under [`docs/`](https://github.com/tinyhumansai/openhuman/tree/main/docs) in the repo. Notable references:

* [Architecture](architecture.md). canonical architecture.
* [`docs/PROMPT_INJECTION_GUARD.md`](../../docs/PROMPT_INJECTION_GUARD.md). security model.
* [Frontend chapter](frontend.md). React app structure (`app/src/`).
* [Tauri shell chapter](tauri-shell.md). desktop host (`app/src-tauri/`).

[`CLAUDE.md`](../../CLAUDE.md) is the source of truth for AI agents working on the codebase, with the same rules contributors are expected to follow.
