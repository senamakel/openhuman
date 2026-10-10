//! Inline host approval carried by the agent/turn tool-hook scope.

use crate::seams::{ToolHook, ToolHookContext, ToolHookDecision};
/// A sendable asynchronous permission decision borrowing its tool context.
pub type PermissionFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = ToolHookDecision> + Send + 'a>>;

pub(crate) struct PermissionHook<F>(pub(crate) F);

#[async_trait::async_trait]
impl<F> ToolHook for PermissionHook<F>
where
    F: for<'a> Fn(&'a ToolHookContext) -> PermissionFuture<'a> + Send + Sync,
{
    fn name(&self) -> &str {
        "embed.can_use_tool"
    }
    async fn before_tool(&self, _: &ToolHookContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn after_tool(&self, _: &ToolHookContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn before_tool_decision(&self, context: &ToolHookContext) -> ToolHookDecision {
        (self.0)(context).await
    }
}
