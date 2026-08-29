//! Host layer over the engine's people domain.
//!
//! The domain itself lives in the engine crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::people::…` path resolving.
//!
//! # Why this names `tinycortex` rather than `tinymemory_core` (#5560)
//!
//! It used to read `pub use tinymemory_core::people::*;`, and that path was
//! itself a re-export: `tinymemory_core::people` is
//! `pub use crate::engine::backend::people::{address_book, migrations,
//! resolver, scorer, store, types};`, and `engine::backend::people` is
//! `pub use tinycortex::memory::people`. Both spellings therefore resolve to
//! the **same six items**; naming the engine crate directly changes no item,
//! only which crate alias holds them in the build.
//!
//! That matters because `tinymemory-core` is what #5560 is removing from the
//! production dependency graph, while `tinycortex` stays — it is a direct
//! dependency of this crate (`Cargo.toml`), it is where the memory engine
//! actually lives, and ~40 files here already name `tinycortex::memory::…`.
//! Same precedent as the `Memory` / `MemoryCategory` type re-exports in
//! `memory/mod.rs`, which were repointed at the contract for the same reason.
//!
//! **The `contacts` gate has to follow.** `address_book`'s macOS reader is
//! `#[cfg(all(target_os = "macos", feature = "contacts"))]` *inside tinycortex*,
//! and this crate reaches it today through `contacts = ["tinymemory-core/contacts"]`
//! → `tinymemory-core`'s `contacts = ["tinycortex/contacts"]`. When the
//! `tinymemory-core` normal dependency is dropped, that forward has to become
//! `contacts = ["tinycortex/contacts"]` directly, or the gate test below stops
//! testing anything.

pub use tinycortex::memory::people::*;

pub mod rpc;
pub mod schemas;

// The controller aggregators this domain's RPC surface defines. Aliased
// exactly as the pre-extraction module exported them.
pub use schemas::{
    all_controller_schemas as all_people_controller_schemas,
    all_registered_controllers as all_people_registered_controllers,
};

#[cfg(test)]
mod schemas_tests;

#[cfg(test)]
mod contacts_gate_tests {
    /// The `contacts` gate must reach the engine, not stop at this crate.
    ///
    /// The macOS address-book reader lives in the memory engine, several crates
    /// below this one, behind `#[cfg(all(target_os = "macos", feature =
    /// "contacts"))]`. This crate's `contacts` feature once enabled four
    /// `objc2` crates *locally* — none of which any file in `src/` names — and
    /// never forwarded, so the reader was always compiled out. Nothing failed:
    /// `refresh_address_book` returned success having seeded zero contacts, and
    /// the only visible symptom was an address book that stayed empty.
    ///
    /// So this asserts the property that was actually missing — that turning
    /// the feature on *here* changes what the reader does *there*. A build with
    /// `contacts` on, on macOS, must reach the real `CNContactStore` arm; the
    /// stub returns `Ok(vec![])` unconditionally, and the real arm cannot,
    /// because it can fail on permission.
    ///
    /// Deliberately not a `cfg!(feature = ...)` self-assertion: that would pass
    /// while the forward is broken, which is the entire bug.
    #[test]
    #[cfg(all(target_os = "macos", feature = "contacts"))]
    fn contacts_feature_reaches_the_engine_reader() {
        use super::address_book::{AddressBookError, ContactsSource, SystemContactsSource};

        // The stub arm returns Ok(vec![]) and can never report a permission
        // failure. Reaching a `PermissionDenied` — or real contacts — proves the
        // macOS arm compiled in. On a CI box with no Contacts authorisation the
        // permission error is the expected outcome.
        match SystemContactsSource.fetch_contacts() {
            Err(AddressBookError::PermissionDenied) => {}
            Ok(_) => {}
            Err(other) => panic!("address book read failed unexpectedly: {other:?}"),
        }
    }

    /// Off macOS the gate is a documented no-op, and the stub is correct.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn contacts_gate_is_a_no_op_off_macos() {
        use super::address_book::{ContactsSource, SystemContactsSource};

        assert_eq!(
            SystemContactsSource
                .fetch_contacts()
                .expect("stub never fails"),
            vec![],
            "off macOS the reader must be the empty stub"
        );
    }
}
