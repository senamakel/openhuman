use super::*;

#[tokio::test]
async fn dispatch_scope_uses_explicit_origin_without_mutating_shared_context() {
    let ctx = ctx("/tmp/origin-scope");
    let origin = crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel {
        channel: "test".into(),
        sender: Some("sender".into()),
        reply_target: "room".into(),
        message_id: "message".into(),
        history_key: None,
    };
    CoreContext::scope_with_turn_origin(ctx.clone(), Some(origin), async {
        assert!(matches!(
            CoreContext::current_turn_origin(),
            Some(crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel { .. })
        ));
    })
    .await;
    assert!(CoreContext::current_turn_origin().is_none());
    assert!(ctx.embedder_config().is_none());
}

#[tokio::test]
async fn missing_child_origin_inherits_bound_external_authority() {
    let ctx = ctx("/tmp/origin-inheritance");
    let external = crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel {
        channel: "test".into(),
        sender: None,
        reply_target: "room".into(),
        message_id: "message".into(),
        history_key: None,
    };
    CoreContext::scope_with_turn_origin(ctx, Some(external), async {
        let parent = CoreContext::current().expect("parent context");
        CoreContext::scope_with_turn_origin(parent, None, async {
            assert!(matches!(
                CoreContext::current_turn_origin(),
                Some(crate::agent::turn_origin::AgentTurnOrigin::ExternalChannel { .. })
            ));
        })
        .await;
    })
    .await;
}

#[tokio::test]
async fn explicit_origin_scope_bridges_legacy_and_core_context_without_cross_talk() {
    use crate::agent::turn_origin::{self, AgentTurnOrigin};

    let run = |workspace: &'static str, channel: &'static str| async move {
        let context = ctx(workspace);
        let origin = AgentTurnOrigin::ExternalChannel {
            channel: channel.into(),
            sender: Some(format!("{channel}-sender")),
            reply_target: format!("{channel}-room"),
            message_id: format!("{channel}-message"),
            history_key: None,
        };
        CoreContext::scope(context, async {
            turn_origin::with_origin(origin, async {
                assert!(matches!(
                    turn_origin::current(),
                    Some(AgentTurnOrigin::ExternalChannel { channel: current, .. })
                        if current == channel
                ));
                assert!(matches!(
                    CoreContext::current_turn_origin(),
                    Some(AgentTurnOrigin::ExternalChannel { channel: current, .. })
                        if current == channel
                ));
            })
            .await;

            assert!(turn_origin::current().is_none());
            assert!(CoreContext::current_turn_origin().is_none());
        })
        .await;
    };

    tokio::join!(
        run("/tmp/origin-a", "channel-a"),
        run("/tmp/origin-b", "channel-b")
    );
}
