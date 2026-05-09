---
icon: '2'
---

# Individual Power Users

You live in messaging. Dozens of active Slack channels, Telegram groups, DMs, threads, plus an inbox that never quite hits zero. Every morning starts with a wall of unread context.

#### The problem

High-volume comms create information overload that gets worse as you join more groups. Reading everything is impossible; skimming means you miss decisions, commitments, and replies you owe people.

#### How OpenHuman helps

**Auto-fetch keeps the picture fresh.** Every five minutes, OpenHuman pulls new messages from your active [third-party integrations](../features/integrations.md) — Gmail, Slack, GitHub, Notion — and folds them into the [Memory Tree](../features/obsidian-wiki/memory-tree.md). By the time you sit down with coffee, the briefing is already pre-computed.

**Topic trees catch the things waiting on you.** As entities (people, projects) get more activity, their topic tree gets refreshed. "Things waiting on a reply from me" becomes a real query, not a manual scrub.

**TokenJuice keeps it cheap.** Sweeping through hundreds of emails for a daily briefing costs cents, not dollars, because [TokenJuice](../features/token-compression.md) compacts the noise before the model ever sees it.

**Your wiki is yours.** All the same data lands in `<workspace>/wiki/` as Markdown. Open it in [Obsidian](../features/obsidian-wiki/) when you want to wander through it by hand.

#### Example prompts

* "What do I need to know from the last 12 hours?"
* "Are there any messages waiting for my response?"
* "Summarize the key decisions from #engineering this week."
* "Extract all action items assigned to me from the last 3 days."
* "What did Sarah say about the Q4 roadmap across email and Slack?"

#### Features that matter most here

| Feature                                                                 | Why it matters                                            |
| ----------------------------------------------------------------------- | --------------------------------------------------------- |
| [Auto-fetch](../features/auto-fetch.md)                                 | Your inbox/Slack/etc. lands in memory without you asking  |
| [Memory Tree topic summaries](../features/obsidian-wiki/memory-tree.md) | Per-entity recaps surface what's outstanding              |
| [TokenJuice](../features/token-compression.md)                          | Daily sweeps stay cheap even across thousands of messages |
| [third-party integrations](../features/integrations.md)                 | One-click OAuth into 118+ services                        |

#### When this use case is strongest

You're juggling 10+ active conversations across at least two or three platforms. If you have two quiet channels and a tidy inbox, the manual approach works fine.
