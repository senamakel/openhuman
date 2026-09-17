use crate::config::Config;
use crate::search::registry::SearchToolParams;
use crate::tools::Tool;

/// Tavily BYOK: every tool here calls `https://api.tavily.com` directly with the
/// user's own key. Nothing in this surface routes through the managed backend.
pub(crate) fn build(root_config: &Config, params: SearchToolParams) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[search] active engine = tavily (BYO direct API)");

    let api_key = root_config.search.tavily.api_key.clone();
    vec![
        Box::new(crate::search::tools::TavilySearchTool::new_web_search_tool(
            api_key.clone(),
            None,
            params.max_results,
            params.timeout_secs,
        )),
        Box::new(crate::search::tools::TavilySearchTool::new(
            api_key.clone(),
            None,
            params.max_results,
            params.timeout_secs,
        )),
        Box::new(crate::search::tools::TavilyExtractTool::new(
            api_key,
            None,
            params.max_results,
            params.timeout_secs,
        )),
    ]
}
