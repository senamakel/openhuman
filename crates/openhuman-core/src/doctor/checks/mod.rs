//! Native doctor checks for locally configured modules and services.

mod mcp;
mod modules;
mod sandbox;
mod skills;

use std::sync::Arc;

use super::contract::{DoctorCheck, DoctorCtx};

/// Constructs checks from the configuration selected for this report.
pub(super) fn for_context(ctx: &DoctorCtx) -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks = modules::for_context(ctx);
    checks.extend(mcp::for_context(ctx));
    checks.extend(skills::for_context(ctx));
    checks.extend(sandbox::for_context(ctx));
    checks
}

/// Encodes an arbitrary configured name as one valid, reversible check-id
/// segment. Lowercase ASCII letters, digits, and dashes remain readable;
/// every other UTF-8 byte is escaped, including underscores, so the encoding
/// cannot collide with an already escaped name.
pub(super) fn canonical_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' | b'-' => encoded.push(char::from(*byte)),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(encoded, "_{byte:02x}");
            }
        }
    }
    if encoded.is_empty() {
        "__empty".to_owned()
    } else {
        encoded
    }
}
