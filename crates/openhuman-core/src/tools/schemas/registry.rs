//! Controller schema definitions and dispatch table for the `tools` namespace.
//!
//! [`all_controller_schemas`] and [`all_registered_controllers`] enumerate the
//! small allowlist of tool-like operations exposed over JSON-RPC (see the module
//! doc on `super`). [`tools_schemas`] is the schema lookup shared by both.

use crate::core::all::RegisteredController;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

use super::apify::handle_apify_linkedin_scrape;
use super::composio::handle_composio_execute;
use super::web_search::{
    handle_querit_search, handle_searxng_search, handle_seltz_search, handle_web_search,
};

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        tools_schemas("tools_composio_execute"),
        tools_schemas("tools_web_search"),
        tools_schemas("tools_seltz_search"),
        tools_schemas("tools_querit_search"),
        tools_schemas("tools_searxng_search"),
        tools_schemas("tools_apify_linkedin_scrape"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: tools_schemas("tools_composio_execute"),
            handler: handle_composio_execute,
        },
        RegisteredController {
            schema: tools_schemas("tools_web_search"),
            handler: handle_web_search,
        },
        RegisteredController {
            schema: tools_schemas("tools_seltz_search"),
            handler: handle_seltz_search,
        },
        RegisteredController {
            schema: tools_schemas("tools_querit_search"),
            handler: handle_querit_search,
        },
        RegisteredController {
            schema: tools_schemas("tools_searxng_search"),
            handler: handle_searxng_search,
        },
        RegisteredController {
            schema: tools_schemas("tools_apify_linkedin_scrape"),
            handler: handle_apify_linkedin_scrape,
        },
    ]
}

pub fn tools_schemas(function: &str) -> ControllerSchema {
    match function {
        "tools_composio_execute" => ControllerSchema {
            namespace: "tools",
            function: "composio_execute",
            description: "Execute a Composio action. Routes through the mode-aware \
                          factory: backend mode proxies via the OpenHuman backend; \
                          direct mode calls backend.composio.dev with the user's own \
                          API key. Exposed for Tauri-driven flows (e.g. onboarding) \
                          that orchestrate tool calls themselves.",
            inputs: vec![
                FieldSchema {
                    name: "action",
                    ty: TypeSchema::String,
                    comment: "Composio action slug (e.g. `GMAIL_FETCH_EMAILS`).",
                    required: true,
                },
                FieldSchema {
                    name: "params",
                    ty: TypeSchema::Json,
                    comment: "Action parameters object passed straight through to Composio.",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "successful",
                    ty: TypeSchema::Bool,
                    comment: "Whether the upstream provider reported success.",
                    required: true,
                },
                FieldSchema {
                    name: "data",
                    ty: TypeSchema::Json,
                    comment: "Raw provider response.",
                    required: true,
                },
                FieldSchema {
                    name: "error",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Provider error message if `successful` is false.",
                    required: false,
                },
            ],
        },
        "tools_web_search" => ControllerSchema {
            namespace: "tools",
            function: "web_search",
            description: "Web search via the backend Parallel proxy. Returns structured \
                          results so callers can inspect titles, URLs, and excerpts \
                          without parsing the agent-facing pretty text.",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::String,
                    comment: "Search query string.",
                    required: true,
                },
                FieldSchema {
                    name: "objective",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Optional objective sent to Parallel (defaults to `query`).",
                    required: false,
                },
                FieldSchema {
                    name: "max_results",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max results (1-10, default 5).",
                    required: false,
                },
                FieldSchema {
                    name: "timeout_secs",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Request timeout in seconds (default 15).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "results",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment: "Each item: {url, title, publish_date?, excerpts[]}.",
                    required: true,
                },
                FieldSchema {
                    name: "provider",
                    ty: TypeSchema::String,
                    comment: "Upstream provider the managed backend resolved this \
                              search to, for attribution. Reported by the backend when \
                              it names one; falls back to the managed default.",
                    required: true,
                },
            ],
        },
        "tools_seltz_search" => ControllerSchema {
            namespace: "tools",
            function: "seltz_search",
            description: "Web search via the Seltz API. Returns structured results with \
                          URLs, content, and optional published dates. Supports domain \
                          filtering, date ranges, and news scope.",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::String,
                    comment: "Search query string.",
                    required: true,
                },
                FieldSchema {
                    name: "max_results",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max results (1-20, default 10).",
                    required: false,
                },
                FieldSchema {
                    name: "include_domains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Restrict results to these domains.",
                    required: false,
                },
                FieldSchema {
                    name: "exclude_domains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Exclude results from these domains.",
                    required: false,
                },
                FieldSchema {
                    name: "from_date",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Only results published on or after (YYYY-MM-DD).",
                    required: false,
                },
                FieldSchema {
                    name: "to_date",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Only results published on or before (YYYY-MM-DD).",
                    required: false,
                },
                FieldSchema {
                    name: "scope",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Restrict to a scope, e.g. \"news\".",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "documents",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "Each item: {url, content, title?, published_date?}.",
                required: true,
            }],
        },
        "tools_querit_search" => ControllerSchema {
            namespace: "tools",
            function: "querit_search",
            description: "Web search via the Querit API. Returns current results with URLs, \
                          snippets, site names, and page age. Supports site filters, \
                          time ranges, country filters, and language filters.",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::String,
                    comment: "Search query string.",
                    required: true,
                },
                FieldSchema {
                    name: "max_results",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max results (1-20, default 10).",
                    required: false,
                },
                FieldSchema {
                    name: "count",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Querit-native alias for max_results.",
                    required: false,
                },
                FieldSchema {
                    name: "filters",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment:
                        "Querit-native filters object with sites, timeRange, geo, and languages.",
                    required: false,
                },
                FieldSchema {
                    name: "include_domains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Only fetch results from these domains.",
                    required: false,
                },
                FieldSchema {
                    name: "exclude_domains",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Exclude results from these domains.",
                    required: false,
                },
                FieldSchema {
                    name: "time_range",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Querit date filter: d7, w2, m6, y1, or YYYY-MM-DDtoYYYY-MM-DD.",
                    required: false,
                },
                FieldSchema {
                    name: "from_date",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Start date for a Querit date-range filter (YYYY-MM-DD).",
                    required: false,
                },
                FieldSchema {
                    name: "to_date",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "End date for a Querit date-range filter (YYYY-MM-DD).",
                    required: false,
                },
                FieldSchema {
                    name: "countries",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Country filters, e.g. united states, japan, germany.",
                    required: false,
                },
                FieldSchema {
                    name: "languages",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::String,
                    )))),
                    comment: "Language filters, e.g. english, japanese, german.",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "results",
                ty: TypeSchema::String,
                comment: "Formatted Querit search results.",
                required: true,
            }],
        },
        "tools_searxng_search" => ControllerSchema {
            namespace: "tools",
            function: "searxng_search",
            description:
                "Web search via a user-configured SearXNG instance. Returns normalized \
                          results with title, URL, snippet, and source. Intended for private, \
                          self-hosted search without routing queries through the OpenHuman backend.",
            inputs: vec![
                FieldSchema {
                    name: "query",
                    ty: TypeSchema::String,
                    comment: "Search query string.",
                    required: true,
                },
                FieldSchema {
                    name: "categories",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Array(Box::new(
                        TypeSchema::Enum {
                            variants: vec!["web", "general", "news", "images"],
                        },
                    )))),
                    comment: "Optional SearXNG categories. `web` maps to SearXNG `general`.",
                    required: false,
                },
                FieldSchema {
                    name: "language",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "Optional language code, e.g. `en`, `zh-CN`, or `fr`.",
                    required: false,
                },
                FieldSchema {
                    name: "max_results",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Max results (1-50, default from searxng.max_results).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "results",
                ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                comment: "Each item: {title, url, snippet, source}.",
                required: true,
            }],
        },
        "tools_apify_linkedin_scrape" => ControllerSchema {
            namespace: "tools",
            function: "apify_linkedin_scrape",
            description: "Run the Apify LinkedIn profile scraper actor on a single profile \
                          URL and return both the raw scraped item and a pre-rendered \
                          markdown view of it (same layout as the legacy enrichment pipeline).",
            inputs: vec![FieldSchema {
                name: "profile_url",
                ty: TypeSchema::String,
                comment: "Canonical LinkedIn profile URL (`https://www.linkedin.com/in/<slug>`).",
                required: true,
            }],
            outputs: vec![
                FieldSchema {
                    name: "data",
                    ty: TypeSchema::Json,
                    comment: "Raw scraped profile JSON from Apify.",
                    required: true,
                },
                FieldSchema {
                    name: "markdown",
                    ty: TypeSchema::String,
                    comment: "Markdown rendering of the scraped profile (full, pre-summary).",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "tools",
            function: "unknown",
            description: "Unknown tools controller.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}
