---
icon: play
---

# Getting Started

This page walks you through setting up OpenHuman and running your first request. 

OpenHuman is open source under the GNU GPL3 license. The codebase is at [github.com/tinyhumansai/openhuman](https://github.com/tinyhumansai/openhuman).

***

#### System requirements

OpenHuman runs on **macOS, Windows and Linux** desktops. 8 GB+ RAM is recommended; 16 GB+ if you intend to ingest very large mailboxes or repos.

#### Permissions

The first time you launch OpenHuman, the OS will prompt for the permissions the app needs (Accessibility on macOS, Input Monitoring for voice hotkey, etc.). You can review and adjust these any time under **Settings → Automation & Channels**.

***

## Download & install

Get the OpenHuman desktop app from [openhuman.ai](https://openhuman.ai) or via your platform's package manager.

***

## Create your account

When you first open OpenHuman, you'll be asked to sign in. Multiple sign-in options are available, including social login.

{% hint style="info" %}
**No permanent lock-in.** Creating an account does not grant OpenHuman ongoing access to anything. All third-party access requires explicit OAuth approval per integration later.
{% endhint %}

***

## Connect your first source

OpenHuman works by connecting to your existing tools through [third-party integrations](../features/integrations/README.md). Each connection expands your [Memory Tree](../features/obsidian-wiki/memory-tree.md). You choose what to connect, and you can revoke access at any time.

The 118+ catalog spans Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira, Outlook, Dropbox, Airtable, Salesforce, HubSpot, Figma, Asana, Trello, Telegram, WhatsApp, Discord, Microsoft Teams, Twitter / X, Reddit, Spotify, YouTube, Facebook, Instagram and more.

Recommended starting points:

* **Gmail**. high-signal, has a native ingest path into the Memory Tree.
* **Slack**. picks up workplace chat context fast.
* **Notion**. for structured docs and exports.
* **GitHub**. if you write code.

Click **Connect** on any integration, complete the OAuth flow, and the next [auto-fetch](../features/integrations/auto-fetch.md) tick will start syncing it within twenty minutes.

***

## Run your first request

Once a source is connected and auto-fetch has run a tick, try prompts like:

**Briefings:**

* "What do I need to know from the last 12 hours?"
* "What's waiting on me?"

**Messaging queries:**

* "Summarize what I missed today across my channels."
* "What are the key decisions from this week?"
* "Extract action items from my recent conversations."

**Cross-source queries:**

* "Connect what my team discussed in Slack with what I was reviewing in Notion."
* "What did Sarah say about the project across email and chat?"

OpenHuman picks the right model for each task automatically, see [Automatic Model Routing](../features/model-routing/README.md).

***

## Open the Obsidian vault

The Memory tab has a **View vault in Obsidian** button. Click it to open `<workspace>/wiki/` in [Obsidian](https://obsidian.md). You can browse summaries, drop in your own notes, and even build manual links, the agent will see your edits next ingest. See [Obsidian Wiki](../features/obsidian-wiki/).

***

## Explore Skills & Integrations

After your first request, explore what else OpenHuman can do:

* **Skills** extend the assistant's capabilities, fetching data, running scheduled tasks, processing information.
* **Integrations** let you push structured results to Notion, Google Sheets, and other connected tools.

Learn more in [Skills & Integrations](../features/integrations/README.md).

***

#### Join the community

OpenHuman is in early beta. Feedback and contributions make a real difference at this stage.

* **GitHub:** [github.com/tinyhumansai/openhuman](https://github.com/tinyhumansai/openhuman)
* **Discord:** [discord.tinyhumans.ai](https://discord.tinyhumans.ai)
