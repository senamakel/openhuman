---
icon: '6'
---

# Distributed Teams

Your team makes decisions in chat and struggles to execute. Commitments dissolve into scroll history. Contradictions between separate conversations go unnoticed until standup.

<figure><img src="../.gitbook/assets/25. Distributed Teams@2x.png" alt=""><figcaption></figcaption></figure>

#### The problem

Chat is fast but ephemeral. Action items live in threads nobody revisits. Accountability depends on individual memory, which fails at scale.

#### How OpenHuman helps

**Cross-channel ingest, on autopilot.** [Auto-fetch](../features/auto-fetch.md) pulls from Slack, Discord, GitHub, Linear, Jira, Notion and Gmail every five minutes — every channel you've connected through [third-party integrations](../features/integrations.md) — and folds the results into a single [Memory Tree](../features/obsidian-wiki/memory-tree.md).

**Decision and action extraction.** The agent can pull commitments out of chat with attribution and source links. Because every chunk is also a `.md` file in your [Obsidian vault](../features/obsidian-wiki/), you can audit any extracted commitment back to the original message.

**Contradiction detection.** Topic trees aggregate per entity (per project, per ticket) across channels, so the agent can flag conflicting commitments before they become surprises.

**Structured exports.** Decisions and action items can be written straight into Notion or Google Sheets via the same same integration surface — no manual transcription.

#### Example prompts

* "What did we commit to this week across all channels?"
* "Are there any contradictions between what different team members said about the Q4 launch?"
* "Extract all action items from #product since Monday and write them to the team Notion."
* "What decisions were made yesterday that need documenting?"

#### When this use case is strongest

Best when coordination happens through messaging across multiple channels with 5+ people. If your team uses a rigid project-management tool for _every_ commitment and chat is purely social, OpenHuman adds less value.
