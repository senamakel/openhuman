//! Shared helpers for the facet controllers: cache access, key construction,
//! JSON serialisation, and the `forget_facet` log line.

/// Build a [`FacetCache`] from the bound memory driver, or return a string
/// error.
///
/// Async since facets moved behind the module: there is no process-global
/// memory client to ask any more.
pub(super) async fn get_cache() -> Result<crate::agent::learning::cache::FacetCache, String> {
    let guard = crate::memory::ops::guard::active_memory_guard()
        .await
        .map_err(|e| format!("memory unavailable: {e}"))?;
    Ok(crate::agent::learning::cache::FacetCache::new(guard))
}

/// Build the full facet key from class string + key suffix.
/// E.g. (`"style"`, `"verbosity"`) → `"style/verbosity"`.
pub(super) fn full_key(class_str: &str, key_suffix: &str) -> String {
    format!("{class_str}/{key_suffix}")
}

/// Serialize a [`ProfileFacet`] to a serde_json [`Value`] for RPC output.
pub(super) fn facet_to_json(f: &tinymemory_api::provider::ProfileFacet) -> serde_json::Value {
    serde_json::json!({
        "key": f.key,
        "value": f.value,
        "state": f.state.as_str(),
        "user_state": f.user_state.as_str(),
        "stability": f.stability,
        "confidence": f.confidence,
        "evidence_count": f.evidence_count,
        "first_seen_at": f.first_seen_at,
        "last_seen_at": f.last_seen_at,
        "class": f.class,
        // Provenance the store already persists and row_to_facet hydrates, but
        // the serializer previously dropped: the citations behind the facet and
        // the per-cue-family evidence counts. `None`/empty serialize to
        // `null`/`[]`.
        "cue_families": f.cue_families,
        "evidence_refs": f.evidence_refs,
    })
}

/// The log line `learning.forget_facet` emits, given whether a row was actually
/// written.
///
/// Split out so the claim can be unit-tested without a cache: the defect this
/// replaces built the "state=dropped user_state=forgotten" line unconditionally,
/// *before* the read told it whether there was anything to drop, so an absent key
/// produced a log asserting a state change that never happened (#6108).
pub(super) fn forget_facet_log(full_key: &str, dropped: bool) -> Vec<String> {
    vec![if dropped {
        format!("learning.forget_facet: key={full_key} state=dropped user_state=forgotten")
    } else {
        format!("learning.forget_facet: key={full_key} not present — no change")
    }]
}
