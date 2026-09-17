use std::sync::Arc;

use crate::config::Config;
use crate::search::registry::SearchToolParams;
use crate::tools::Tool;

pub(crate) fn build(root_config: &Config, params: SearchToolParams) -> Vec<Box<dyn Tool>> {
    tracing::debug!("[search] active engine = parallel (BYO direct API)");

    let client = crate::integrations::build_client(root_config);
    let Some(client) = client else {
        tracing::warn!(
            "[search] engine=parallel but no backend client — falling back to managed surface"
        );
        return vec![Box::new(crate::search::WebSearchTool::new(
            None,
            Some(Arc::new(root_config.clone())),
            params.max_results,
            params.timeout_secs,
        ))];
    };

    vec![
        Box::new(crate::search::tools::ParallelSearchTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::tools::ParallelExtractTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::tools::ParallelChatTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::tools::ParallelResearchTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::tools::ParallelEnrichTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::tools::ParallelDatasetTool::new(Arc::clone(
            &client,
        ))),
        Box::new(crate::search::WebSearchTool::new(
            Some(Arc::clone(&client)),
            Some(Arc::new(root_config.clone())),
            params.max_results,
            params.timeout_secs,
        )),
    ]
}
