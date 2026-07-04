<h1 align="center">OpenHuman</h1>

<p align="center">
 <img src="./gitbooks/.gitbook/assets/demo.png" alt="The Tet" />
</p>

<p align="center" style="display: inline-block">
	<a href="https://trendshift.io/repositories/23680" target="_blank" style="display: inline-block">
		<img src="https://trendshift.io/api/badge/repositories/23680" alt="tinyhumansai%2Fopenhuman | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/>
	</a>
	<a href="https://www.producthunt.com/products/openhuman?embed=true&amp;utm_source=badge-top-post-badge&amp;utm_medium=badge&amp;utm_campaign=badge-openhuman" target="_blank" rel="noopener noreferrer">
		<img alt="OpenHuman - An open source AI harness built with the human in mind | Product Hunt" width="250" height="54" src="https://api.producthunt.com/widgets/embed-image/v1/top-post-badge.svg?post_id=1136902&amp;theme=light&amp;period=daily&amp;t=1778916022823">
		</a>
		<a href="https://www.producthunt.com/products/openhuman?embed=true&amp;utm_source=badge-top-post-badge&amp;utm_medium=badge&amp;utm_campaign=badge-openhuman" target="_blank" rel="noopener noreferrer">
			<img alt="OpenHuman - An open source AI harness built with the human in mind | Product Hunt" width="250" height="54" src="https://api.producthunt.com/widgets/embed-image/v1/top-post-badge.svg?post_id=1136902&amp;theme=light&amp;period=weekly&amp;t=1779351403565">
		</a>
</p>
<p align="center" style="display: inline-block">
 <a href="https://www.producthunt.com/products/openhuman?embed=true&amp;utm_source=badge-top-post-topic-badge&amp;utm_medium=badge&amp;utm_campaign=badge-openhuman" target="_blank" rel="noopener noreferrer">
  <img alt="OpenHuman - An open source AI harness built with the human in mind | Product Hunt" width="250" height="54" src="https://api.producthunt.com/widgets/embed-image/v1/top-post-topic-badge.svg?post_id=1136902&amp;theme=light&amp;period=weekly&amp;topic_id=268&amp;t=1779351808756">
  </a>
  <a href="https://www.producthunt.com/products/openhuman?embed=true&amp;utm_source=badge-top-post-topic-badge&amp;utm_medium=badge&amp;utm_campaign=badge-openhuman" target="_blank" rel="noopener noreferrer">
   <img alt="OpenHuman - An open source AI harness built with the human in mind | Product Hunt" width="250" height="54" src="https://api.producthunt.com/widgets/embed-image/v1/top-post-topic-badge.svg?post_id=1136902&amp;theme=light&amp;period=weekly&amp;topic_id=46&amp;t=1779351808756">
   </a>
 </p>

<p align="center">
 <strong>OpenHuman is your personal AI super intelligence: a brain that remembers everything, a fantastic orchestrator, a deep researcher. Local-first, simple, powerful.</strong>
</p>

<p align="center">
 <a href="https://discord.tinyhumans.ai/">Discord</a> •
 <a href="https://www.reddit.com/r/tinyhumansai/">Reddit</a> •
 <a href="https://x.com/intent/follow?screen_name=tinyhumansai">X/Twitter</a> •
 <a href="https://tinyhumans.gitbook.io/openhuman/">Docs</a> •
 <a href="https://x.com/intent/follow?screen_name=senamakel">Follow @senamakel (Creator)</a>
</p>

<p align="center">
  🇺🇸 <a href="./README.md">English</a> | 🇨🇳 <a href="./docs/README.zh-CN.md">简体中文</a> | 🇯🇵 <a href="./docs/README.ja-JP.md">日本語</a> | 🇰🇷 <a href="./docs/README.ko.md">한국어</a> | 🇩🇪 <a href="./docs/README.de.md">Deutsch</a> | 🇵🇰 <a href="./docs/README.ur-pk.md">اردو</a>
</p>

<p align="center">
 <img src="https://img.shields.io/badge/status-early%20beta-orange" alt="Early Beta" />
 <a href="https://github.com/tinyhumansai/openhuman/releases/latest"><img src="https://img.shields.io/github/v/release/tinyhumansai/openhuman?label=latest" alt="Latest Release" /></a>
 <a href="https://github.com/tinyhumansai/openhuman/stargazers"><img src="https://img.shields.io/github/stars/tinyhumansai/openhuman?style=flat" alt="GitHub Stars" /></a>
 <a href="./LICENSE"><img src="https://img.shields.io/github/license/tinyhumansai/openhuman" alt="License" /></a>
 <a href="./docs/README.zh-CN.md"><img src="https://img.shields.io/badge/lang-简体中文-blue" alt="简体中文" /></a>
 <a href="./docs/README.ja-JP.md"><img src="https://img.shields.io/badge/lang-日本語-blue" alt="日本語" /></a>
 <a href="./docs/README.ko.md"><img src="https://img.shields.io/badge/lang-한국어-blue" alt="한국어" /></a>
 <a href="./docs/README.de.md"><img src="https://img.shields.io/badge/lang-Deutsch-blue" alt="Deutsch" /></a>
 <a href="./docs/README.ur-pk.md"><img src="https://img.shields.io/badge/lang-اردو-blue" alt="اردو" /></a>
</p>

> **Early Beta**: Under active development. Expect rough edges.

> 🎉 Within one week of launch, OpenHuman became the number one trending repository on GitHub for nine days in a row.

# Install

Download installers from [tinyhumans.ai/openhuman](https://tinyhumans.ai/openhuman?utm_source=github&utm_medium=readme) or from the [GitHub Releases](https://github.com/tinyhumansai/openhuman/releases/latest) page. For terminal installs, the native package paths below are preferred because they use your OS package manager or native installer where available.

## Recommended install (native packages)

These paths use native installer surfaces. Homebrew and MSI provide their normal signing/integrity checks; Debian/Ubuntu uses `apt-get` to install the release `.deb` and resolve system dependencies.

**macOS (Homebrew tap):**

```bash
brew tap tinyhumansai/core
brew install openhuman
```

**Linux (Debian/Ubuntu — release `.deb`):**

```bash
# Download OpenHuman_<version>_amd64.deb or OpenHuman_<version>_arm64.deb
# from https://github.com/tinyhumansai/openhuman/releases/latest, then:
# Replace amd64 with arm64 on arm64 hosts.
sudo apt-get install -y --no-install-recommends ./OpenHuman_*_amd64.deb
```

**Linux (Arch — AUR):** the [`openhuman-bin` AUR recipe](./packages/arch/openhuman-bin/) is in the repo. Once published, Arch users can install it with `yay -S openhuman-bin`.

**Windows:** download the signed `.msi` from the [latest release](https://github.com/tinyhumansai/openhuman/releases/latest) and run it.

**Manual `.dmg` / `.deb` / `.AppImage` / `.msi`:** grab the installer for your platform directly from the [latest release page](https://github.com/tinyhumansai/openhuman/releases/latest).

> **Linux:** the AppImage can crash on launch under Wayland, miss host system libraries such as `libgbm.so.1`, or fail on Arch-based distros with `sharun: Interpreter not found!` — see [#2463](https://github.com/tinyhumansai/openhuman/issues/2463) for the cause and env-var workarounds. The `.deb` package above avoids those failure modes on Debian/Ubuntu by letting apt resolve runtime dependencies.

## Alternative: script install (no integrity check)

> **Warning — unverified install.** These scripts are served live from `raw.githubusercontent.com` and do **not** ship a separate signature, so `curl … | bash` and `irm … | iex` have no way to detect tampering of the script bytes. Prefer the **native package** paths above whenever possible. If you must use the script, see "Verified script install" below.

```bash
# macOS or Linux x64
curl -fsSL https://raw.githubusercontent.com/tinyhumansai/openhuman/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/tinyhumansai/openhuman/main/scripts/install.ps1 | iex
```

On Debian/Ubuntu, `install.sh` resolves the latest release `.deb` first and installs it with `apt-get` so runtime dependencies are handled by apt. Set `OPENHUMAN_INSTALLER_LINUX_PACKAGE=appimage` to force the AppImage path.

## Verified script install status

A separately signed script-install path is not currently available. Issue [#2620](https://github.com/tinyhumansai/openhuman/issues/2620) is closed after the native package paths were promoted, but current release assets do not include `install.sh.asc` / `install.ps1.asc` for pre-execution script verification. Treat the script install path as unverified and prefer the native package options above when possible.

# What is OpenHuman?

OpenHuman is three things most assistants aren't: **a brain** that builds a persistent, local memory of your world; **a fantastic orchestrator** that runs fleets of agents on durable graphs; and **a deep researcher** that sweeps your data and the web before you finish asking. Every bullet links to the deeper writeup in the [docs](https://tinyhumans.gitbook.io/openhuman/).

### 🧠 The brain

- **[Memory Tree](https://tinyhumans.gitbook.io/openhuman/features/memory-tree) + [Obsidian Wiki](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki)**: your data compressed into scored Markdown trees in SQLite on your machine, mirrored as an [Obsidian vault](https://x.com/karpathy/status/2039805659525644595) you can open and edit. No vector-soup black box.
- **[100+ OAuth integrations, 5,000+ MCP servers, 90,000+ Skills](https://tinyhumans.gitbook.io/openhuman/features/integrations)**: one click into Gmail, Notion, GitHub, Slack and the rest of your stack. [Auto-fetch](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki/auto-fetch) feeds the brain every 20 minutes — it has tomorrow's context this morning.
- **[A subconscious](https://tinyhumans.gitbook.io/openhuman/features/subconscious)**: a background loop that diffs your world, advances your goals, and writes your morning briefing — thinking continues after you stop typing.
- **[Goals & Todos](https://tinyhumans.gitbook.io/openhuman/features/goals-and-todos)**: long-term goals, durable per-thread goals, and a shared kanban board per conversation.
- **[TokenJuice](https://tinyhumans.gitbook.io/openhuman/features/token-compression)**: tool output compressed before it hits the model — same information, up to 80% fewer tokens. A brain this big would be unaffordable without it.

### 🕸️ The orchestrator

- **[Workflows](https://tinyhumans.gitbook.io/openhuman/features/workflows)** _(new)_: the agent proposes the automation; you review it on a canvas and save. Durable, trigger-driven, approval-gated runs on open-source [tinyflows](https://github.com/tinyhumansai/tinyflows).
- **[A harness that finishes the job](https://tinyhumans.gitbook.io/openhuman/developing/architecture/agent-harness)** _(new)_: checkpointed graph runs on open-source [tinyagents](https://github.com/tinyhumansai/tinyagents) — stuck agents get steered, halted ones return a root cause, every run replays with real per-call costs.
- **[A split brain, always on](https://tinyhumans.gitbook.io/openhuman/features/orchestration)** _(new)_: a fast reflex agent triages inbound traffic while a deep reasoning core delegates to worker fleets, steered by the subconscious.
- **[An agent economy](https://tinyhumans.gitbook.io/openhuman/features/tinyplace)**: a `@handle` on [tiny.place](https://tiny.place), Signal-encrypted agent-to-agent orchestration, x402 USDC bounties and trading — keys never touch disk.

### 🔬 The deep researcher & doer

- **[SuperContext](https://tinyhumans.gitbook.io/openhuman/features/super-context)**: a research scout sweeps your memory and files before the model reads your first message. No cold starts.
- **Batteries included**: web search, scraper, coder toolset, a real [browser](https://tinyhumans.gitbook.io/openhuman/features/native-tools/browser-and-computer), [native voice](gitbooks/features/native-tools/voice.md) with in-process Whisper — and [model routing](https://tinyhumans.gitbook.io/openhuman/features/model-routing) that picks the right LLM per workload, one subscription, [local AI optional](https://tinyhumans.gitbook.io/openhuman/features/model-routing/local-ai).
- **[Meeting agents](https://tinyhumans.gitbook.io/openhuman/features/mascot/meeting-agents)** _(new)_: joins **Meet, Zoom, Teams, and Webex** with a face and a voice — auto-joins from your calendar, streams a live transcript, answers by name, files summary + action items.
- **[Image & video generation](https://tinyhumans.gitbook.io/openhuman/features/native-tools)** _(new)_: Seedream/SeedEdit images and Seedance/Veo video, straight into your workspace on the same subscription.
- **[17 messaging channels](https://tinyhumans.gitbook.io/openhuman/features/channels)**: Telegram, Discord, Slack, WhatsApp, Signal, iMessage… plus **native email** (IMAP IDLE + SMTP). Your agent reaches you where you already are.

### 🧍 Human, private, yours

- **Simple, UI-first & Human**: install to working agent in a few clicks — no config files, no terminal. And it has [a face](https://tinyhumans.gitbook.io/openhuman/features/mascot): a mascot that speaks, reacts, and remembers you.
- **[Privacy & security](https://tinyhumans.gitbook.io/openhuman/features/privacy-and-security)**: on-device encrypted data, approval gate, OS-keyring secrets, opt-in sandboxing — and _(new)_ **[Privacy Mode](https://tinyhumans.gitbook.io/openhuman/features/privacy-mode)**: one switch and no inference leaves your machine, enforced in the Rust core.
- **[Themes & Theme Studio](https://tinyhumans.gitbook.io/openhuman/features/theming)**: five theme families plus a full visual editor, exportable as JSON.

## Contributing from source

New contributor? Start with [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the fork/PR workflow and local validation commands, or use the copy-paste AI-agent prompt in [`CONTRIBUTING-BEGINNERS.md`](./CONTRIBUTING-BEGINNERS.md#optional-let-an-ai-coding-agent-guide-you). The short path is:

1. Install Git, Node.js 24+, pnpm 10.10.0, Rust 1.93.0 (`rustfmt` + `clippy`), CMake, Ninja, ripgrep, and the platform desktop build prerequisites.
2. Fork and clone the repo, then run `git submodule update --init --recursive` before `pnpm install` so the vendored Tauri/CEF sources are present.
3. Use `pnpm dev` for web-only UI work, `pnpm --filter openhuman-app dev:app` for the desktop shell, and focused checks such as `pnpm typecheck`, `pnpm format:check`, and `cargo check -p openhuman --lib` before opening a PR.

Deeper docs: [Architecture](https://tinyhumans.gitbook.io/openhuman/developing/architecture) · [Getting Set Up](https://tinyhumans.gitbook.io/openhuman/developing/getting-set-up) · [Cloud Deploy](./gitbooks/features/cloud-deploy.md).

## Context in minutes, not weeks

OpenHuman is the first agent harness that gets to know you in minutes. Inspired by [Karpathy's LLM Knowledgebase](https://x.com/karpathy/status/2039805659525644595). Most agents start cold. Hermes learns by watching you work; OpenClaw waits for plugins to ferry context in. Either way, you spend days or weeks before the agent knows enough about your stack to be genuinely useful.

<p align="center">
 <img src="./gitbooks/.gitbook/assets/image (1).png" alt="OpenHuman context-building diagram">
</p>

> OpenHuman summarizes and compresses all your documents, emails & chats; and creates a memory graph that lets your agent remember everything about you.

OpenHuman skips the wait. Connect your accounts, let [auto-fetch](https://tinyhumans.gitbook.io/openhuman/features/integrations/auto-fetch) pull data locally on a 20-minute loop, and then have [Memory Trees](https://tinyhumans.gitbook.io/openhuman/features/memory-tree) compress everything into Markdown files stored intelligently in a [Karpathy-style Obsidian wiki](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki).

In just one sync pass, the agent has full (compressed) context of your inbox, your calendar, your repos, your docs, your messages. No training period. No "give it a few weeks.". It becomes you, controlled by you.

Already self-host [agentmemory](https://github.com/rohitg00/agentmemory) across other coding agents? OpenHuman ships an optional `Memory` backend that proxies to it — set `memory.backend = "agentmemory"` in `config.toml` and the same durable store powers OpenHuman alongside Claude Code, Cursor, Codex, and OpenCode. See the [agentmemory backend](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki/agentmemory-backend) page for setup.

## An orchestrator, not a chatbot

The deepest difference isn't any single feature — it's the execution model. Claude Code, OpenClaw, and Hermes run **one agent in one loop**. OpenHuman is an **[orchestrator](https://tinyhumans.gitbook.io/openhuman/features/orchestration)**:

- **Graphs, not loops** — turns compile to checkpointed state-machine graphs on [tinyagents](https://github.com/tinyhumansai/tinyagents): pause for a human, survive a restart, resume mid-run.
- **Sub-agent fleets** — specialists spawn three levels deep, idle workers get reused, and circuit breakers turn stuck agents into root-cause reports.
- **Workflows you can see** — agent-proposed [tinyflows](https://github.com/tinyhumansai/tinyflows) graphs, reviewed on a canvas: durable, trigger-driven, approval-gated.
- **A split brain, always on** — a fast reflex agent triages while a deep core reasons, steered by the subconscious loop.
- **Agent-to-agent, encrypted** — instances orchestrate each other over **Signal-protocol E2E** sessions with consent-based pairing and x402 payments. No server sees plaintext.
- **Next: RLMs** — the model writing its own orchestration code in a sandboxed REPL, on the same graph engine and trust model.

## OpenHuman vs Other Agent Harnesses

High-level comparison (products evolve, so verify against each vendor). OpenHuman is built to **minimize vendor sprawl**, keep **workflow knowledge on-device**, and give the agent a **persistent memory** of your data, not only chat.

|                        | Claude Cowork     | OpenClaw          | Hermes Agent      | OpenHuman                                                                                                |
| ---------------------- | ----------------- | ----------------- | ----------------- | -------------------------------------------------------------------------------------------------------- |
| **Open-source**        | 🚫 Proprietary    | ✅ MIT            | ✅ MIT            | ✅ GNU                                                                                                   |
| **Simple to start**    | ✅ Desktop + CLI  | ⚠️ Terminal-first | ⚠️ Terminal-first | ✅ Clean UI, minutes                                                                                     |
| **Cost**               | ⚠️ Sub + add-ons  | ⚠️ BYO models     | ⚠️ BYO models     | ✅ One sub + TokenJuice                                                                                  |
| **Memory**             | ✅ Chat-scoped    | ⚠️ Plugin-reliant | ✅ Self-learning  | 🚀 Memory Tree + Obsidian vault, optional [agentmemory](https://github.com/rohitg00/agentmemory) backend |
| **Integrations**       | ⚠️ Few connectors | ⚠️ BYO            | ⚠️ BYO            | 🚀 100+ OAuth · 5k+ MCP · 90k+ Skills                                                                    |
| **Auto-fetch**         | 🚫 None           | 🚫 None           | 🚫 None           | ✅ 20-min sync into memory                                                                               |
| **Orchestration**      | ⚠️ Sub-tasks      | ⚠️ Single loop    | ⚠️ Single loop    | 🚀 Agent graphs + checkpoints + E2E-encrypted A2A                                                        |
| **Workflows**          | 🚫 None           | ⚠️ Scripts        | ⚠️ Scripts        | 🚀 Visual, durable, agent-proposed, approval-gated                                                       |
| **Meetings**           | 🚫 None           | 🚫 None           | 🚫 None           | 🚀 Joins Meet/Zoom/Teams/Webex, speaks, live transcript                                                  |
| **Messaging channels** | 🚫 None           | ⚠️ A few          | ⚠️ A few          | ✅ 17 incl. native email (IMAP/SMTP)                                                                     |
| **Local-only mode**    | 🚫 Cloud-only     | ⚠️ BYO local      | ⚠️ BYO local      | ✅ One-switch enforced Privacy Mode                                                                      |
| **Observability**      | 🚫 Opaque         | ⚠️ Logs           | ⚠️ Logs           | ✅ Replayable run journals + per-call cost accounting                                                    |
| **API sprawl**         | 🚫 Extra keys     | 🚫 BYOK           | 🚫 Multi-vendor   | ✅ One account                                                                                           |
| **Model routing**      | 🚫 Single model   | ⚠️ Manual         | ⚠️ Manual         | ✅ Built-in                                                                                              |
| **Native tools**       | ✅ Code-only      | ✅ Code-only      | ✅ Code-only      | ✅ Code + search + scraper + browser + voice + media gen                                                 |

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

Show some love and end up in the hall of fame. Contributors get free merch and special access to our [Discord](https://discord.tinyhumans.ai/).

<a href="https://github.com/tinyhumansai/openhuman/graphs/contributors">
 <img src="https://contrib.rocks/image?repo=tinyhumansai/openhuman" alt="OpenHuman contributors" />
</a>
