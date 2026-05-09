<h1 align="center">OpenHuman</h1>

<p align="center">
<a href="https://trendshift.io/repositories/23680" target="_blank"><img src="https://trendshift.io/api/badge/repositories/23680" alt="tinyhumansai%2Fopenhuman | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
</p>

<p align="center">
 <strong>The age of super intelligence is here. OpenHuman is your Personal AI super intelligence. Private, Simple and extremely powerful.</strong>
</p>

<p align="center">
 <a href="https://discord.tinyhumans.ai/">Discord</a> •
 <a href="https://www.reddit.com/r/tinyhumansai/">Reddit</a> •
 <a href="https://x.com/intent/follow?screen_name=tinyhumansai">X/Twitter</a> •
 <a href="https://tinyhumans.gitbook.io/openhuman/">Docs</a>
</p>
<p align="center">
 <a href="https://x.com/intent/follow?screen_name=senamakel">Follow @senamakel (Creator)</a>
</p>

<p align="center">
 <img src="https://img.shields.io/badge/status-early%20beta-orange" alt="Early Beta" />
 <img src="https://img.shields.io/badge/platform-desktop-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platforms: desktop only" />
 <a href="https://github.com/tinyhumansai/openhuman/releases/latest"><img src="https://img.shields.io/github/v/release/tinyhumansai/openhuman?label=latest" alt="Latest Release" /></a>
</p>

<p align="center">
 <img src="./docs/the-tet.png" alt="The Tet" />
</p>

<p align="center" style="font-style: italic">
 "The Tet. What a brilliant machine" — Morgan Freeman as he reminisces about <a href="https://youtu.be/SveLVpqy_Rc?si=y83aZNokPiUjILN0&t=60">alien superintelligence</a> in the movie <em>Oblivion</em>
</p>

> **Early Beta** — Under active development. Expect rough edges.

To install or get started, either download from the website over at [tinyhumans.ai/openhuman](https://tinyhumans.ai/openhuman) or run

```
# For MacOS/Linux
curl -fsSL https://raw.githubusercontent.com/tinyhumansai/openhuman/main/scripts/install.sh | bash

# For Windows
irm https://raw.githubusercontent.com/tinyhumansai/openhuman/main/scripts/install.ps1 | iex
```

# What is OpenHuman?

OpenHuman is an open-source agentic assistant that is designed to integrate with you in your daily life. Here's what makes OpenHuman special:

- **Simple, UI-first** — A **clean** desktop experience and short onboarding paths so you can go from install to a **working agent in a few clicks**, without a config-first setup. You don't need a terminal to run OpenHuman.

- **One subscription, many providers** — You only need **one** account to get access to many agentic APIs (AI models, search, webhooks/tunnels and other third-party APIs), simplifying the experience to get a powerful agent going.

- **118+ third-party integrations** — Plug into **Gmail**, **Notion**, **GitHub**, **Slack**, **Stripe**, **Calendar**, **Drive**, **Linear**, **Jira** and the rest of your stack with **one-click OAuth**. No API keys to wire by hand, and every connection is exposed to the agent as a typed tool.

- **Memory Tree + Obsidian Wiki** — A **local-first** knowledge base built from your data and your activity. Everything you connect is canonicalized into Markdown chunks, scored, and folded into hierarchical summary trees stored in **SQLite on your machine**. The same chunks land as `.md` files in an **Obsidian-compatible vault** you can open, browse and edit directly — inspired by Karpathy's [obsidian-wiki workflow](https://x.com/karpathy/status/2039805659525644595).

- **Auto-fetch from your stack** — Every five minutes the core walks each active connection and pulls fresh data into the memory tree (e.g. Gmail → canonical Markdown → chunks → summaries). No prompts, no polling loops you have to write — the agent already has tomorrow's context this morning.

- **Smart token compression (TokenJuice)** — Verbose tool output (git, npm, cargo, docker, large emails) is compacted by a **rule overlay** before it ever enters LLM context. Sweeping through thousands of emails or large repos stays **cheap** because the model never sees the noise.

- **Automatic model routing** — Tasks pick their model. A `hint:reasoning` request lands on a strong reasoning model, `hint:fast` on a fast one, vision goes to a vision model — all under **one subscription**, with zero per-provider key juggling.

- **Native voice (ElevenLabs)** — STT in, **ElevenLabs** TTS out, with mascot lip-sync. Includes a live **Google Meet agent** that listens, takes notes and can speak back in your meetings.

- **Native search, scraper and coder** — A built-in **web search**, **web-fetch** scraper, and a full **filesystem / git / lint / test / grep** toolset are wired into the agent out of the box. No "install a plugin to read files" friction.

- **Optional local AI (via Ollama)** — Off by default. Opt in per-workload to keep **memory embeddings**, **summary-tree building**, and the **background reflection loops** (heartbeat / learning / subconscious) on your machine. Chat / vision / voice stay cloud — the local path is **scoped to the workloads where on-device actually pays**, not a "run everything on Gemma 3" overreach.

Architecture: [Architecture](https://tinyhumans.gitbook.io/openhuman/developing/architecture). Contributor orientation: [`CONTRIBUTING.md`](./CONTRIBUTING.md). Running from source: [Getting Set Up](https://tinyhumans.gitbook.io/openhuman/developing/getting-set-up). Cloud-hosting the headless core: [Cloud Deploy](https://tinyhumans.gitbook.io/openhuman/developing/cloud-deploy).

## Highlights

- **[Memory Tree](https://tinyhumans.gitbook.io/openhuman/features/memory-tree)** — local-first knowledge base: ingest → canonical Markdown → ≤3k-token chunks → SQLite with hierarchical summary trees.
- **[Obsidian Wiki](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki)** — every memory chunk also lives as a `.md` file in an Obsidian-compatible vault you can open and edit (Karpathy-style).
- **[Third-party Integrations (118+)](https://tinyhumans.gitbook.io/openhuman/features/integrations)** — one-click OAuth into Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira and more.
- **[Auto-fetch](https://tinyhumans.gitbook.io/openhuman/features/auto-fetch)** — every five minutes, fresh data from each active connection is folded into the memory tree.
- **[Smart Token Compression](https://tinyhumans.gitbook.io/openhuman/features/token-compression)** — TokenJuice rule overlay compacts verbose tool output before it enters LLM context.
- **[Automatic Model Routing](https://tinyhumans.gitbook.io/openhuman/features/model-routing)** — `hint:reasoning`, `hint:fast`, `hint:vision` map to the right provider+model per task.
- **[Native Voice (ElevenLabs)](https://tinyhumans.gitbook.io/openhuman/features/voice)** — STT in, ElevenLabs TTS out, mascot lip-sync, live Google Meet agent.
- **[Native Tools](https://tinyhumans.gitbook.io/openhuman/features/native-tools)** — built-in web search, web-fetch scraper, and a full filesystem/git/lint/test/grep coder toolset.
- **[Local AI (optional)](https://tinyhumans.gitbook.io/openhuman/features/local-ai)** — opt-in Ollama path for memory embeddings, summary-tree building, and background reflection loops.
- **[Messaging Channels](https://tinyhumans.gitbook.io/openhuman/product/messaging-channels)** — inbound/outbound across the channels you already use, routed through your agent.
- **[Teams & Organizations](https://tinyhumans.gitbook.io/openhuman/product/teams)** — shared workspaces for collaborating with an agent across a team.
- **[Privacy & Security](https://tinyhumans.gitbook.io/openhuman/product/privacy-and-security)** — workflow data stays on device, encrypted locally, and treated as yours.

## OpenHuman vs other agents

High-level comparison (products evolve—verify against each vendor). OpenHuman is built to **minimize vendor sprawl**, keep **workflow knowledge on-device**, and give the agent a **persistent memory** of your data — not only chat.

| | Claude Code/Cowork | OpenClaw | Hermes Agent | OpenHuman |
| --------------------- | ------------------ | ----------------- | ----------------- | ---------------------------------- |
| **Open-source** | 🚫 Proprietary | ✅ MIT | ✅ MIT | ✅ GNU |
| **Simple to start** | ✅ Desktop + CLI | ⚠️ Terminal-first | ⚠️ Terminal-first | ✅ Clean UI, minutes |
| **Cost** | ⚠️ Sub + add-ons | ⚠️ BYO models | ⚠️ BYO models | ✅ One sub + TokenJuice |
| **Memory** | ✅ Chat-scoped | ⚠️ Plugin-reliant | ✅ Self-learning | 🚀 Memory Tree + Obsidian vault |
| **Integrations** | ⚠️ Few connectors | ⚠️ BYO | ⚠️ BYO | 🚀 118+ via OAuth |
| **Auto-fetch** | 🚫 None | 🚫 None | 🚫 None | ✅ 5-min sync into memory |
| **API sprawl** | 🚫 Extra keys | 🚫 BYOK | 🚫 Multi-vendor | ✅ One account |
| **Model routing** | 🚫 Single model | ⚠️ Manual | ⚠️ Manual | ✅ Hint-based (`hint:reasoning`) |
| **Native tools** | ✅ Code-only | ✅ Code-only | ✅ Code-only | ✅ Code + search + scraper + voice |

# Star us on GitHub

_Building toward AGI and artificial consciousness? Star the repo and help others find the path._

<p align="center">
 <a href="https://www.star-history.com/#tinyhumansai/openhuman&type=date&legend=top-left">
 <picture>
 <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=tinyhumansai/openhuman&type=date&theme=dark&legend=top-left" />
 <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=tinyhumansai/openhuman&type=date&legend=top-left" />
 <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=tinyhumansai/openhuman&type=date&legend=top-left" />
 </picture>
 </a>
</p>

# Contributors Hall of Fame

Show some love and end up in the hall of fame

<a href="https://github.com/tinyhumansai/openhuman/graphs/contributors">
 <img src="https://contrib.rocks/image?repo=tinyhumansai/openhuman" alt="OpenHuman contributors" />
</a>
