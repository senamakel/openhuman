---
icon: nfc-signal
---

# Automation & Channels

Navigate to **Settings → Automation & Channels**. Configure desktop automation, messaging channels, and the auto-fetch schedule for each connected integration.

#### Accessibility Automation

Desktop permissions and assisted controls.

**Permissions:** Accessibility (GRANTED/DENIED) and Input Monitoring (GRANTED/DENIED). Buttons: Request Accessibility, Open Input Monitoring, Refresh Status.

**Features:** Toggles for Device Control (interact with UI elements) and Predictive Input (provide input predictions).

#### Messaging Channels

Configure the default messaging channel and auth modes.

**Default Messaging Channel:** Choose between Telegram, Discord, or Web. Shows active route status.

**Channel Integrations:** Configure auth modes for Telegram and Discord. Click a channel name to open its configuration page.

#### Auto-fetch & Cron Jobs

OpenHuman runs a global [auto-fetch](../features/auto-fetch.md) tick every five minutes that walks every active connection and pulls fresh data into the [Memory Tree](../features/memory-tree.md). On top of that:

**Core Cron Jobs:** System-level jobs in the OpenHuman core scheduler database.

**Per-integration sync intervals:** Each [integration](../features/integrations.md) declares its own minimum interval between syncs. The defaults are sensible; you can override them here.

| Integration | Default sync interval |
| ----------- | --------------------- |
| Gmail | every 15 minutes |
| Notion | every 20 minutes |

More frequent syncing keeps data fresher but uses more inference budget. [TokenJuice](../features/token-compression.md) keeps the cost bounded even at high frequencies.

Click **Refresh Cron Jobs** to reload the schedule.
