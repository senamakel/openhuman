use super::*;
use crate::core::runtime::{context::CoreContext, ContextOverlay, DomainSet};
use crate::tools::toolpacks::ToolGroups;

fn agent(parent: &Arc<CoreContext>, id: &str) -> Arc<CoreContext> {
    parent.derive_with(
        ContextOverlay::new(
            crate::config::Config::default(),
            DomainSet::kernel(),
            ToolGroups::none(),
        )
        .session_agent(id),
    )
}

#[tokio::test]
async fn each_agent_context_owns_its_turn_tables() {
    let root = CoreContext::for_test(DomainSet::full(), None);
    let alpha = agent(&root, "alpha");
    let beta = agent(&root, "beta");

    let (alpha_sessions, alpha_in_flight, alpha_parallel) =
        CoreContext::scope(Arc::clone(&alpha), async {
            (thread_sessions(), in_flight(), parallel_in_flight())
        })
        .await;
    let (beta_sessions, beta_in_flight, beta_parallel) = CoreContext::scope(beta, async {
        (thread_sessions(), in_flight(), parallel_in_flight())
    })
    .await;
    let alpha_again = CoreContext::scope(alpha, async { in_flight() }).await;

    assert!(!Arc::ptr_eq(&alpha_sessions, &beta_sessions));
    assert!(!Arc::ptr_eq(&alpha_in_flight, &beta_in_flight));
    assert!(!Arc::ptr_eq(&alpha_parallel, &beta_parallel));
    assert!(Arc::ptr_eq(&alpha_in_flight, &alpha_again));
}

#[tokio::test]
async fn two_profiles_on_the_same_thread_id_hold_two_keys() {
    let root = CoreContext::for_test(DomainSet::full(), None);
    let profile = |id: &str| {
        root.derive_with(
            ContextOverlay::new(
                crate::config::Config::default(),
                DomainSet::kernel(),
                ToolGroups::none(),
            )
            .profile(id)
            .session_agent(id),
        )
    };
    let (alice, bob) = (profile("u-alice"), profile("u-bob"));
    let key_a = CoreContext::scope(Arc::clone(&alice), async { key_for("t1") }).await;
    let key_b = CoreContext::scope(Arc::clone(&bob), async { key_for("t1") }).await;
    assert_ne!(key_a, key_b);
    assert_eq!(
        CoreContext::scope(Arc::clone(&alice), async { unscope(&key_a) }).await,
        "t1"
    );
    assert_eq!(
        CoreContext::scope(Arc::clone(&alice), async { unscope(&key_b) }).await,
        key_b,
        "another tenant's key is not unscoped"
    );
    assert_eq!(thread_id_of_key(&key_b), Some("t1"));
    // The desktop keeps the bare id.
    assert_eq!(key_in(&crate::core::runtime::Tenant::default(), "t1"), "t1");
}

#[tokio::test]
async fn a_user_cancel_under_the_fence_placeholder_id_keeps_the_callers_client() {
    let token = tokio_util::sync::CancellationToken::new();
    track_parallel_turn_for_test("fence-thread", "fence-req", token.clone()).await;
    let mut events = crate::web_chat::subscribe_web_channel_events();

    crate::web_chat::ops::channel_ops::cancel_chat_scoped("profile-fence", "fence-thread", None)
        .await
        .unwrap();

    assert!(token.is_cancelled());
    let mut seen = None;
    while let Ok(event) = events.try_recv() {
        if event.event == "chat_cancelled" && event.request_id == "fence-req" {
            seen = Some(event.client_id);
        }
    }
    assert_eq!(seen.as_deref(), Some("profile-fence"));
}
