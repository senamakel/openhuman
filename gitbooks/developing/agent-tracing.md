---
description: >-
  Full-content Langfuse tracing for agent runs — one trace per turn, span
  hierarchy down to every LLM call, tool call and sub-agent, provider-qualified
  models, real token counts and dollar costs, and graph-view exports for
  workflow runs.
icon: chart-network
---

# Agent Tracing (Langfuse)

OpenHuman ships production-grade tracing for every agent turn. This page covers the **Langfuse run-tracing layer**; the E2E test artifact-capture layer lives on the separate [Agent Observability](agent-observability.md) page.

The design decision behind it: **traces without content are not actionable.** By default (`capture_content = true`), traces carry the actual prompts, completions, tool arguments and results — truncated and redacted, not stripped — because a trace that only says "llm.chat-v1, 3.2s, ok" cannot tell you *why* an agent went sideways.

***

## Configuration

Everything lives in the `[observability]` config block (`src/openhuman/config/schema/observability.rs`):

| Key | Default | Meaning |
| --- | --- | --- |
| `share_usage_data` | `true` | Consent to push traces to the OpenHuman backend's Langfuse proxy. |
| `agent_tracing.enabled` | `false` | Opt-in **local** exporter (NDJSON file or app log). |
| `agent_tracing.backend` | `otel` | Local export format: `otel` or `langfuse`. |
| `agent_tracing.export_path` | — | NDJSON file path; falls back to the app log. |
| `agent_tracing.capture_content` | `true` | Include prompt/completion/tool content in spans. |

The two paths are independent: usage-data sharing feeds the hosted Langfuse project; the local exporter writes spans on your machine. Both are best-effort — a tracing failure is never fatal to a turn.

**Key handling:** clients POST batches to the backend proxy route `/telemetry/langfuse/ingestion`, authenticated with the normal OpenHuman session bearer. The backend injects the Langfuse project keys server-side. **Clients never hold Langfuse credentials.**

***

## The trace model — one trace per turn

The span pipeline (`src/openhuman/agent/progress_tracing.rs`) folds the harness's `AgentProgress` event stream through a pure `SpanCollector` state machine into a span tree:

```
agent.turn                          ← root span, one Langfuse trace per turn
└── agent.iteration#1
    ├── llm.managed.chat-v1         ← Generation (provider-qualified model)
    ├── tool.web_search
    └── subagent.researcher
        └── subagent.iteration#1
            ├── llm.openai.gpt-4o
            └── tool.web_fetch
```

* **Trace id = the per-turn session id**, so every turn is its own trace; Langfuse `sessionId` is set to the thread id, so turns group under one conversation.
* **`userId`**, `client.id`, `agent.id`, and channel source ride along as identity metadata.
* Every trace is tagged with a **run type** — `interactive_chat`, `autonomous_task`, `agentbox`, or `channel_inbound` — so you can slice production traffic by origin.

### Generations: real models, real costs

Each LLM call becomes a Generation span named `llm.<model>` with a **provider-qualified label** (`managed.chat-v1`, `openai.gpt-4o`) plus:

* `gen_ai.usage.*` token attributes — input, output, cached input, cache creation, reasoning tokens — promoted into Langfuse-native `usageDetails`.
* `gen_ai.usage.cost_usd` in `costDetails`, using the provider's actual `charged_amount_usd` when available.
* An **auditable pricing basis**: `gen_ai.pricing.{input,cached_input,output}_per_mtok_usd` from the in-core pricing catalog, so a cost figure is always reproducible.

Sub-agent model calls roll their usage and cost up onto the owning `subagent.*` span.

### Tools and failures

Tool spans carry `tool.name`, `tool.call_id`, `tool.success`, `tool.output_chars`, `tool.elapsed_ms`. A failed tool sets the span to `ERROR` with the *classified* plain-English cause as the status message — the same [failure classification](architecture/agent-harness.md) the chat timeline renders.

### What is captured — and what never is

With content capture on: the turn prompt and final reply, truncated generation request (including system prompt) and completion (cap 25k chars), truncated tool args/results (cap 4k chars), sub-agent prompt and output, error messages (cap 500 chars).

Never exported regardless of the flag: streaming deltas (text/thinking/tool-arg), raw error strings when capture is off, and filesystem paths. Before anything persists locally, a `RedactingSink` masks process credentials.

***

## Workflow runs: the Agent Graph view

[Workflow](../features/workflows.md) runs export as graph traces (`src/openhuman/tinyflows/langfuse_export.rs`): after `flows_run`/`flows_resume`, the run's durable `GraphObservation` slice becomes one Langfuse trace where **each node/superstep is a timed span** stamped with `langgraph_node`/`langgraph_step` metadata — Langfuse renders the run in its Agent Graph view. Traces are named `flow.run:<flow_name>`, tagged `run:flow` + `trigger:<schedule|app_event|rpc|resume>`, and stamped with the app version as the release.

***

## Replayable run journals

Independent of Langfuse, every run writes a **durable event journal** (`src/openhuman/tinyagents/journal.rs`): a JSONL append store at `{workspace}/tinyagents_store/journal`, with restart-stable event ids (`{run_id}-evt-{offset}`). Even an unobserved background turn is reconstructable after the fact.

Three read-only RPCs expose it:

* `openhuman.agent_run_events` — paged, late-attach replay of a run's events (`run_id`, `offset`, `limit`).
* `openhuman.agent_run_status` — the latest harness status for a run.
* `openhuman.agent_run_list` — active runs, filterable by thread or root run.

***

## See also

* [Agent Harness](architecture/agent-harness.md) — the tinyagents turn loop these events come from.
* [Agent Observability](agent-observability.md) — E2E test artifact capture (screenshots, page source, mock request logs).
* [Billing, Cost & Usage](../features/billing-and-usage.md) — the user-facing cost surface fed by the same accounting.
