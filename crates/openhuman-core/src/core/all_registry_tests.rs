use super::*;

#[tokio::test]
async fn doctor_report_is_in_both_scoped_saas_dispatch_and_schema_surfaces() {
    use crate::core::runtime::{mode, ContextOverlay, CoreContext, DomainSet, Mode};

    mode::lock_mode(Mode::Saas).expect("test process uses SaaS mode");
    let doctor = "openhuman.doctor_report";
    let doctor_models = "openhuman.doctor_models";
    let user_domains = crate::profiles::host::user_domains();
    let user_context = CoreContext::for_test(user_domains, None).derive_with(
        ContextOverlay::new(
            crate::config::Config::default(),
            user_domains,
            crate::tools::toolpacks::ToolGroups::none(),
        )
        .profile("alice"),
    );

    let (operator_methods, operator_schemas) = CoreContext::scope(
        CoreContext::for_test(DomainSet::saas(), None),
        async {
            (
                all_registered_controllers()
                    .iter()
                    .map(RegisteredController::rpc_method_name)
                    .collect::<Vec<_>>(),
                all_controller_schemas()
                    .iter()
                    .map(rpc_method_name)
                    .collect::<Vec<_>>(),
            )
        },
    )
    .await;
    let (user_methods, user_schemas) = CoreContext::scope(user_context, async {
        (
            all_registered_controllers()
                .iter()
                .map(RegisteredController::rpc_method_name)
                .collect::<Vec<_>>(),
            all_controller_schemas()
                .iter()
                .map(rpc_method_name)
                .collect::<Vec<_>>(),
        )
    })
    .await;

    assert!(operator_methods.iter().any(|method| method == doctor));
    assert!(operator_schemas.iter().any(|method| method == doctor));
    assert!(user_methods.iter().any(|method| method == doctor));
    assert!(user_schemas.iter().any(|method| method == doctor));
    assert!(!operator_methods.iter().any(|method| method == doctor_models));
    assert!(!user_methods.iter().any(|method| method == doctor_models));
    assert!(!user_methods
        .iter()
        .any(|method| method == "openhuman.config_get_config"));
}

#[tokio::test]
async fn full_registration_is_byte_identical() {
    // With no ambient CoreContext (⇒ full, no filter), the public
    // `all_registered_controllers()` must equal the raw grouped registry — same
    // length AND same rpc-method-name sequence IN ORDER. This is the DoD (1)
    // proof that wrapping every entry in a `GroupedController` + filtering by the
    // ambient DomainSet changes neither the membership nor the ordering of the
    // full() surface.
    //
    // The baseline is the raw `registry()` view rather than a checked-in method
    // snapshot (a #4808 review suggestion): `all_registered_controllers()` and
    // `registry()` are DIFFERENT code paths — the former exercises the ambient
    // filter (`group_allowed`) and re-collects, the latter is the unfiltered
    // source — so this asserts the filter is an order-preserving identity under
    // full(). A frozen snapshot would instead ossify the controller list and
    // force churn on every legitimate new controller; git history is the
    // authoritative pre-#4796 baseline for "did the raw list itself change".
    // Tests in this binary may have initialized the process default context,
    // whose DomainSet can be narrower than full(). Scope the assertion
    // explicitly so parallel test order cannot change the registry surface.
    let ctx = CoreContext::for_test(DomainSet::full(), None);
    let (filtered_methods, raw_methods) = CoreContext::scope(ctx, async {
        // Capture extensions once: other registry tests may append extensions
        // concurrently, and each public lookup intentionally takes a fresh
        // snapshot.
        let view = registry_view();
        let filtered = registered_controllers(&view)
            .iter()
            .map(|c| c.rpc_method_name())
            .collect::<Vec<_>>();
        // `full()` is every family a single-user core has; the SaaS operator
        // plane is the one family it deliberately leaves out.
        let raw = view
            .iter()
            .filter(|g| g.group != DomainGroup::Operator)
            .map(|g| g.controller.rpc_method_name())
            .collect::<Vec<_>>();
        (filtered, raw)
    })
    .await;

    assert_eq!(
        filtered_methods.len(),
        raw_methods.len(),
        "unfiltered all_registered_controllers() must equal raw registry length"
    );
    // Ordered comparison — NOT sorted. A reordering (or a drop/add) under full()
    // would change dispatch/schema iteration order and must fail here.
    assert_eq!(
        filtered_methods, raw_methods,
        "unfiltered rpc-method sequence must be byte-identical (order + membership) to the raw registry"
    );
}
