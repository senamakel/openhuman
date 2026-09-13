//! Handlers for the search-family controller schemas: `tools_web_search`,
//! `tools_seltz_search`, `tools_querit_search`, and `tools_searxng_search`.

use serde_json::{json, Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;
use crate::rpc::RpcOutcome;
use crate::search::tools::SEARXNG_MAX_RESULTS;
use crate::tools::traits::Tool;

pub(super) fn handle_web_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "missing or empty `query`".to_string())?;
        let objective = params
            .get("objective")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| query.clone());
        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 10) as usize)
            .unwrap_or(5);
        let timeout_secs = params
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .map(|n| n.max(1))
            .unwrap_or(15);

        let config = config_rpc::load_config_with_timeout().await?;
        let client = crate::integrations::build_client(&config).ok_or_else(|| {
            "web search unavailable — no backend session token. Sign in first.".to_string()
        })?;

        // Body matches `parallelSearchSchema` (backend-2/.../validators/agentIntegration.validator.ts).
        // `timeout_secs` remains accepted in our RPC schema for compatibility
        // with existing callers, but the upstream validator currently strips
        // unknown keys and Parallel governs its own per-mode deadline.
        let _ = timeout_secs;
        let body = json!({
            "objective": objective,
            "searchQueries": [query],
            "mode": "fast",
            "excerpts": {
                "maxResults": max_results,
                "maxCharsPerResult": 500
            }
        });

        let resp = client
            .post::<crate::search::tools::SearchResponse>(
                "/agent-integrations/parallel/search",
                &body,
            )
            .await
            .map_err(|e| format!("parallel search failed: {e:#}"))?;

        let count = resp.results.len();
        // Attribute the search to the provider the managed backend resolved to,
        // so this RPC surface labels a call the same way the agent-facing
        // `web_search` tool does (#5136).
        let provider = crate::search::tools::resolve_managed_provider(&resp);
        let payload = json!({ "results": resp.results, "provider": provider });
        // Log the query's length, never its text: a search query is
        // user-authored and can carry PII or credentials. This matches the
        // sibling seltz/querit handlers below, which already log `query_len`.
        let log = vec![format!(
            "tools.web_search: query_len={} results={count} provider={provider}",
            query.chars().count()
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn handle_seltz_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "missing or empty `query`".to_string())?;
        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 20) as usize)
            .unwrap_or(10);

        let config = config_rpc::load_config_with_timeout().await?;

        if !config.seltz.enabled {
            tracing::debug!("[rpc][tools.seltz_search] seltz disabled — rejecting");
            return Err("Seltz search is not enabled. Set SELTZ_API_KEY to enable.".to_string());
        }

        let has_include_domains = params.get("include_domains").is_some();
        let has_exclude_domains = params.get("exclude_domains").is_some();
        let has_scope = params.get("scope").is_some();

        tracing::debug!(
            query_len = query.chars().count(),
            max_results,
            has_include_domains,
            has_exclude_domains,
            has_scope,
            "[rpc][tools.seltz_search] start"
        );

        let tool = crate::search::tools::SeltzSearchTool::new(
            config.seltz.api_key.clone(),
            config.seltz.api_url.clone(),
            max_results,
            config.seltz.timeout_secs,
        );

        // Build args JSON with all optional fields.
        let mut args = json!({ "query": query, "max_results": max_results });
        let args_map = args.as_object_mut().unwrap();
        if let Some(v) = params.get("include_domains") {
            args_map.insert("include_domains".to_string(), v.clone());
        }
        if let Some(v) = params.get("exclude_domains") {
            args_map.insert("exclude_domains".to_string(), v.clone());
        }
        if let Some(v) = params.get("from_date") {
            args_map.insert("from_date".to_string(), v.clone());
        }
        if let Some(v) = params.get("to_date") {
            args_map.insert("to_date".to_string(), v.clone());
        }
        if let Some(v) = params.get("scope") {
            args_map.insert("scope".to_string(), v.clone());
        }

        let result = tool
            .execute(args)
            .await
            .map_err(|e| format!("seltz search failed: {e:#}"))?;

        let payload = json!({ "documents": result.output() });
        let log = vec![format!(
            "[rpc][tools.seltz_search] success query_len={} max_results={}",
            query.chars().count(),
            max_results
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn handle_querit_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "missing or empty `query`".to_string())?;
        let max_results = params
            .get("max_results")
            .or_else(|| params.get("count"))
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 20) as usize)
            .unwrap_or(10);

        let config = config_rpc::load_config_with_timeout().await?;
        if !config.search.querit.has_key() {
            tracing::debug!("[rpc][tools.querit_search] querit not configured — rejecting");
            return Err("Querit search is not enabled. Set QUERIT_API_KEY to enable.".to_string());
        }

        let has_include_domains = params.get("include_domains").is_some();
        let has_exclude_domains = params.get("exclude_domains").is_some();
        let has_time_range = params.get("time_range").is_some();
        let has_countries = params.get("countries").is_some();
        let has_languages = params.get("languages").is_some();
        let has_native_filters = params.get("filters").is_some();

        tracing::debug!(
            query_len = query.chars().count(),
            max_results,
            has_include_domains,
            has_exclude_domains,
            has_time_range,
            has_countries,
            has_languages,
            has_native_filters,
            "[rpc][tools.querit_search] start"
        );

        let tool = crate::search::tools::QueritSearchTool::new(
            config.search.querit.api_key.clone(),
            None,
            max_results,
            config.search.timeout_secs,
        );

        let mut args = json!({ "query": query, "max_results": max_results });
        let args_map = args.as_object_mut().unwrap();
        for key in [
            "count",
            "filters",
            "include_domains",
            "exclude_domains",
            "time_range",
            "from_date",
            "to_date",
            "countries",
            "languages",
        ] {
            if let Some(v) = params.get(key) {
                args_map.insert(key.to_string(), v.clone());
            }
        }

        let result = tool
            .execute(args)
            .await
            .map_err(|e| format!("querit search failed: {e:#}"))?;

        let payload = json!({ "results": result.output() });
        let log = vec![format!(
            "[rpc][tools.querit_search] success query_len={} max_results={}",
            query.chars().count(),
            max_results
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn handle_searxng_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "missing or empty `query`".to_string())?;
        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, SEARXNG_MAX_RESULTS as u64) as usize);
        let categories = optional_string_array(&params, "categories")?;
        let language = params
            .get("language")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let config = config_rpc::load_config_with_timeout().await?;
        if !config.searxng.enabled {
            tracing::debug!("[rpc][tools.searxng_search] searxng disabled — rejecting");
            return Err(
                "SearXNG search is not enabled. Set searxng.enabled=true or OPENHUMAN_SEARXNG_ENABLED=true."
                    .to_string(),
            );
        }

        tracing::debug!(
            query_len = query.chars().count(),
            max_results = max_results.unwrap_or(config.searxng.max_results),
            category_count = categories.len(),
            has_language = language.is_some(),
            base_url = %config.searxng.base_url,
            "[rpc][tools.searxng_search] start"
        );

        let tool = crate::search::tools::SearxngSearchTool::new(
            config.searxng.base_url.clone(),
            config.searxng.max_results,
            config.searxng.default_language.clone(),
            config.searxng.timeout_secs,
        );

        let response = tool
            .search(crate::search::tools::SearxngSearchArgs {
                query,
                categories,
                language,
                max_results,
            })
            .await
            .map_err(|e| format!("searxng search failed: {e:#}"))?;

        let result_count = response.results.len();
        let payload = json!({
            "query": response.query,
            "results": response.results,
        });
        let log = vec![format!(
            "[rpc][tools.searxng_search] success results={result_count}"
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn optional_string_array(
    params: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, String> {
    let Some(value) = params.get(key) else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let items = value
        .as_array()
        .ok_or_else(|| format!("`{key}` must be an array of strings"))?;
    items
        .iter()
        .filter_map(|item| match item.as_str() {
            Some(value) => {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| Ok(trimmed.to_string()))
            }
            None => Some(Err(format!("`{key}` must contain only strings"))),
        })
        .collect()
}
