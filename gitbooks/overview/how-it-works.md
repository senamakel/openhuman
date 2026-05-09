---
icon: brain
---

# How It Works

OpenHuman has a simple shape: connect your tools, let it pull from them continuously, watch the memory tree grow, and ask the agent anything across the whole picture. The interesting parts are in the layers between those steps.

## The four moving parts

### 1. A local-first knowledge base — the [Memory Tree](../features/obsidian-wiki/memory-tree.md)

Everything OpenHuman knows about you lives in a SQLite database and a Markdown vault inside your workspace. The pipeline is:

```
source → canonical Markdown → ≤3k-token chunks → score → summary trees
```

Three trees layer on top: per-source, per-topic (entities), and a daily global digest. Embeddings, hotness scoring and entity extraction all run locally; nothing about your raw data leaves your machine.

### 2. An [Obsidian-style wiki](../features/obsidian-wiki/) on top of that knowledge base

The same chunks the agent reasons over are written as `.md` files in `<workspace>/wiki/`. You can open the vault in [Obsidian](https://obsidian.md), browse it, edit it, drop in your own notes, and the agent will see your edits next ingest. This is directly inspired by Karpathy's obsidian-wiki workflow.

You can't trust a memory you can't read. The vault is the inverse of the usual "AI memory" black box.

### 3. [third-party integrations](../features/integrations/README.md) feeding the tree on autopilot

OpenHuman ships with **118+ third-party integrations**. Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira, and more. Connecting any of them is a one-click OAuth flow.

Once connected, the [auto-fetch scheduler](../features/integrations/auto-fetch.md) ticks every twenty minutes, pulls fresh data from every active connection, and pipes the results through the same ingest path the manual UI uses. By the time you ask "what landed in my inbox overnight?", the answer is already in the memory tree.

### 4. An agent with the right tools, the right model, and a budget

When you talk to the agent, four things happen behind the scenes:

* **Model routing**. The model parameter can be a hint (`hint:reasoning`, `hint:fast`, `hint:vision`). The [router](../features/model-routing/README.md) resolves the hint to the right provider+model. One subscription, many models.
* **Native tools**. A built-in [web search, web-fetch scraper, and full filesystem/git/lint/test/grep coder toolset](../features/native-tools.md) are wired in by default. No "install a plugin to read files" friction.
* **TokenJuice compression**. Verbose tool output (git logs, large emails, build output) is compacted by a [rule overlay](../features/token-compression.md) before it ever enters the model's context. Sweeping through your last six months of email costs single-digit dollars instead of hundreds.
* **Voice, when you want it**. STT in, [ElevenLabs TTS](../features/voice.md) out, with a live Google Meet agent that can listen, take notes, and speak back into the call.

## How they connect

```
┌────────────────────────────────────────────────────────────┐
│ Third-party services (118+) │
│ ▲ ▲ │
│ one-click auto-fetch every 20 min │
└──────┼───────────┼─────────────────────────────────────────┘
 │ │
 ▼ ▼
┌────────────────────────────────────────────────────────────┐
│ Memory Tree (canonical MD → chunks → scored → summaries) │
│ │ │
│ ├─ SQLite ──────────── agent retrieval │
│ └─ Markdown vault ──── you, in Obsidian │
└────────────────────────────────────────────────────────────┘
 ▲ │
 │ ▼
 agent reads ┌────────────────────────────────┐
 │ Agent (model router) │
 │ + native tools │
 │ + TokenJuice compression │
 │ + voice in/out (ElevenLabs) │
 └────────────────────────────────┘
```

## What stays on your machine

* The Memory Tree SQLite database (`<workspace>/memory_tree/chunks.db`).
* The Markdown vault (`<workspace>/wiki/`).
* Audio capture and dictation buffers.
* Any local model state.

What goes through the OpenHuman backend: model calls (under one subscription), web search proxy, integration OAuth tokens, TTS streaming. See [Privacy & Security](../features/privacy-and-security.md) for the full boundary.

## Limitations

OpenHuman runs on probabilistic models. It can miss nuance, mishandle sarcasm, or weight things wrong, especially in noisy informal threads with limited prior context. Auto-fetch is bound by the rate limits of each integration, so very high-volume sources may lag the global tick by a few minutes. The product is in early beta; expect rough edges and breaking changes.
