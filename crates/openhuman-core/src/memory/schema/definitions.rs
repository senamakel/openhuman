//! Schema definitions for every `memory_tree` JSON-RPC method.
//!
//! The [`schemas`] function is the single source of truth for each
//! controller's input/output field descriptions. Handlers delegate to
//! [`super::handlers`]; the registry lists are in [`super::registry`].

use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

pub(crate) const NAMESPACE: &str = "memory_tree";

/// Lookup the [`ControllerSchema`] for a single `memory_tree` function name.
#[path = "tree_operations_schema.rs"]
mod tree_operations_schema;
#[path = "vault_and_pipeline_schema.rs"]
mod vault_and_pipeline_schema;

pub fn schemas(function: &str) -> ControllerSchema {
    if let Some(schema) = tree_operations_schema::lookup(function) {
        return schema;
    }
    if let Some(schema) = vault_and_pipeline_schema::lookup(function) {
        return schema;
    }
    ControllerSchema {
        namespace: NAMESPACE,
        function: "unknown",
        description: "Unknown memory_tree controller function.",
        inputs: vec![FieldSchema {
            name: "function",
            ty: TypeSchema::String,
            comment: "Unknown function requested for schema lookup.",
            required: true,
        }],
        outputs: vec![FieldSchema {
            name: "error",
            ty: TypeSchema::String,
            comment: "Lookup error details.",
            required: true,
        }],
    }
}
