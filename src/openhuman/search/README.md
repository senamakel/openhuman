# Search Domain

Top-level home for web search selection and agent-facing search tool registration.

## Shape

- `registry.rs` builds the active search tool surface from `Config.search`.
- `tools/` contains search-owned agent tools such as `WebSearchTool`.
- Provider-specific direct API implementations that also serve other integration surfaces can stay under `openhuman::integrations::tools`; the search registry assembles them into the active engine surface.

## Engine Behavior

`search.engine` accepts:

- `disabled` — register no search tools.
- `managed` — register backend-proxied `web_search_tool`.
- `parallel` — register the Parallel family plus `web_search_tool` when configured.
- `brave` — register Brave web/news/image/video search when configured.
- `querit` — register Querit search plus `web_search_tool` when configured.

When search is disabled, search tools are absent from the agent runtime tool list, so they do not render in agent context.
