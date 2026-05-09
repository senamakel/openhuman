---
description: >-
 Built-in web search, web-fetch scraper, and a full filesystem / git / lint /
 test / grep coder toolset. No "install a plugin" friction.
icon: toolbox
---

# Native Tools (search, scraper, coder)

OpenHuman's agent has the tools you'd expect from a serious coding-and-research assistant out of the box. None of these are MCP servers you have to wire up; they ship inside the core.

## Native web search

`src/openhuman/tools/impl/network/web_search.rs`

A search tool the agent can call directly. Backed by a server-side proxy (Parallel) so you don't carry a search API key. Returns titles, snippets and URLs ready for the agent to follow up on.

Use it for: research, "what's the latest on X", citation hunting.

## Native web scraper

`src/openhuman/tools/impl/network/web_fetch.rs`

A purpose-built "GET-and-read" tool, separate from generic `http_request` / `curl`:

- Caps response at 1 MB.
- 20-second timeout.
- Strips boilerplate; returns clean text the agent can reason over.

Use it for: reading articles, blog posts, docs pages, GitHub READMEs without the noise.

## Native coder

`src/openhuman/tools/impl/filesystem/`

A complete toolset for working on real codebases:

| Tool | What it does |
| --- | --- |
| `file_read` | Read a file (with line numbers, like `cat -n`). |
| `file_write` | Write a new file. |
| `edit_file` | Targeted edits — match-and-replace with strict uniqueness checks. |
| `apply_patch` | Apply a unified diff. |
| `glob_search` | Find files by glob pattern. |
| `grep` | Ripgrep-style search across the tree. |
| `git_operations` | Status, diff, log, blame, branch, commit. |
| `run_linter` | Run the project's linter. |
| `run_tests` | Run the project's test command. |

Together this is what makes OpenHuman a viable coding partner instead of a chat window that *pretends* to know the codebase.

## Why ship them natively

A plugin-only model means tools live in different processes, behind RPC, with their own auth and packaging stories. That's fine for open-ended extensibility — but for the **core** tools every agent needs (read a file, search the web, edit code), shipping them in-process means:

- Consistent error handling.
- Zero install friction.
- All output passes through [TokenJuice](token-compression.md) for free.
- Predictable security boundary (filesystem tools respect workspace scoping).

## See also

- [Smart Token Compression](token-compression.md) — what keeps tool output costs bounded.
- [Third-party Integrations](integrations.md) — for the long tail of third-party services.
