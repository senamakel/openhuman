use std::sync::Arc;

use crate::openhuman::config::{Config, SearchEngine};
use crate::openhuman::tools::Tool;

/// Build the complete agent-facing search tool surface for the configured
/// search engine.
///
/// Exactly one engine owns the canonical `web_search_tool` slot. When search is
/// disabled, this returns an empty list so search tools are absent from both the
/// agent prompt context and the runtime tool map.
pub fn build_search_tools(root_config: &Config) -> Vec<Box<dyn Tool>> {
    let search = &root_config.search;
    let max_results = search.max_results.clamp(1, 20);
    let timeout_secs = search.timeout_secs.max(1);
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    match search.effective_engine() {
        SearchEngine::Disabled => {
            tracing::debug!("[search] disabled — no search tools registered");
        }
        SearchEngine::Managed => {
            tracing::debug!(
                requested = %search.requested_engine_str(),
                "[search] active engine = managed (backend-proxied web_search)"
            );
            tools.push(Box::new(crate::openhuman::search::WebSearchTool::new(
                crate::openhuman::integrations::build_client(root_config),
                max_results,
                timeout_secs,
            )));
        }
        SearchEngine::Parallel => {
            tracing::debug!("[search] active engine = parallel (BYO direct API)");
            let client = crate::openhuman::integrations::build_client(root_config);
            if let Some(client) = client {
                tools.push(Box::new(crate::openhuman::tools::ParallelSearchTool::new(
                    Arc::clone(&client),
                )));
                tools.push(Box::new(crate::openhuman::tools::ParallelExtractTool::new(
                    Arc::clone(&client),
                )));
                tools.push(Box::new(crate::openhuman::tools::ParallelChatTool::new(
                    Arc::clone(&client),
                )));
                tools.push(Box::new(
                    crate::openhuman::tools::ParallelResearchTool::new(Arc::clone(&client)),
                ));
                tools.push(Box::new(crate::openhuman::tools::ParallelEnrichTool::new(
                    Arc::clone(&client),
                )));
                tools.push(Box::new(crate::openhuman::tools::ParallelDatasetTool::new(
                    Arc::clone(&client),
                )));
                tools.push(Box::new(crate::openhuman::search::WebSearchTool::new(
                    Some(Arc::clone(&client)),
                    max_results,
                    timeout_secs,
                )));
            } else {
                tracing::warn!(
                    "[search] engine=parallel but no backend client — falling back to managed surface"
                );
                tools.push(Box::new(crate::openhuman::search::WebSearchTool::new(
                    None,
                    max_results,
                    timeout_secs,
                )));
            }
        }
        SearchEngine::Brave => {
            tracing::debug!("[search] active engine = brave (BYO direct API)");
            let api_key = search.brave.api_key.clone();
            tools.push(Box::new(crate::openhuman::tools::BraveWebSearchTool::new(
                api_key.clone(),
                max_results,
                timeout_secs,
            )));
            tools.push(Box::new(crate::openhuman::tools::BraveNewsSearchTool::new(
                api_key.clone(),
                max_results,
                timeout_secs,
            )));
            tools.push(Box::new(
                crate::openhuman::tools::BraveImageSearchTool::new(
                    api_key.clone(),
                    max_results,
                    timeout_secs,
                ),
            ));
            tools.push(Box::new(
                crate::openhuman::tools::BraveVideoSearchTool::new(
                    api_key,
                    max_results,
                    timeout_secs,
                ),
            ));
        }
        SearchEngine::Querit => {
            tracing::debug!("[search] active engine = querit (BYO direct API)");
            tools.push(Box::new(
                crate::openhuman::tools::QueritSearchTool::new_web_search_tool(
                    search.querit.api_key.clone(),
                    None,
                    max_results,
                    timeout_secs,
                ),
            ));
            tools.push(Box::new(crate::openhuman::tools::QueritSearchTool::new(
                search.querit.api_key.clone(),
                None,
                max_results,
                timeout_secs,
            )));
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use crate::openhuman::config::Config;

    #[test]
    fn disabled_engine_registers_no_search_tools() {
        let mut cfg = Config::default();
        cfg.search.engine = "disabled".to_string();

        let tools = super::build_search_tools(&cfg);

        assert!(tools.is_empty());
    }

    #[test]
    fn managed_engine_registers_unified_web_search_tool() {
        let mut cfg = Config::default();
        cfg.search.engine = "managed".to_string();

        let tools = super::build_search_tools(&cfg);
        let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();

        assert_eq!(names, vec!["web_search_tool"]);
    }

    #[test]
    fn brave_engine_registers_brave_search_family() {
        let mut cfg = Config::default();
        cfg.search.engine = "brave".to_string();
        cfg.search.brave.api_key = Some("test-key".to_string());

        let tools = super::build_search_tools(&cfg);
        let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "web_search_tool",
                "brave_news_search",
                "brave_image_search",
                "brave_video_search"
            ]
        );
    }
}
