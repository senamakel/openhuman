//! RPC handler functions for `openhuman.skill_registry_*` controllers.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::openhuman::skill_registry::ops;
use crate::openhuman::skill_registry::types::RegistrySource;
use crate::rpc::RpcOutcome;

use super::wire_types::{
    AddSourceParams, BrowseParams, BrowseResult, InstallParams, InstallResult, RemoveSourceParams,
    SearchParams, SearchResult, SourcesResult,
};

fn deserialize_params<T: serde::de::DeserializeOwned>(
    params: Map<String, Value>,
) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}

pub(super) fn handle_browse(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<BrowseParams>(params)?;
        tracing::debug!(
            force_refresh = p.force_refresh,
            "[skill_registry][rpc] browse"
        );
        let entries = ops::browse_catalog(p.force_refresh).await?;
        tracing::debug!(count = entries.len(), "[skill_registry][rpc] browse result");
        to_json(RpcOutcome::new(BrowseResult { entries }, Vec::new()))
    })
}

pub(super) fn handle_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<SearchParams>(params)?;
        tracing::debug!(
            query = %p.query,
            format = ?p.format,
            source = ?p.source,
            "[skill_registry][rpc] search"
        );
        let entries =
            ops::search_catalog(&p.query, p.format.as_deref(), p.source.as_deref()).await?;
        tracing::debug!(count = entries.len(), "[skill_registry][rpc] search result");
        to_json(RpcOutcome::new(SearchResult { entries }, Vec::new()))
    })
}

pub(super) fn handle_sources(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _ = params;
        let sources = ops::list_sources();
        to_json(RpcOutcome::new(SourcesResult { sources }, Vec::new()))
    })
}

pub(super) fn handle_add_source(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<AddSourceParams>(params)?;
        tracing::info!(id = %p.id, url = %p.url, "[skill_registry][rpc] add_source");
        let source = RegistrySource {
            id: p.id,
            name: p.name,
            url: p.url,
            kind: p.kind,
            enabled: true,
        };
        ops::add_source(source)?;
        let sources = ops::list_sources();
        to_json(RpcOutcome::new(SourcesResult { sources }, Vec::new()))
    })
}

pub(super) fn handle_remove_source(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<RemoveSourceParams>(params)?;
        tracing::info!(id = %p.id, "[skill_registry][rpc] remove_source");
        ops::remove_source(&p.id)?;
        let sources = ops::list_sources();
        to_json(RpcOutcome::new(SourcesResult { sources }, Vec::new()))
    })
}

pub(super) fn handle_install(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = deserialize_params::<InstallParams>(params)?;
        tracing::info!(
            entry_id = %p.entry_id,
            source_id = %p.source_id,
            "[skill_registry][rpc] install"
        );

        let catalog = ops::browse_catalog(false).await?;
        let entry = catalog
            .iter()
            .find(|e| e.id == p.entry_id && e.source_id == p.source_id)
            .ok_or_else(|| {
                format!(
                    "entry '{}' not found in source '{}'",
                    p.entry_id, p.source_id
                )
            })?;

        let workspace = crate::openhuman::workflows::schemas::resolve_workspace_dir().await;
        let outcome = ops::install_from_catalog(&workspace, entry).await?;

        to_json(RpcOutcome::new(
            InstallResult {
                url: outcome.url,
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                new_skills: outcome.new_skills,
            },
            Vec::new(),
        ))
    })
}
