//! The tool trait and its vocabulary — the stable host import path.
//!
//! Every definition here lives in [`tinytools`], which `tinyagents` also
//! depends on. That is what makes `tinytools::Tool` and the trait the harness
//! runs a loop over the *same* trait: a tool is implemented once and both sides
//! accept it, with no conversion at the seam to get subtly wrong.
//!
//! ~190 call sites in this crate import from `crate::openhuman::tools::traits`,
//! so this module stays as the path they name rather than rewriting each import
//! to point at the crate. New code may name either.
//!
//! # What did *not* move
//!
//! Everything that decides something. `tinytools` lets a tool declare the
//! privilege it needs and whether it reaches outside the machine; this host
//! decides what to do about those declarations — [`policy`], the security
//! policy, the approval gate and the sandbox are all still ours, and are meant
//! to stay in one auditable place.
//!
//! [`policy`]: crate::openhuman::tools::policy

pub use tinytools::{
    context_detail_from_args, humanize_tool_name, PermissionLevel, Tool, ToolCallOptions,
    ToolCategory, ToolContent, ToolResult, ToolRunContext, ToolScope, ToolSpec, ToolTimeout,
};

use crate::openhuman::agent::tool_policy::GeneratedToolRuntimeContext;
use crate::openhuman::tools::toolpacks::PackRegistryHandle;

/// Reads a tool's pack-registry handle back out of the erased host extension.
///
/// `load_skill` / `use_skill` read the registry they themselves live in, so
/// they cannot be handed it at construction; `toolpacks::bind_pack_registry`
/// finds them in an already-built registry and hands them a `Weak` view of it.
///
/// The handle rides on [`Tool::host_extension`] rather than a typed trait
/// method because it is *this host's* concept — a vocabulary shared with other
/// hosts has no business naming it. Every other tool returns `None` here and
/// pays nothing.
pub fn pack_registry_handle(tool: &dyn Tool) -> Option<&PackRegistryHandle> {
    tool.host_extension()
        .and_then(|any| any.downcast_ref::<PackRegistryHandle>())
}

/// Reads a tool's generated-tool runtime metadata back out of the erased
/// per-call host extension.
///
/// Generated or externally supplied tools carry this so the agent policy layer
/// can apply provider / capability / risk rules before execution. Built-in
/// tools leave it unset. Erased for the same reason as
/// [`pack_registry_handle`].
pub fn generated_runtime_context(
    tool: &dyn Tool,
    args: &serde_json::Value,
) -> Option<GeneratedToolRuntimeContext> {
    tool.host_call_extension(args)
        .and_then(|any| any.downcast::<GeneratedToolRuntimeContext>().ok())
        .map(|boxed| *boxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy_tool"
        }

        fn description(&self) -> &str {
            "A deterministic test tool"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "value": { "type": "string" } }
            })
        }

        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
            let text = args
                .get("value")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(ToolResult::success(text))
        }
    }

    #[tokio::test]
    async fn a_tool_written_against_this_path_satisfies_the_shared_trait() {
        // The point of the re-export: `dyn Tool` here is `dyn tinytools::Tool`,
        // which is what the harness accepts. If these ever became two traits,
        // this coercion is what would stop compiling.
        let erased: &dyn tinytools::Tool = &DummyTool;
        let result = erased
            .execute(serde_json::json!({ "value": "hello-tool" }))
            .await
            .expect("the tool runs");
        assert_eq!(result.output(), "hello-tool");
        assert_eq!(erased.permission_level(), PermissionLevel::ReadOnly);
        assert_eq!(erased.scope(), ToolScope::All);
        assert_eq!(erased.category(), ToolCategory::System);
    }

    #[test]
    fn a_tool_carrying_no_host_extension_yields_none() {
        let tool = DummyTool;
        assert!(pack_registry_handle(&tool).is_none());
        assert!(generated_runtime_context(&tool, &serde_json::Value::Null).is_none());
    }

    #[test]
    fn spec_uses_tool_metadata_and_schema() {
        let spec = DummyTool.spec();
        assert_eq!(spec.name, "dummy_tool");
        assert_eq!(spec.description, "A deterministic test tool");
        assert_eq!(spec.parameters["type"], "object");
    }
}
