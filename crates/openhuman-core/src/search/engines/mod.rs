//! One `pub(crate) fn build(&Config, SearchToolParams) -> Vec<Box<dyn Tool>>`
//! per search engine; `registry::build_search_tools` dispatches to exactly one
//! based on `search.effective_engine()`.

pub(crate) mod brave;
pub(crate) mod disabled;
pub(crate) mod exa;
pub(crate) mod managed;
pub(crate) mod parallel;
pub(crate) mod querit;
pub(crate) mod tavily;
