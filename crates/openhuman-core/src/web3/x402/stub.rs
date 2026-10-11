//! Disabled-x402 facade.
//!
//! Compiled only when the `web3` Cargo feature is OFF (see the gate in
//! [`super`]). Only three entry points have always-on callers: `init_ledger`
//! (`core/runtime/bootstrap.rs` boot, itself runtime-gated on `DomainGroup::Web3`) and
//! the controller-registration pair (`core/all.rs`). The `X402RequestTool`
//! registration and the http_request 402-retry path are `#[cfg(feature =
//! "web3")]` at their call sites, so no other x402 surface is referenced when
//! off.
//!
//! Signatures MUST match the real ones exactly; the disabled build
//! (`cargo check --no-default-features`) is
//! the only thing that catches drift.

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;

/// No x402 controller schemas when the domain is compiled out.
pub fn all_x402_controller_schemas() -> Vec<ControllerSchema> {
    Vec::new()
}

/// No x402 controllers are registered when the domain is compiled out — the
/// `openhuman.x402_*` RPCs become unknown-method.
pub fn all_x402_registered_controllers() -> Vec<RegisteredController> {
    Vec::new()
}

// Compiled only in the disabled build (`#[cfg(not(feature = "web3"))] mod stub;`
// in `super`), so a plain `#[cfg(test)]` here runs only when x402 is compiled
// out — it pins the disabled facade's callable no-op + empty-registration
// contract that `core/runtime/bootstrap.rs` boot and `core/all.rs` rely on.
#[cfg(test)]
#[path = "stub_tests.rs"]
mod tests;
