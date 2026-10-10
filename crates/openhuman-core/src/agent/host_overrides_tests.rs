use super::*;
use async_trait::async_trait;

struct Hook;
#[async_trait]
impl PostTurnHook for Hook {
    fn name(&self) -> &str {
        "local"
    }
    async fn on_turn_complete(&self, _: &crate::agent::hooks::TurnContext) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn local_hooks_do_not_escape_their_agent_and_can_be_removed() {
    let a = HostOverrides::default();
    let b = HostOverrides::default();
    a.post_turn_hook("local", Some(Arc::new(Hook)));
    assert_eq!(a.post_turn_hooks().len(), 1);
    assert!(b.post_turn_hooks().is_empty());
    a.post_turn_hook("local", None);
    assert!(a.post_turn_hooks().is_empty());
}

#[tokio::test]
async fn caller_cancellation_is_shared_and_does_not_escape_its_scope() {
    let token = CancellationToken::new();
    with_cancellation(token.clone(), async {
        token.cancel();
        assert!(current_cancellation().is_cancelled());
    })
    .await;
    assert!(!current_cancellation().is_cancelled());
}
