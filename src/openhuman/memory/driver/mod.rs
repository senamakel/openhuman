//! Memory-driver implementations of the [`tinycortex_api`] contract.
//!
//! One subdirectory per driver. [`embedded`] wraps the in-process tinycortex
//! engine, and [`module`] talks to TinyMemory when it is loaded as a `TinyBus`
//! module instead of compiled in — plus the reference `NullMemoryProvider` that
//! ships inside the contract crate itself.
//!
//! The two are not interchangeable yet: [`embedded`] implements the
//! `tinycortex_api` contract this build pins, while [`module`] speaks the
//! `tinymemory-bus` vocabulary the loadable module was built against. They
//! converge when the TinyCortex pin moves to a revision that re-exports
//! `tinymemory-api` — until then [`module`] is the client seam, not a
//! `MemoryProvider` impl.
//!
//! Drivers live *under* `memory/` rather than in a sibling top-level directory
//! so the "one directory equals one feature gate" family rule holds: a memory
//! driver is memory, and gating it separately from the domain it implements
//! would be meaningless.

pub mod embedded;
pub mod module;
