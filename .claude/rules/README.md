# `.claude/rules/`

This directory is intentionally near-empty.

Authoritative docs for AI agents and contributors:

- **[`CLAUDE.md`](../../CLAUDE.md)** — repo layout, runtime scope, commands, frontend/Tauri/Rust conventions, testing, debug logging, feature workflow.
- **[`AGENTS.md`](../../AGENTS.md)** — RPC controller patterns, `RpcOutcome<T>` contract.
- **[`gitbooks/developing/architecture.md`](../../gitbooks/developing/architecture.md)** — narrative architecture, dual-socket sync.
- **[`gitbooks/resources/design-language.md`](../../gitbooks/resources/design-language.md)** — visual language.
- **[`gitbooks/developing/e2e-testing.md`](../../gitbooks/developing/e2e-testing.md)** — WDIO/Appium testing.
- **[`gitbooks/developing/frontend.md`](../../gitbooks/developing/frontend.md)** — frontend.
- **[`gitbooks/developing/tauri-shell.md`](../../gitbooks/developing/tauri-shell.md)** — Tauri shell.

## When to add a file here

Only add a `*.md` file in this directory if you need **path-gated context** loaded conditionally by Claude Code (via the `paths:` frontmatter) for a narrow part of the tree, AND the content is not already covered in `CLAUDE.md`.

Each file added here ships in every agent context that matches its `paths:` glob — so keep them small, current, and non-overlapping with `CLAUDE.md`. Stale rules actively mislead agents.
