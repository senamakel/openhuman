---
description: >-
  OpenHuman is a personal AI assistant that runs on your desktop, connects to
  118+ services, and builds a local-first memory of your life from them.
icon: diamond
---

# Welcome to OpenHuman

<figure><img src=".gitbook/assets/demo.png" alt=""><figcaption></figcaption></figure>

OpenHuman is an open-source AI assistant designed to be the **memory** and **doer** for everything you do across your tools. Built on Rust + Tauri and licensed under GNU GPL3, it closes the gap between what AI models can do and what they actually know about _you_.

Every model in the world, all 200+ of them, shares the same fundamental limitation: they are stateless. You type a prompt, get a response, and the context evaporates. Even the ones with "memory" store a few bullet points. A few bullet points is a sticky note, not intelligence.

OpenHuman solves this with a stack that's calmly, deliberately different:

**A local-first** [**Memory Tree**](features/obsidian-wiki/memory-tree.md)**.** Every source you connect. Gmail, Slack, GitHub, Notion, your own notes, flows through a deterministic pipeline: canonical Markdown, ≤3k-token chunks, scored, folded into per-source / per-topic / per-day summary trees. Stored in SQLite on your machine. No vector-soup black box.

**An** [**Obsidian-style wiki**](features/obsidian-wiki/) **on top of it.** The same chunks the agent reasons over land as `.md` files in a vault you can open in [Obsidian](https://obsidian.md), browse, edit, and link by hand. Inspired by [Karpathy's obsidian-wiki workflow](https://x.com/karpathy/status/2039805659525644595). You can't trust a memory you can't read.

[**118+ third-party integrations**](features/integrations.md)**.** One-click OAuth into Gmail, GitHub, Slack, Notion, Stripe, Calendar, Drive, Linear, Jira and more — no API keys to wire by hand, no plugin marketplace to navigate.

[**Auto-fetch**](features/obsidian-wiki/auto-fetch.md)**.** Every twenty minutes, OpenHuman pulls fresh data from every active connection and folds it into the Memory Tree without you asking, so the agent already has tomorrow's context this morning.

**An agent built for big data.** [Smart token compression (TokenJuice)](features/token-compression.md) compacts verbose tool output before it ever enters the model's context, so sweeping through your last six months of email costs single-digit dollars. [Automatic model routing](features/model-routing/) sends each task to the right model — `hint:reasoning` to a frontier model, `hint:fast` to a cheap one, vision to vision — all under one subscription. Optional [local AI via Ollama](features/model-routing/local-ai.md) keeps embeddings and summarization on-device.

[**Batteries included**](features/native-tools/README.md)**.** A complete agent toolbelt is wired in by default: [web search](features/native-tools/web-search.md), a [web-fetch scraper](features/native-tools/web-scraper.md), a full [coder toolset](features/native-tools/coder.md) (filesystem, git, lint, test, grep), [browser & computer control](features/native-tools/browser-and-computer.md), [cron & scheduling](features/native-tools/cron.md), [memory tools](features/native-tools/memory-tools.md), [agent coordination](features/native-tools/agent-coordination.md) for spawning sub-agents, and [native voice](features/native-tools/voice.md) — STT in, TTS out, mascot lip-sync, and a live Google Meet agent that joins meetings, transcribes them into your Memory Tree, and can speak back into the call. No "install a plugin to read files" friction.

**Simple, UI-first.** A clean desktop experience and short onboarding paths take you from install to a working agent in a few clicks — no config-first setup, no terminal required. The agent has [a face](features/mascot.md): a desktop mascot that speaks, reacts to its surroundings, joins your Google Meets as a real participant, remembers you across weeks, and keeps thinking in the background even when you've stopped typing.

Together, these turn OpenHuman into something fundamentally different from a chatbot. It is an AI agent that consumes large amounts of personal data at low cost, maintains a persistent and evolving understanding of your world, and takes proactive actions on your behalf.

{% hint style="info" %}
OpenHuman is not AGI. But it is a meaningful architectural step closer, with better memory, better orchestration, and better economics for the long-context workloads that matter.
{% endhint %}

## What OpenHuman does

OpenHuman connects to your tools, pulls from them continuously, and turns the firehose into something structured any AI can act on.

* It **fetches automatically** from every active integration every twenty minutes, so the agent already has tomorrow's context this morning.
* It **compresses** millions of tokens of organizational noise into a deterministic Memory Tree of chunks, scores, entities, and summaries.
* It **surfaces signals** that matter: decisions, action items, risks, sentiment shifts, and buried context you would otherwise miss.
* It **routes intelligently**, picking the right model for each task and compacting tool output through TokenJuice so cost stays minimal even at scale.
* It **speaks**, listens, and joins meetings, voice is a first-class surface, not an afterthought.
* It **preserves privacy** by design. The Memory Tree's SQLite database and your Obsidian vault stay on your machine. Integration tokens are held by the OpenHuman backend, not on disk in plaintext on your laptop.

## Who it's for

OpenHuman is built for people and teams who operate across many conversations and tools, and feel the cost of it.

* **Knowledge workers** who spend their days across 8+ applications and lose context every time they switch.
* **Developers and power users** who want a memory and context layer that actually scales, and that they can audit by opening a folder of Markdown.
* **High-volume communicators** who miss decisions, context, and follow-ups buried in message noise across multiple platforms.
* **Traders and analysts** who need fast signal extraction and risk awareness across information channels.
* **Distributed teams** who make decisions in chat but need structured follow-through in external tools.

## What OpenHuman does not do

OpenHuman does not claim to be AGI. It does not take actions in your connected platforms without your explicit instruction. It does not store your raw message data on someone else's server. It does not train on your data.

{% hint style="info" %}
Privacy is a core architectural decision, not a checkbox. The full privacy design is covered in [Privacy & Security](features/privacy-and-security.md).
{% endhint %}

## How to think about OpenHuman

ChatGPT, Claude, Gemini and every other model are the brain. They are brilliant at reasoning. But they are amnesiac. They know nothing about your actual life.

OpenHuman is the memory and the doer that makes those brains actually useful. It is the context engine that compresses your entire organizational life into intelligence any AI can act on, stored as Markdown you own.

Your AI is smart. It just does not know you. OpenHuman fixes that.
