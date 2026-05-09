---
icon: question
---

# FAQ & Troubleshooting

## Frequently Asked Questions

#### What is OpenHuman?

OpenHuman is a personal AI assistant that runs natively on your desktop. It connects to 118+ third-party services with one-click OAuth, pulls data from them automatically, builds a local-first [Memory Tree](../features/memory-tree.md) you can browse as a Markdown vault in Obsidian, and gives you an agent with native voice, smart model routing, and built-in coder/search/scraper tools.

***

#### What is the Memory Tree?

The Memory Tree is OpenHuman's knowledge base. It's a deterministic pipeline that turns every source you connect — chats, emails, documents, integration sync results — into canonical Markdown, ≤3k-token chunks, scored and folded into per-source / per-topic / per-day summary trees. Stored in SQLite on your machine; the same chunks live as `.md` files in your Obsidian vault. See [Memory Tree](../features/memory-tree.md).

***

#### What does "Big Data AI" mean?

Every AI model today is a prompt engine. You type something, it responds, and the context disappears. OpenHuman is different. It compresses your entire organizational life — messages, documents, tools, transactions — into a structured, queryable Memory Tree that persists and evolves. This is what we mean by Big Data AI: an agent that operates on months of your real data, not just the prompt you typed right now.

#### **How is OpenHuman different from ChatGPT, Claude, or Gemini?**

Those models are brilliant at reasoning. But they are stateless. They know nothing about your actual life beyond what you paste into the chat window. OpenHuman is the context layer that makes those models useful. It compresses your organizational data into structured intelligence any AI can reason over. Think of it this way: ChatGPT is the brain. OpenHuman is the memory.

***

#### **How is OpenHuman different from other AI memory solutions like Mem0, SuperMemory, or MemGPT?**

Most "AI memory" stacks are vector databases with a thin wrapper. Vector similarity is one signal among many — it answers "what is similar?" but says nothing about importance, recency, or provenance. The Memory Tree is structurally different: deterministic chunking, per-scope summary trees (source / topic / global), entity-driven hotness, and every chunk auditable as a `.md` file in your [Obsidian vault](../features/obsidian-wiki.md). You can read the memory by hand. You can fix it by hand.

***

#### **Is OpenHuman open source?**

Yes. OpenHuman is open-sourced under GNU GPL3. The full codebase is available on [GitHub](https://github.com/tinyhumansai/openhuman). Contributions and feedback are welcomed.

***

#### **Is OpenHuman AGI?**

No. OpenHuman is not AGI, and we do not claim it is. It is a meaningful architectural step closer, with a memory layer and an agent loop that go beyond what stateless chat assistants offer. But it operates within defined boundaries and requires human judgment.

***

#### **What does auto-fetch do?**

Every five minutes, OpenHuman walks every active [integration](../features/integrations.md), checks per-connection sync state (last timestamp, dedup set, daily budget), and — if enough time has elapsed — pulls fresh data and folds it into the Memory Tree. By the time you ask "what landed in my inbox overnight?", the answer is already there. See [Auto-fetch](../features/auto-fetch.md).

***

#### Does OpenHuman read all my messages?

OpenHuman processes whatever you've connected — that's the whole point. But you choose what to connect, and you can revoke any connection at any time. There is no continuous scanning of anything you haven't explicitly authorized through OAuth.

***

#### Is my data safe?

The Memory Tree's SQLite database and the Obsidian vault stay on your machine. Integration OAuth tokens are held by the OpenHuman backend, never in plaintext on your laptop. On desktop, OS-level credentials live in your platform keychain. All communication between the app and OpenHuman's servers is encrypted in transit.

***

#### Does OpenHuman store my messages?

Source data is canonicalized into Markdown chunks and stored in your **local** memory tree. The OpenHuman backend processes the *requests* you make through it (LLM calls, web search, integration proxying) but does not retain a copy of your raw source data.

***

#### Can OpenHuman send messages on my behalf?

Yes — through your connected third-party integrations. Telegram, Slack, Gmail and the rest expose action-style tools the agent can call when you ask. All actions go through the connections you've authorized; you control which capabilities are active.

***

#### Who is OpenHuman for?

OpenHuman is useful for anyone who:

* Manages high-volume communication across multiple platforms.
* Needs to stay on top of decisions, action items and context without reading everything.
* Works in distributed teams or coordination-heavy environments.
* Wants structured outputs from conversations, exportable to tools like Notion or Google Sheets.
* Wants a memory they can audit by opening a folder of Markdown.

You do not need to be technical to use it.

***

#### What platforms does OpenHuman support?

OpenHuman ships natively on **macOS, Windows and Linux** desktop. See [Install](../overview/install.md).

***

#### What integrations are available?

118+ services via [third-party integrations](../features/integrations.md): Gmail, Notion, GitHub, Slack, Stripe, Calendar, Drive, Linear, Jira, Outlook, Dropbox, Airtable, Salesforce, HubSpot, Figma, Asana, Trello, Telegram, WhatsApp, Discord, Microsoft Teams, Twitter/X, Reddit, Spotify, YouTube, Facebook, Instagram and more.

***

#### How much does OpenHuman cost?

OpenHuman offers individual and team plans with core features included. Deeper features are available in higher tiers. [Smart token compression](../features/token-compression.md) keeps usage costs low even when sweeping large amounts of data.

See [Pricing](../product/pricing.md) for details.

***

## Troubleshooting

#### Summaries feel incomplete or too broad

If a summary feels incomplete, the most common cause is overly broad scope. When a request spans many conversations, long time ranges, or high-volume groups, OpenHuman prioritizes signal over detail.

**Solution:** Narrow the request to a specific conversation, time window, or intent. OpenHuman performs best when it knows what kind of outcome you are looking for.

***

#### Important context seems missing

If relevant context exists but isn't surfacing, the source may not have been auto-fetched recently or may not be connected at all.

**Solution:** Check the Memory tab — under each source, the last sync timestamp tells you when auto-fetch last pulled data. You can also trigger a manual ingest.

***

#### Outputs feel incorrect or misinterpreted

OpenHuman interprets conversations probabilistically. Tone, sarcasm, and informal language can be misread, especially in fast-moving or meme-heavy discussions.

**Solution:** Refine the request or re-run analysis with a narrower scope. Because every chunk is also a Markdown file in your Obsidian vault, you can audit any claim back to the original source.

***

#### Auto-fetch doesn't seem to be running

Check that the integration is still connected (Skills tab → status should be **Connected**). Auto-fetch runs every five minutes globally; per-provider intervals cap the *minimum* delay between actual syncs. If a provider's daily budget is exhausted, sync pauses until the next day.

**Solution:** Reconnect the integration if status shows an error. Check the agent logs for sync activity.

***

#### A source does not appear in analysis

OpenHuman can only analyze sources you have connected. If you've connected something but it isn't appearing, give auto-fetch a tick or two — first ingest can take a few minutes for high-volume sources.

***

#### Performance feels slow

Response time depends on request complexity, data volume, and current model latency.

**Solution:** Large scopes and long histories require more processing. Narrowing scope improves responsiveness. The [model router](../features/model-routing.md) routes by hint — `hint:fast` calls land on a cheap fast model; `hint:reasoning` on a more expensive frontier one.

***

#### Revoking access and residual data

When you revoke a integration, OpenHuman immediately stops syncing it. Chunks that were ingested before revocation remain in your local Memory Tree (they're yours) — you can delete them by hand from the Markdown vault if you want them gone.

***

#### **What are the system requirements?**

OpenHuman runs on macOS, Windows and Linux. 8 GB+ RAM recommended; 16 GB+ for heavier workloads. See [Install](../overview/install.md).
