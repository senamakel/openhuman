<h1 align="center">OpenHuman</h1>

<p align="center">
 <img src="./docs/mascot.gif" alt="The Tet" />
</p>

<p align="center">
 <a href="https://trendshift.io/repositories/23680" target="_blank"><img src="https://trendshift.io/api/badge/repositories/23680" alt="tinyhumansai%2Fopenhuman | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
</p>

<p align="center">
 <strong>OpenHuman is your Personal AI super intelligence. Private, Simple and extremely powerful.</strong>
</p>

<p align="center">
 <a href="https://discord.tinyhumans.ai/">Discord</a> •
 <a href="https://www.reddit.com/r/tinyhumansai/">Reddit</a> •
 <a href="https://x.com/intent/follow?screen_name=tinyhumansai">X/Twitter</a> •
 <a href="https://tinyhumans.gitbook.io/openhuman/">Docs</a> •
 <a href="https://x.com/intent/follow?screen_name=senamakel">Follow @senamakel (Creator)</a>
</p>

<p align="center">
 <img src="https://img.shields.io/badge/status-early%20beta-orange" alt="Early Beta" />
 <a href="https://github.com/tinyhumansai/openhuman/releases/latest"><img src="https://img.shields.io/github/v/release/tinyhumansai/openhuman?label=latest" alt="Latest Release" /></a>
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

OpenHuman is an open-source agentic assistant designed to integrate with you in your daily life. Each bullet links to the deeper writeup in the [docs](https://tinyhumans.gitbook.io/openhuman/).

- **Simple, UI-first** — A clean desktop experience and short onboarding paths so you can go from install to a working agent in a few clicks, without a config-first setup. No terminal required.

- **One subscription, many providers** — One account gets you access to many agentic APIs (AI models, search, webhooks/tunnels, third-party APIs).

- **[118+ third-party integrations](https://tinyhumans.gitbook.io/openhuman/features/integrations)** — Plug into Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira and the rest of your stack with **one-click OAuth**. Every connection is exposed to the agent as a typed tool.

- **[Memory Tree](https://tinyhumans.gitbook.io/openhuman/features/memory-tree) + [Obsidian Wiki](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki)** — A local-first knowledge base built from your data and your activity. Everything you connect is canonicalized into ≤3k-token Markdown chunks, scored, and folded into hierarchical summary trees stored in **SQLite on your machine**. The same chunks land as `.md` files in an Obsidian-compatible vault you can open, browse and edit — inspired by Karpathy's [obsidian-wiki workflow](https://x.com/karpathy/status/2039805659525644595).

- **[Auto-fetch from your stack](https://tinyhumans.gitbook.io/openhuman/features/auto-fetch)** — Every five minutes the core walks each active connection and pulls fresh data into the memory tree. No prompts, no polling loops you have to write — the agent already has tomorrow's context this morning.

- **[Smart token compression (TokenJuice)](https://tinyhumans.gitbook.io/openhuman/features/token-compression)** — Verbose tool output (git, npm, cargo, docker, large emails) is compacted by a rule overlay before it ever enters LLM context. Sweeping through thousands of emails stays cheap because the model never sees the noise.

- **[Automatic model routing](https://tinyhumans.gitbook.io/openhuman/features/model-routing)** — Tasks pick their model. `hint:reasoning` lands on a strong reasoning model, `hint:fast` on a fast one, vision goes to a vision model — all under one subscription, with zero per-provider key juggling.

- **[Native voice (ElevenLabs)](https://tinyhumans.gitbook.io/openhuman/features/voice)** — STT in, ElevenLabs TTS out, with mascot lip-sync. Includes a live Google Meet agent that listens, takes notes and can speak back in your meetings.

- **[Native search, scraper and coder](https://tinyhumans.gitbook.io/openhuman/features/native-tools)** — Built-in web search, web-fetch scraper, and a full filesystem / git / lint / test / grep toolset wired into the agent out of the box. No "install a plugin to read files" friction.

- **[Optional local AI (via Ollama)](https://tinyhumans.gitbook.io/openhuman/features/local-ai)** — Off by default. Opt in per-workload to keep memory embeddings, summary-tree building, and background reflection loops on your machine. Chat / vision / voice stay cloud — the local path is scoped to the workloads where on-device actually pays.

- **[Messaging channels](https://tinyhumans.gitbook.io/openhuman/product/messaging-channels), [teams & orgs](https://tinyhumans.gitbook.io/openhuman/product/teams), [privacy & security](https://tinyhumans.gitbook.io/openhuman/product/privacy-and-security)** — Inbound/outbound across the channels you already use, shared workspaces for collaborating with an agent across a team, and workflow data that stays on device, encrypted locally, treated as yours.

For contributors: [Architecture](https://tinyhumans.gitbook.io/openhuman/developing/architecture) · [Getting Set Up](https://tinyhumans.gitbook.io/openhuman/developing/getting-set-up) · [Cloud Deploy](https://tinyhumans.gitbook.io/openhuman/developing/cloud-deploy) · [`CONTRIBUTING.md`](./CONTRIBUTING.md).

## OpenHuman vs Other Agent Harnesses

High-level comparison (products evolve—verify against each vendor). OpenHuman is built to **minimize vendor sprawl**, keep **workflow knowledge on-device**, and give the agent a **persistent memory** of your data — not only chat.

| | Claude Cowork | OpenClaw | Hermes Agent | OpenHuman |
| --------------------- | ------------------ | ----------------- | ----------------- | ---------------------------------- |
| **Open-source** | 🚫 Proprietary | ✅ MIT | ✅ MIT | ✅ GNU |
| **Simple to start** | ✅ Desktop + CLI | ⚠️ Terminal-first | ⚠️ Terminal-first | ✅ Clean UI, minutes |
| **Cost** | ⚠️ Sub + add-ons | ⚠️ BYO models | ⚠️ BYO models | ✅ One sub + TokenJuice |
| **Memory** | ✅ Chat-scoped | ⚠️ Plugin-reliant | ✅ Self-learning | 🚀 Memory Tree + Obsidian vault |
| **Integrations** | ⚠️ Few connectors | ⚠️ BYO | ⚠️ BYO | 🚀 118+ via OAuth |
| **Auto-fetch** | 🚫 None | 🚫 None | 🚫 None | ✅ 5-min sync into memory |
| **API sprawl** | 🚫 Extra keys | 🚫 BYOK | 🚫 Multi-vendor | ✅ One account |
| **Model routing** | 🚫 Single model | ⚠️ Manual | ⚠️ Manual | ✅ Built-in |
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
