use super::*;
use crate::core::cli_capability::{capability_verdict, CAPABILITY_UNAVAILABLE_PREFIX};
use crate::core::subsystem::DriverClass;
use tinymemory_api::capabilities::{Capabilities, Capability};

/// Drift guard: a renamed controller function must break here rather than
/// silently un-gate a subcommand (`required_capability` would start
/// returning `None` for it).
#[test]
fn memory_cli_subcommands_mirror_real_controllers() {
    for (sub, function) in SUBCOMMAND_CONTROLLER {
        assert!(
            crate::core::all::rpc_method_from_parts("memory", function).is_some(),
            "`openhuman memory {sub}` maps to memory.{function}, which is not registered"
        );
    }
}

/// Adding a subcommand without recording a capability decision fails here.
#[test]
fn every_dispatched_subcommand_is_in_the_controller_table() {
    for sub in [
        "ingest",
        "docs",
        "list",
        "graph",
        "graph-query",
        "query",
        "namespaces",
        "ns",
        "clear",
    ] {
        assert!(
            SUBCOMMAND_CONTROLLER.iter().any(|(s, _)| *s == sub),
            "`openhuman memory {sub}` is dispatched but has no controller mapping"
        );
    }
}

#[test]
fn ingest_and_graph_are_the_gated_subcommands() {
    assert_eq!(required_capability("ingest"), Some(Capability::Ingest));
    assert_eq!(required_capability("graph"), Some(Capability::Graph));
    assert_eq!(required_capability("graph-query"), Some(Capability::Graph));
    // Core/recall share the Core gate so a null driver can remove the
    // complete driver-backed memory surface.
    for sub in ["docs", "list", "query", "namespaces", "ns", "clear"] {
        assert_eq!(
            required_capability(sub),
            Some(Capability::Core),
            "{sub} must use Core"
        );
    }
}

/// A real typo must never be reported as a capability fact.
#[test]
fn unknown_memory_subcommand_still_reports_unknown_subcommand() {
    let err = run_memory_command(&["not_a_subcommand".to_string()])
        .expect_err("an unknown subcommand must error");
    let msg = err.to_string();
    assert!(msg.contains("unknown memory subcommand"), "{msg}");
    assert!(!msg.contains(CAPABILITY_UNAVAILABLE_PREFIX), "{msg}");
    assert_eq!(required_capability("not_a_subcommand"), None);
}

/// `Capabilities::mandatory()` is exactly what the `null` driver advertises;
/// the set is used directly rather than through `binding::for_workspace` so
/// this file stays off the memory-guard bypass allowlist (that scanner does
/// not strip inline `#[cfg(test)]` modules). The binding-level equivalence
/// is pinned in `cli_capability_tests.rs`.
#[test]
fn gated_subcommand_reports_the_driver_and_capability() {
    let err = capability_verdict(
        "null",
        Capabilities::mandatory(),
        required_capability("ingest"),
        "openhuman memory ingest",
    )
    .expect_err("the null driver does not advertise `ingest`");
    let msg = err.to_string();
    assert!(msg.contains("null"), "{msg}");
    assert!(msg.contains("ingest"), "{msg}");
    assert!(!msg.contains("unknown memory subcommand"), "{msg}");
}

/// The default embedded driver advertises every family, so nothing changes.
#[test]
fn default_embedded_driver_gates_nothing() {
    for (sub, _) in SUBCOMMAND_CONTROLLER {
        assert!(
            capability_verdict(
                "tinycortex",
                Capabilities::all(),
                required_capability(sub),
                "openhuman memory <sub>",
            )
            .is_ok(),
            "`openhuman memory {sub}` must stay available under the default driver"
        );
    }
}

/// The subcommands still resolved through [`create_memory_client`], and so
/// still subject to the legacy-client gate. The rest reach the bound driver
/// through the contract, where "not the embedded engine" is not a refusal
/// reason — see [`create_memory_binding`].
const LEGACY_ENGINE_SUBCOMMANDS: &[&str] = &["ingest", "query"];

/// Every legacy subcommand — gated or not — must be rejected under a null
/// binding: they operate on the embedded store directly, and the null
/// driver is not that engine. This is the regression the reviewer flagged:
/// `openhuman memory clear` used to open the embedded DB even with
/// `driver = "null"` (and now does not open it at all).
#[test]
fn null_driver_rejects_every_legacy_subcommand() {
    for sub in LEGACY_ENGINE_SUBCOMMANDS {
        let err = crate::core::cli_capability::legacy_client_verdict(
            "null",
            DriverClass::Null,
            &format!("openhuman memory {sub}"),
        )
        .expect_err("a null binding must reject legacy subcommands");
        let msg = err.to_string();
        assert!(msg.contains("null"), "{msg}");
        assert!(
            msg.contains("local store"),
            "refusal must explain that these subcommands read the local store: {msg}"
        );
    }
}

/// Both local-store classes may serve the legacy subcommands.
///
/// `Module` matters more than `Embedded` now: `binding::admit` refuses
/// `Embedded` outright, so the built-in driver binds as `Module` and a gate
/// that accepted only `Embedded` refused every subcommand in the field.
#[test]
fn local_store_drivers_serve_every_legacy_subcommand() {
    for (driver, class) in [
        ("tinycortex", DriverClass::Embedded),
        ("tinymemory", DriverClass::Module),
    ] {
        for sub in LEGACY_ENGINE_SUBCOMMANDS {
            assert!(
                crate::core::cli_capability::legacy_client_verdict(
                    driver,
                    class,
                    &format!("openhuman memory {sub}"),
                )
                .is_ok(),
                "`openhuman memory {sub}` must stay available under {driver} ({class:?})"
            );
        }
    }
}

/// A driver that answers from somewhere else must still be refused —
/// reading the local store there would answer from the wrong place.
#[test]
fn external_driver_still_rejects_every_legacy_subcommand() {
    for sub in LEGACY_ENGINE_SUBCOMMANDS {
        assert!(
            crate::core::cli_capability::legacy_client_verdict(
                "supermemory",
                DriverClass::External,
                &format!("openhuman memory {sub}"),
            )
            .is_err(),
            "`openhuman memory {sub}` must stay refused under a remote driver"
        );
    }
}

/// The legacy-client diagnostic must not leak credentials or endpoints.
#[test]
fn legacy_message_never_contains_a_credential_or_endpoint() {
    use crate::core::subsystem::DriverClass;
    let msg = crate::core::cli_capability::legacy_client_unavailable_message(
        "supermemory",
        DriverClass::External,
        "openhuman memory clear",
    );
    assert!(!msg.contains("keychain:"), "{msg}");
    assert!(!msg.contains("api.supermemory.ai"), "{msg}");
    assert!(
        msg.starts_with(crate::core::cli_capability::LEGACY_CLIENT_UNAVAILABLE_PREFIX),
        "{msg}"
    );
}
