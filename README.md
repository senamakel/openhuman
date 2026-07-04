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
 <strong>OpenHuman is your Personal AI super intelligence: local memory, managed services where needed, simple and powerful.</strong>
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

> **Local + managed services, upfront:** OpenHuman stores its Memory Tree, Obsidian-style Markdown vault, workspace config, and local runtime state on your machine. The default managed experience still uses OpenHuman-hosted services for account sign-in, model routing, web search proxying, and managed integration/OAuth flows through the Composio connector layer. Choose custom/local settings if you want to bring your own model, search, or Composio credentials; some real-time triggers and hosted features still require the managed backend.

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

OpenHuman is an open-source agentic assistant designed to integrate with you in your daily life — a personal AI with **persistent local memory**, **real hands** (100+ integrations, saved workflows, a browser, a wallet, a voice), and **a face** (a desktop mascot that joins your meetings and talks back). Each bullet links to the deeper writeup in the [docs](https://tinyhumans.gitbook.io/openhuman/).

- **Simple, UI-first & Human** A clean desktop experience and short onboarding paths take you from install to a working agent in a few clicks — no config-first setup, no terminal required. The agent has [a face](https://tinyhumans.gitbook.io/openhuman/features/mascot): a desktop mascot that speaks, reacts to its surroundings, [joins your meetings](https://tinyhumans.gitbook.io/openhuman/features/mascot/meeting-agents) as a real participant, remembers you across weeks, and keeps thinking in the background even when you've stopped typing.

- **100+ one-click OAuth integrations, 5,000+ MCP servers, 90,000+ Skills**: plug into Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira and the rest of your stack with [**one-click OAuth**](https://tinyhumans.gitbook.io/openhuman/features/integrations) — 100+ curated connectors brokered through the Composio layer. Beyond that, OpenHuman browses the open **Model Context Protocol** ecosystem (Smithery + the official MCP registry — thousands of servers) and a **90,000-entry Skills catalog**, so the agent can install new typed tools and skills on demand. Every connection becomes a typed tool, and every twenty minutes [auto-fetch](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki/auto-fetch) walks each active connection and pulls fresh data into the [memory tree](https://tinyhumans.gitbook.io/openhuman/features/memory-tree). No prompts, no polling loops you have to write, so the agent already has tomorrow's context this morning.

  Managed integrations use OpenHuman's Composio connector layer. OAuth handshakes and integration tool calls are proxied through the managed backend by default. If you want to run Composio directly instead, configure direct mode with your own Composio API key; real-time trigger webhooks then need to be hosted and wired by you.

- **[Memory Tree](https://tinyhumans.gitbook.io/openhuman/features/memory-tree) + [Obsidian Wiki](https://tinyhumans.gitbook.io/openhuman/features/obsidian-wiki)**: a local-first knowledge base built from your data and your activity. Everything you connect is canonicalized into ≤3k-token Markdown chunks, scored, and folded into hierarchical summary trees stored in **SQLite on your machine**. The same chunks land as `.md` files in an Obsidian-compatible vault you can open, browse and edit, inspired by Karpathy's [obsidian-wiki workflow](https://x.com/karpathy/status/2039805659525644595).

- **[SuperContext](https://tinyhumans.gitbook.io/openhuman/features/super-context)**: a fresh chat shouldn't start cold. With SuperContext enabled, the harness deterministically spawns a read-only `context_scout` on the **first turn of every new thread** — it sweeps your memory tree, files, and connected data, assembles a bounded context bundle, and prepends it to your message before the model ever reads it. No tool call to wait on, no "let me look that up" round-trip: the agent answers your first message already knowing the relevant background. Toggle it from the composer or `context.super_context_enabled`.

- **[Goals & Todos](https://tinyhumans.gitbook.io/openhuman/features/goals-and-todos)**: OpenHuman keeps the agent pointed at what matters. A short, human-editable list of **long-term goals** (`MEMORY_GOALS.md`) rides along in memory and can self-reflect against your recent activity; each **thread** can carry a single durable **goal** with an optional token budget that the agent works across turns, interrupts and idle periods (autonomous idle continuation); and every conversation hosts a **kanban task board** of todos that you and the agent build together — plans, acceptance criteria, approval gates and all. There's also a personal task list you own outright.

- **[Workflows](https://tinyhumans.gitbook.io/openhuman/features/workflows)** _(new)_: durable, visual automations powered by our open-source [tinyflows](https://github.com/tinyhumansai/tinyflows) engine. Describe what you want in chat and **the agent proposes a workflow** — a typed graph of triggers, agents, tool calls, HTTP requests, conditions and code steps — that you review on a visual canvas and save with one click. Workflows fire on schedules, inbound app events (a new Gmail thread, a Linear ticket), or manually; runs are durable and **pause at approval gates** until you tap Approve, then resume exactly where they stopped. Every run keeps full step-by-step history in a Run Inspector, and each flow carries its own trust level so an inbound webhook can feed *arguments* but can never smuggle in a new *action*.

- **[Meeting agents that actually attend](https://tinyhumans.gitbook.io/openhuman/features/mascot/meeting-agents)** _(expanded)_: the mascot joins **Google Meet, Zoom, Teams, and Webex** calls as a real participant — animated face on the camera tile, its own voice in the call, live transcript streaming into the app *while the meeting happens*. Connect your calendar (read-only scopes) and it **auto-joins upcoming meetings**, wakes when addressed by name ("Hey Tiny…"), answers with your memory and tools mid-call, and files a summary + action items + full transcript into a searchable meeting history. Owner-gated wake words and per-platform auto-join policies keep you in control.

- **[A harness that finishes the job](https://tinyhumans.gitbook.io/openhuman/developing/architecture/agent-harness)** _(new)_: every agent turn runs on our open-source [tinyagents](https://github.com/tinyhumansai/tinyagents) graph engine with **durable checkpointing** — sub-agents that need your input pause, checkpoint to disk, and resume when you answer instead of dying. A no-progress circuit breaker detects identical-call loops, steers the model back on course, and if it must halt, hands back a root-cause summary instead of silence. Tool failures are **classified** (needs sign-in vs. blocked by policy vs. safe to retry) and rendered in the chat timeline as actions you can actually take. Every run writes a durable event journal you can replay after the fact — and optionally streams **full-content traces to Langfuse** (every prompt, tool call, sub-agent, token count, and real dollar cost, one trace per turn).

- **[Privacy Mode](https://tinyhumans.gitbook.io/openhuman/features/privacy-and-security)** _(new)_: flip one switch and **no inference leaves your machine** — local-only mode hard-blocks every cloud model call at the provider factory and permits only local runtimes (Ollama, LM Studio, MLX, any local OpenAI-compatible endpoint). Not a prompt-level promise; an enforcement chokepoint in the Rust core.

- **[Image & video generation](https://tinyhumans.gitbook.io/openhuman/features/native-tools)** _(new)_: ask for it and dedicated media agents generate or edit images (Seedream / SeedEdit) and render text-to-video or animate a reference image (Seedance / Veo) through hosted models — artifacts download straight into your workspace, billed on the same single subscription.

- **[Themes & Theme Studio](https://tinyhumans.gitbook.io/openhuman/features/theming)**: make it yours. Five built-in theme families (Classic, Ocean, Sepia, Matrix, HAL 9000) ship in light/dark/auto variants, and the **Theme Studio** in Settings is a full visual editor — adjust every colour token with live contrast warnings, swap fonts per role, pick an animated WebGL-mesh / flat / custom-image backdrop, and export or import themes as JSON to share. Editing a preset auto-forks a custom theme so the originals stay pristine; everything applies instantly and persists locally.

- **Batteries included**: web search, a web-fetch [scraper](https://tinyhumans.gitbook.io/openhuman/features/native-tools), a full coder toolset (filesystem, git, lint, test, grep), a real [browser](https://tinyhumans.gitbook.io/openhuman/features/native-tools/browser-and-computer) with pluggable backends (Playwright daemon, native Rust, computer-use — with domain allowlists), and [native voice](gitbooks/features/native-tools/voice.md) (in-process Whisper STT — no external binary needed on macOS — ElevenLabs TTS out, mascot lip-sync, live meeting agent) are wired in by default. By default, [model routing](https://tinyhumans.gitbook.io/openhuman/features/model-routing) uses the OpenHuman backend to select and proxy the right LLM for each workload (reasoning, fast, or vision). One subscription includes all models. No "install a plugin to read files" friction. Use [optional local AI via Ollama](https://tinyhumans.gitbook.io/openhuman/features/model-routing/local-ai) for supported on-device workloads.

- **[Smart token compression (TokenJuice)](https://tinyhumans.gitbook.io/openhuman/features/token-compression)**: every tool call, scrape result, email body, and search payload is run through a token compression layer before it touches any LLM Model. HTML is converted to Markdown, long URLs are shortened, and verbose tool output is deduped and summarized via a configurable rule overlay etc... CJK, emoji, and other multi-byte text are preserved grapheme-by-grapheme — never stripped. You get the same information but at a fraction of the tokens. Reducing cost &amp; latency by up to 80%.

- **[17 messaging channels](https://tinyhumans.gitbook.io/openhuman/features/channels)** — talk to your agent where you already are: Telegram, Discord, Slack, WhatsApp, Signal, iMessage, Matrix, IRC, Mattermost, Lark, DingTalk, QQ, SMS… and now **native email**: a self-hosted IMAP IDLE + SMTP connector that pushes new mail to the agent in seconds and replies from your own address — any provider, your credentials, with a sender allowlist as the inbound security gate. Proactive delivery (cron, triggers, the subconscious loop) reaches you on the channel you pick.

- **[A subconscious that keeps working](https://tinyhumans.gitbook.io/openhuman/features/subconscious)**: between your messages, a background loop diffs your connected world, advances your goals and to-dos, and can steer a **split-brain orchestration layer** — a fast reflex agent that triages inbound traffic in seconds, backed by a deep reasoning core that does the multi-step work, with 20:1 history compression so week-long sessions stay sharp. It even writes you a [personalized morning briefing](https://tinyhumans.gitbook.io/openhuman/features/subconscious): greeting by name, matched to your local time, sorted into Highlights / Action items / Mentions / FYI.

- **[An economy for your agent](https://tinyhumans.gitbook.io/openhuman/features/wallet)**: OpenHuman agents are first-class citizens of [tiny.place](https://tiny.place), the agent-to-agent social network — register a `@handle`, message other agents over **Signal-protocol end-to-end encryption**, post and win **x402 USDC bounties**, trade in the marketplace, all signed by a wallet key derived on-device and never written to disk.

- **[Privacy & security](https://tinyhumans.gitbook.io/openhuman/features/privacy-and-security)**: workflow data stays on device, encrypted locally, treated as yours — with an approval gate on outbound actions, OS-keyring secret storage, command classification (read/write/network/destructive), and opt-in OS-level sandboxing (Landlock / Seatbelt / AppContainer) for agent actions.

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

## OpenHuman vs Other Agent Harnesses

High-level comparison (products evolve, so verify against each vendor). OpenHuman is built to **minimize vendor sprawl**, keep **workflow knowledge on-device**, and give the agent a **persistent memory** of your data, not only chat.

|                      | Claude Cowork     | OpenClaw          | Hermes Agent      | OpenHuman                          |
| -------------------- | ----------------- | ----------------- | ----------------- | ---------------------------------- |
| **Open-source**      | 🚫 Proprietary    | ✅ MIT            | ✅ MIT            | ✅ GNU                             |
| **Simple to start**  | ✅ Desktop + CLI  | ⚠️ Terminal-first | ⚠️ Terminal-first | ✅ Clean UI, minutes               |
| **Cost**             | ⚠️ Sub + add-ons  | ⚠️ BYO models     | ⚠️ BYO models     | ✅ One sub + TokenJuice            |
| **Memory**           | ✅ Chat-scoped    | ⚠️ Plugin-reliant | ✅ Self-learning  | 🚀 Memory Tree + Obsidian vault, optional [agentmemory](https://github.com/rohitg00/agentmemory) backend |
| **Integrations**     | ⚠️ Few connectors | ⚠️ BYO            | ⚠️ BYO            | 🚀 100+ OAuth · 5k+ MCP · 90k+ Skills |
| **Auto-fetch**       | 🚫 None           | 🚫 None           | 🚫 None           | ✅ 20-min sync into memory         |
| **Workflows**        | 🚫 None           | ⚠️ Scripts        | ⚠️ Scripts        | 🚀 Visual, durable, agent-proposed, approval-gated |
| **Meetings**         | 🚫 None           | 🚫 None           | 🚫 None           | 🚀 Joins Meet/Zoom/Teams/Webex, speaks, live transcript |
| **Messaging channels** | 🚫 None         | ⚠️ A few          | ⚠️ A few          | ✅ 17 incl. native email (IMAP/SMTP) |
| **Local-only mode**  | 🚫 Cloud-only     | ⚠️ BYO local      | ⚠️ BYO local      | ✅ One-switch enforced Privacy Mode |
| **Observability**    | 🚫 Opaque         | ⚠️ Logs           | ⚠️ Logs           | ✅ Full-content Langfuse traces + replayable run journals |
| **API sprawl**       | 🚫 Extra keys     | 🚫 BYOK           | 🚫 Multi-vendor   | ✅ One account                     |
| **Model routing**    | 🚫 Single model   | ⚠️ Manual         | ⚠️ Manual         | ✅ Built-in                        |
| **Native tools**     | ✅ Code-only      | ✅ Code-only      | ✅ Code-only      | ✅ Code + search + scraper + browser + voice + media gen |

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
