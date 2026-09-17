use crate::config::Config;
use crate::search::registry::SearchToolParams;
use crate::tools::Tool;

pub(crate) fn build(root_config: &Config, params: SearchToolParams) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[search] active engine = brave (BYO direct API)");

    let api_key = root_config.search.brave.api_key.clone();
    vec![
        Box::new(crate::search::tools::BraveWebSearchTool::new(
            api_key.clone(),
            params.max_results,
            params.timeout_secs,
        )),
        Box::new(crate::search::tools::BraveNewsSearchTool::new(
            api_key.clone(),
            params.max_results,
            params.timeout_secs,
        )),
        Box::new(crate::search::tools::BraveImageSearchTool::new(
            api_key.clone(),
            params.max_results,
            params.timeout_secs,
        )),
        Box::new(crate::search::tools::BraveVideoSearchTool::new(
            api_key,
            params.max_results,
            params.timeout_secs,
        )),
    ]
}
