---
description: A native search tool the agent can call directly - no API key required.
icon: magnifying-glass
---

# Web Search

The agent can search the live web on its own. Backed by a server-side proxy (Parallel) so you don't carry a search API key, the tool returns titles, snippets, and URLs ready to follow up on.

## What it's good for

* Research - "what's the latest on X".
* Citation hunting - "find me three sources for Y".
* Fact-checking before answering - the agent runs a quick search if it isn't confident.

## How it differs from generic HTTP

A pure `http_request` tool can fetch a URL but can't *find* one. Web Search is the discovery layer: it picks the right URLs for the agent, which then hands them off to the [Web Scraper](web-scraper.md) for the actual reading.

## See also

* [Web Scraper](web-scraper.md) - fetch and clean a specific URL.
* [Smart Token Compression](../token-compression.md) - search snippets are compressed before they hit the model.
