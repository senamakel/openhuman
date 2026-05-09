---
icon: '1'
---

# Knowledge Workers

You work across 8+ applications every day. Email, Slack, spreadsheets, documents, design tools, project boards, your code editor. Each app switch costs you context. By the end of the day, the thread connecting your morning research to your afternoon decisions has frayed. You spend as much time reconstructing context as you do acting on it.

OpenHuman turns that fragmented workflow into cumulative awareness.

#### The problem

Context evaporates with every app switch. You review a spreadsheet at 10am, discuss it in Slack at 11am, and by 2pm when you need to write a summary email, you have to reopen the spreadsheet, re-read the Slack thread, and piece it all back together.

Your brain does this all day. It is exhausting, and things fall through the cracks.

#### How OpenHuman helps

**Auto-fetch builds a continuous record.** Every five minutes, OpenHuman pulls fresh data from every active [integration](../features/integrations.md) — Gmail, Slack, GitHub, Notion, Drive, Linear, Jira, Calendar — and folds the results into the [Memory Tree](../features/memory-tree.md). You don't log anything manually; the picture builds itself in the background.

**Topic trees bridge your tools.** When you ask "what happened with the Q3 projections today?", you get a unified answer spanning every connected source — because the topic tree for "Q3 projections" was built from all of them.

**Your wiki is yours.** Everything also lands as `.md` in `<workspace>/wiki/`. Open it in [Obsidian](../features/obsidian-wiki.md) at the end of the day to skim, or drop in your own meeting notes — they get ingested into the same trees.

**TokenJuice keeps it cheap.** Sweeping through dozens of long email threads or a busy Slack channel for a daily roll-up costs cents, not dollars, because [TokenJuice](../features/token-compression.md) compacts the noise before the model sees it.

#### Example prompts

* "What did I miss while I was heads-down this morning?"
* "Summarize everything related to Project Atlas across email, Slack and Jira."
* "What did the team discuss about the launch timeline while I was heads-down in the doc?"
* "Connect what Sarah said in Slack with the spreadsheet I was reviewing."
* "What did I commit to today, with sources?"

#### Features that matter most here

| Feature | Why it matters |
| ------------------------------------------------------ | ------------------------------------------------------------- |
| [Auto-fetch](../features/auto-fetch.md) | Continuous ingest from every connected tool, no manual logging |
| [Memory Tree](../features/memory-tree.md) | Per-topic + per-day summaries that span every source |
| [third-party integrations](../features/integrations.md) | One-click OAuth into 118+ services |
| [Obsidian Wiki](../features/obsidian-wiki.md) | Audit and edit your memory by hand, in plain Markdown |

#### A typical workflow

**Morning:** You open your laptop. Auto-fetch quietly catches up on Gmail, Slack and GitHub from overnight. The first thing you ask is "what's waiting on me?" and get a 30-second briefing.

**Midday:** You jump from Slack to Notion to a code review. You don't have to remember each detail — the topic tree for the project absorbs it all.

**Afternoon:** You're drafting a status update. You ask "what did the team commit to this week, with attribution?" and get a clean list pulled from #engineering, #product and Linear.

**End of day:** You open the Obsidian vault and skim today's global digest. If something is wrong, you fix the Markdown by hand and the agent sees the correction next ingest.

#### When this use case is strongest

Knowledge Workers get the most value when they work across many applications daily and frequently need to reference earlier work. If your workflow is single-app, OpenHuman's cross-source advantage matters less.
