//! Handlers for the profile-level learning controllers: LinkedIn enrichment,
//! `PROFILE.md` saving, and facet-cache rebuild / statistics.

use serde_json::{Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;
use crate::rpc::RpcOutcome;

pub(super) fn handle_linkedin_enrichment(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let preset_profile_url = params
            .get("profile_url")
            .and_then(Value::as_str)
            .map(str::to_string);
        let config = config_rpc::load_config_with_timeout().await?;
        let result = crate::agent::learning::linkedin_enrichment::run_linkedin_enrichment(
            &config,
            preset_profile_url,
        )
        .await
        .map_err(|e| format!("linkedin enrichment failed: {e:#}"))?;

        let payload = serde_json::json!({
            "profile_url": result.profile_url,
            "profile_data": result.profile_data,
            "stages": result.stages,
            "log": result.log,
        });

        RpcOutcome::new(payload, result.log.clone()).into_cli_compatible_json()
    })
}

pub(super) fn handle_save_profile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let markdown = params
            .get("markdown")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "missing required `markdown`".to_string())?;
        let summarize = params
            .get("summarize")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let config = config_rpc::load_config_with_timeout().await?;

        let body = if summarize {
            crate::agent::learning::linkedin_enrichment::summarise_profile_with_llm(
                &config, &markdown,
            )
            .await
            .map_err(|e| format!("LLM summarisation failed: {e:#}"))?
        } else {
            markdown
        };

        let path = config.workspace_dir.join("PROFILE.md");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create workspace dir failed: {e}"))?;
        }
        tokio::fs::write(&path, &body)
            .await
            .map_err(|e| format!("write PROFILE.md failed: {e}"))?;

        let bytes = body.len();
        let path_display = path.display().to_string();
        let payload = serde_json::json!({
            "path": path_display,
            "bytes": bytes,
        });
        let log = vec![format!(
            "learning.save_profile: wrote {bytes} bytes to {path_display} (summarize={summarize})"
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn handle_rebuild_cache(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::agent::learning::cache::FacetCache;
        use crate::agent::learning::stability_detector::StabilityDetector;
        use std::time::{SystemTime, UNIX_EPOCH};

        tracing::debug!("[learning.rebuild_cache] manual rebuild requested via RPC");

        let cache = FacetCache::new(
            crate::memory::ops::guard::active_memory_guard()
                .await
                .map_err(|e| format!("memory unavailable: {e}"))?,
        );
        let detector = StabilityDetector::new(cache);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let outcome = detector
            .rebuild(now)
            .await
            .map_err(|e| format!("rebuild failed: {e:#}"))?;

        let log = vec![format!(
            "learning.rebuild_cache: added={} evicted={} kept={} total={}",
            outcome.added, outcome.evicted, outcome.kept, outcome.total_size,
        )];

        let payload = serde_json::json!({
            "added": outcome.added,
            "evicted": outcome.evicted,
            "kept": outcome.kept,
            "total_size": outcome.total_size,
        });

        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

pub(super) fn handle_cache_stats(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::agent::learning::cache::FacetCache;
        use tinymemory_api::provider::FacetState;

        tracing::debug!("[learning.cache_stats] cache stats requested via RPC");

        let cache = FacetCache::new(
            crate::memory::ops::guard::active_memory_guard()
                .await
                .map_err(|e| format!("memory unavailable: {e}"))?,
        );

        let all_facets = cache
            .list_all()
            .await
            .map_err(|e| format!("list_all failed: {e:#}"))?;

        let total = all_facets.len();
        let active = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Active)
            .count();
        let provisional = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Provisional)
            .count();
        let candidate = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Candidate)
            .count();
        let dropped = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Dropped)
            .count();

        // Per-class count (Active rows only, keyed by class field or key prefix).
        let mut by_class: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for f in all_facets.iter().filter(|f| f.state == FacetState::Active) {
            let cls = f
                .class
                .clone()
                .or_else(|| f.key.split_once('/').map(|(p, _)| p.to_string()))
                .unwrap_or_else(|| "_other".to_string());
            *by_class.entry(cls).or_insert(0) += 1;
        }

        let log = vec![format!(
            "learning.cache_stats: total={total} active={active} provisional={provisional} \
             candidate={candidate} dropped={dropped}"
        )];

        let payload = serde_json::json!({
            "total": total,
            "active": active,
            "provisional": provisional,
            "candidate": candidate,
            "dropped": dropped,
            "by_class": by_class,
        });

        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}
