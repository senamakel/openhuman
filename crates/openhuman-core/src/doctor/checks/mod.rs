//! Native checks for configured channels, integrations, voice, search, and wallet.

use std::sync::Arc;

use crate::doctor::{DoctorCheck, DoctorCtx};

mod channels;
mod integrations;
mod search;
mod voice;
mod wallet;

pub(super) fn for_context(ctx: &DoctorCtx) -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks = Vec::new();
    checks.extend(channels::for_context(ctx));
    checks.extend(integrations::for_context());
    checks.extend(voice::for_context());
    checks.extend(search::for_context(ctx));
    checks.extend(wallet::for_context());
    checks
}
