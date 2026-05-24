//! Derived graph edge shape.

use serde::{Deserialize, Serialize};

/// A derived co-occurrence edge between two entities.
///
/// Not a triple in the classical sense — there's no explicit predicate. The
/// `weight` field is the count of distinct nodes the pair has both appeared
/// on, which serves as a cheap proxy for relationship strength.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub subject: String,
    pub object: String,
    pub weight: u32,
}
