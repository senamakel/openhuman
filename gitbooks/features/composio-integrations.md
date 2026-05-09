---
description: >-
  118+ third-party integrations — Gmail, Notion, GitHub, Slack, Stripe, Calendar
  and more — with one-click OAuth and zero API keys.
icon: plug
---

# Composio Integrations

OpenHuman ships with backend-proxied access to **118+ third-party services** via Composio. Connecting any of them is a one-click OAuth flow inside the app — there are no API keys to wire by hand, and no plugin marketplace to navigate.

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

The full list lives in `src/openhuman/composio/providers/catalogs*.rs` and is wired into the JSON-RPC controller registry.

## Native vs proxied

Some toolkits have **native providers** — Rust modules under `src/openhuman/composio/providers/<toolkit>/` that know how to ingest the toolkit into the memory tree (e.g. Gmail's `ingest_page_into_memory_tree`). Others are exposed as **proxied tools** only: the agent can call them, but there's no auto-ingest yet. New native providers are added as features land.

## Privacy boundary

OpenHuman's core never calls the Composio API directly. All requests go through the OpenHuman backend, which handles OAuth tokens and rate limiting. Your tokens never sit on disk in plaintext on your machine, and the agent only sees the *results* of tool calls — not the credentials.

## RPC surface

For developers, every toolkit is exposed under the `openhuman.composio_*` JSON-RPC namespace — list connections, trigger a sync, fetch a profile, etc. See `src/openhuman/composio/rpc.rs`.

## See also

- [Auto-fetch from Integrations](auto-fetch.md)
- [Memory Tree](memory-tree.md)
- [Skills & Integrations](../product/skills-and-integrations.md) — the product-level view.
