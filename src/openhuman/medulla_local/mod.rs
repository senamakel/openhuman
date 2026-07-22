//! Local Medulla brain — supervises a `medulla-serve` Node child and speaks the
//! medulla-serve NDJSON protocol, v1, as the host.
//!
//! This is Flavor A of the Medulla flavors plan (§3.1–§3.2): openhuman runs
//! medulla-v1's agent-harness facade in a supervised child and answers its port
//! callbacks against the local capability surface (inference routing, curated
//! read-only tools). It also backs the subconscious-replacement draft (§5.2):
//! with `subconscious.engine = "medulla"`, each heartbeat tick routes its
//! observe/reflect/commit cycle through one `instruct` instead of the local
//! tinyagents graph. The default (`local`) is untouched.
//!
//! Module shape (canonical `mod/types/…/ops/schemas`):
//! * [`protocol`]   — wire frame envelopes (`ready`/`req`/`res`/`call`/`ret`/`event`).
//! * [`types`]      — handshake / instruct / status / inference domain types.
//! * [`ports`]      — the [`HostPorts`] seam serve's reverse-RPC drives.
//! * [`server`]     — the supervisor: spawn, handshake, id-correlated NDJSON,
//!                    restart-and-retry-once, stderr drain (mirrors
//!                    `runtime_python_server/server.rs`).
//! * [`host_ports`] — the concrete openhuman [`HostPorts`] (inference + tools).
//! * [`ops`]        — RPC handlers + the subconscious tick entrypoint.
//! * [`schemas`]    — the `medulla_local` controller schemas.

pub mod host_ports;
pub mod ops;
pub mod ports;
pub mod protocol;
pub mod schemas;
pub mod server;
pub mod types;

pub use ports::{HostPorts, PortError};
pub use schemas::{
    all_controller_schemas as all_medulla_local_controller_schemas,
    all_registered_controllers as all_medulla_local_registered_controllers,
};
pub use server::{ensure_started, MedullaSupervisor};
pub use types::{InferenceCall, InferenceResult, InstructReceipt, MedullaLocalStatus};
