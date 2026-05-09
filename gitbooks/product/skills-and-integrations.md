---
icon: screwdriver
---

# Skills & Integrations

OpenHuman extends its capabilities through two surfaces: **third-party integrations** (third-party services) and **Skills** (custom logic that runs inside the app). Both are accessible from the Skills tab in the bottom navigation.

<figure><img src="../.gitbook/assets/9. Skills &#x26; Integrations@2x.png" alt=""><figcaption></figcaption></figure>

#### Third-party Integrations (118+)

OpenHuman ships with backend-proxied access to **over a hundred third-party services**. Each connects via a one-click OAuth flow and shows up as both an agent tool and a memory source.

A non-exhaustive sample of what's in the catalog:

| Category                | Examples                                                   |
| ----------------------- | ---------------------------------------------------------- |
| **Email & calendar**    | Gmail, Outlook, Google Calendar, Apple Calendar            |
| **Docs & storage**      | Google Docs, Drive, Notion, Dropbox, Airtable              |
| **Code & dev**          | GitHub, Linear, Jira, Figma                                |
| **Comms**               | Slack, Discord, Microsoft Teams, Telegram, WhatsApp        |
| **CRM & sales**         | Salesforce, HubSpot                                        |
| **Commerce & payments** | Stripe, Shopify                                            |
| **Project management**  | Asana, Trello                                              |
| **Social**              | Twitter / X, Spotify, YouTube, Reddit, Facebook, Instagram |

Some toolkits — Gmail today, more in flight — also have **native providers** that ingest into the [Memory Tree](../features/obsidian-wiki/memory-tree.md) directly. See [Third-party Integrations](../features/integrations.md) for the full picture.

#### How connections work

Click **Connect** on any integration. A browser window opens for OAuth. Once you sign in, the connection becomes active and OpenHuman starts syncing it through [auto-fetch](../features/auto-fetch.md) on the next 5-minute tick.

Each integration shows its current status:

* **Not connected** — integration has not been set up.
* **Connected** — integration is active and being synced.
* **Manage** — active integration with options to reconfigure or disconnect.

You can revoke any connection at any time.

#### Native voice and tools

Two capabilities ship native rather than as integrations because they're load-bearing for the desktop experience:

* [**Voice**](../features/voice.md) — STT in, ElevenLabs TTS out, plus a live Google Meet agent that joins meetings, transcribes them into your Memory Tree, and can speak back into the call.
* [**Native tools**](../features/native-tools.md) — built-in web search, web-fetch scraper, and a full filesystem/git/lint/test/grep coder toolset that the agent uses out of the box.

#### Skills

Skills are custom logic that runs inside OpenHuman — small, sandboxed modules that can fetch external data, run on a schedule, transform information, and respond to events. Each runs with enforced resource limits.

Skills install from the Skills tab and integrate with the same Memory Tree as everything else.

#### Privacy

Integration tokens are held by the OpenHuman backend, not stored in plaintext on your machine. The core never calls integration APIs directly — every integration request is proxied. Ingest results land in your local Memory Tree and Obsidian vault; the **chunks live on your machine**.

See [Privacy & Security](privacy-and-security.md) for the full boundary.
