use crate::openhuman::config::{Config, SearchEngine};
use crate::openhuman::tools::Tool;

use super::engines;

#[derive(Clone, Copy)]
pub(crate) struct SearchToolParams {
    pub(crate) max_results: usize,
    pub(crate) timeout_secs: u64,
}

/// Build the complete agent-facing search tool surface for the configured
/// search engine.
///
/// Exactly one engine owns the canonical `web_search_tool` slot. When search is
/// disabled, this returns an empty list so search tools are absent from both the
/// agent prompt context and the runtime tool map.
pub fn build_search_tools(root_config: &Config) -> Vec<Box<dyn Tool>> {
    let search = &root_config.search;
    let params = SearchToolParams {
        max_results: search.max_results.clamp(1, 20),
        timeout_secs: search.timeout_secs.max(1),
    };

    match search.effective_engine() {
        SearchEngine::Disabled => engines::disabled::build(root_config, params),
        SearchEngine::Managed => engines::managed::build(root_config, params),
        SearchEngine::Parallel => engines::parallel::build(root_config, params),
        SearchEngine::Brave => engines::brave::build(root_config, params),
        SearchEngine::Querit => engines::querit::build(root_config, params),
    }
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
