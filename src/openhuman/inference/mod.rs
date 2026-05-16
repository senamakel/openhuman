//! External inference domain.
//!
//! This module is the canonical controller surface for text / vision /
//! embedding inference. The underlying implementation still reuses the
//! existing local-runtime service during the migration away from the
//! `local_ai` catch-all namespace.

pub mod ops;
mod schemas;

pub use ops as rpc;
pub use schemas::{
    all_controller_schemas as all_inference_controller_schemas,
    all_registered_controllers as all_inference_registered_controllers,
};
