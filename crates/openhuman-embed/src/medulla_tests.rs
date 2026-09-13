use openhuman_core::medulla::all_medulla_registered_controllers;

/// Every method this facade dispatches must name a registered controller.
///
/// The facade's method names are strings; a typo or a renamed controller
/// would otherwise surface as `CoreError::Unavailable` at runtime, which is
/// indistinguishable from a domain the host gated off on purpose. Pinning
/// them here turns that into a test failure.
#[test]
fn every_dispatched_method_is_registered() {
    let registered: Vec<String> = all_medulla_registered_controllers()
        .iter()
        .map(|c| c.rpc_method_name())
        .collect();

    for method in [
        "openhuman.medulla_status",
        "openhuman.medulla_list_sessions",
        "openhuman.medulla_create_session",
        "openhuman.medulla_get_session",
        "openhuman.medulla_send_message",
        "openhuman.medulla_abort",
        "openhuman.medulla_list_messages",
        "openhuman.medulla_list_events",
        "openhuman.medulla_roster",
    ] {
        assert!(
            registered.iter().any(|m| m == method),
            "facade dispatches `{method}`, which no controller registers. \
                 Registered: {registered:?}"
        );
    }
}
