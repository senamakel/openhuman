---
description: >-
  118+ third-party integrations — Gmail, Notion, GitHub, Slack, Stripe, Calendar
  and more — with one-click OAuth and zero API keys.
icon: plug
---

# Third-party Integrations

OpenHuman ships with backend-proxied access to **118+ third-party services**. Connecting any of them is a one-click OAuth flow inside the app — there are no API keys to wire by hand, and no plugin marketplace to navigate.

(Under the hood, the connector layer is powered by [Composio](https://composio.dev). You will not need to think about it.)

Once a service is connected, it shows up in three places at once:

1. As an **agent tool** — the model can call it directly.
2. As a **memory source** — [auto-fetch](auto-fetch.md) syncs it into the [Memory Tree](memory-tree.md) every five minutes.
3. As a **profile signal** — your activity across services feeds your personalization.

## Some of what's in the catalog

The catalog spans productivity, business, social, messaging and Google. A non-exhaustive sample:

| Category | Examples |
| --- | --- |
| **Email & calendar** | Gmail, Outlook, Google Calendar, Apple Calendar |
| **Docs & storage** | Google Docs, Google Drive, Notion, Dropbox, Airtable |
| **Code & dev** | GitHub, Linear, Jira, Figma |
| **Comms** | Slack, Discord, Microsoft Teams, Telegram, WhatsApp |
| **CRM & sales** | Salesforce, HubSpot |
| **Commerce & payments** | Stripe, Shopify |
| **Project management** | Asana, Trello |
| **Social** | Twitter / X, Spotify, YouTube |

## Native vs proxied

Some services have **native providers** — Rust modules that know how to ingest the service into the Memory Tree directly (e.g. Gmail's native ingest path). Others are exposed as **proxied tools** only: the agent can call them, but there's no automatic ingest yet. New native providers are added as features land.

## Privacy boundary

OpenHuman's core never calls any third-party API directly. All requests go through the OpenHuman backend, which handles OAuth tokens and rate limiting. Your tokens never sit on disk in plaintext on your machine, and the agent only sees the *results* of tool calls — not the credentials.

## See also

- [Auto-fetch from Integrations](auto-fetch.md)
- [Memory Tree](memory-tree.md)
- [Skills & Integrations](../product/skills-and-integrations.md) — the product-level view.
