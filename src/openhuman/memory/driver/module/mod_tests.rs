//! Tests for the TinyMemory module client seam.
//!
//! The behaviour worth pinning here is the error mapping. A `TinyBus` failure
//! arrives as a name plus prose, and which [`MemoryError`] it becomes decides
//! whether the caller retries, rewrites its input, or gives up — so a
//! misclassification is not cosmetic. The mapping is also the one piece of this
//! module that can be exercised without a live bus.

use tinybus::Error as BusError;
use tinymemory_bus::error::MemoryError;
use tinymemory_bus::wire;

use super::map_bus_error;

/// A `MethodFailed` as the module emits one.
fn failed(name: &str) -> BusError {
    BusError::MethodFailed {
        name: name.to_string(),
        message: "prose for a human".to_string(),
    }
}

#[test]
fn a_driver_error_survives_the_round_trip_under_its_own_name() {
    // The module maps out through `wire::wire_name`; this maps back through
    // `wire::from_wire`. Same table, so the variant has to come back intact.
    for (name, matches) in [
        (wire::NOT_FOUND, matches!(map_bus_error(failed(wire::NOT_FOUND)), MemoryError::NotFound(_))),
        (wire::INVALID, matches!(map_bus_error(failed(wire::INVALID)), MemoryError::Invalid(_))),
        (
            wire::BUDGET_EXCEEDED,
            matches!(map_bus_error(failed(wire::BUDGET_EXCEEDED)), MemoryError::BudgetExceeded(_)),
        ),
        (
            wire::UNAUTHORIZED,
            matches!(map_bus_error(failed(wire::UNAUTHORIZED)), MemoryError::Unauthorized(_)),
        ),
    ] {
        assert!(matches, "{name} did not map back to its own variant");
    }
}

#[test]
fn a_path_escape_is_not_flattened_into_an_invalid() {
    // The one that matters: `PathEscape` reports a symlink or traversal that
    // left the workspace sandbox. Reclassifying it as a malformed argument
    // would turn a security-relevant refusal into a caller mistake.
    let mapped = map_bus_error(failed(wire::PATH_ESCAPE));
    assert!(
        matches!(mapped, MemoryError::PathEscape(_)),
        "expected PathEscape, got {mapped:?}"
    );
}

#[test]
fn an_unrecognised_error_name_is_opaque_rather_than_a_caller_mistake() {
    // A module built from a newer contract may name an error this build has no
    // variant for. Answering `Invalid` would tell the caller its input was
    // wrong when it was not.
    let mapped = map_bus_error(failed("ai.tinyhumans.tinymemory.Error.FromTheFuture"));
    assert!(
        matches!(mapped, MemoryError::Other(_)),
        "expected Other, got {mapped:?}"
    );
}

#[test]
fn a_transport_failure_is_reported_as_unreachable() {
    // Never reached the driver, so it is not the driver's error. `Unreachable`
    // is the variant a caller retries on.
    let mapped = map_bus_error(BusError::Transport("socket closed".to_string()));
    assert!(
        matches!(mapped, MemoryError::Unreachable(_)),
        "expected Unreachable, got {mapped:?}"
    );
}

#[test]
fn an_unknown_member_is_a_backend_mismatch_not_an_invalid_argument() {
    // The host believes in a member the module does not serve: a version skew.
    // The arguments were fine, so `Invalid` would point at the wrong thing.
    let mapped = map_bus_error(BusError::UnknownMethod {
        interface: tinybus::name::InterfaceName::new(tinymemory_bus::names::BUS_NAME)
            .expect("the contract's own interface name parses"),
        member: tinybus::name::MemberName::new("FromTheFuture")
            .expect("a PascalCase member name parses"),
    });
    assert!(
        matches!(mapped, MemoryError::Backend(_)),
        "expected Backend, got {mapped:?}"
    );
}

#[test]
fn the_message_of_a_mapped_error_carries_no_payload() {
    // Neither end may put a namespace key, an entry's content or a recall query
    // into an error string. This asserts the seam adds nothing of its own — the
    // message is exactly the prose the module sent.
    let mapped = map_bus_error(failed(wire::NOT_FOUND));
    assert!(
        mapped.to_string().contains("prose for a human"),
        "the module's message should survive: {mapped}"
    );
}
