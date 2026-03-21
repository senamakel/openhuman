//! Tauri commands for the TinyHumans memory layer.

use std::sync::{Arc, Mutex};

use crate::memory::{MemoryClient, MemoryClientRef};

/// App-state slot for the memory client.
/// Starts as `None`; populated by `init_memory_client` when the frontend
/// provides the user's JWT token from `authSlice.token`.
pub struct MemoryState(pub Mutex<Option<MemoryClientRef>>);

/// Called by the frontend with the JWT from `authSlice.token`.
/// (Re-)initialises the TinyHumans memory client for the current session.
#[tauri::command]
pub async fn init_memory_client(
    jwt_token: String,
    state: tauri::State<'_, MemoryState>,
) -> Result<(), String> {
    log::info!("[memory] init_memory_client: entry (token_present={})", !jwt_token.trim().is_empty());
    let client = MemoryClient::from_token(jwt_token).map(Arc::new);
    if client.is_none() {
        log::warn!("[memory] init_memory_client: exit — empty token, memory layer disabled");
    } else {
        log::info!("[memory] init_memory_client: exit — client ready");
    }
    *state.0.lock().map_err(|e| e.to_string())? = client;
    Ok(())
}

/// Recall context from the TinyHumans Master memory node for a skill integration.
/// Returns the recalled context string (or null if the server had nothing to return).
#[tauri::command]
pub async fn recall_memory(
    skill_id: String,
    integration_id: String,
    max_chunks: Option<u32>,
    state: tauri::State<'_, MemoryState>,
) -> Result<Option<String>, String> {
    log::info!(
        "[memory] recall_memory: entry (skill_id={skill_id}, integration_id={integration_id}, max_chunks={max_chunks:?})"
    );
    let client = state.0.lock().map_err(|e| e.to_string())?.clone();
    match client {
        Some(c) => {
            let result = c
                .recall_skill_context(&skill_id, &integration_id, max_chunks.unwrap_or(10))
                .await;
            match &result {
                Ok(ctx) => log::info!(
                    "[memory] recall_memory: exit — ok (has_context={})",
                    ctx.is_some()
                ),
                Err(e) => log::warn!("[memory] recall_memory: exit — error: {e}"),
            }
            result
        }
        None => {
            log::warn!("[memory] recall_memory: exit — client not initialised (no JWT set)");
            Err("Memory layer not configured — JWT token not yet set".into())
        }
    }
}

/// Query the TinyHumans memory for a skill integration.
/// Returns the RAG context string to inject into AI prompts.
#[tauri::command]
pub async fn memory_query(
    skill_id: String,
    integration_id: String,
    query: String,
    max_chunks: Option<u32>,
    state: tauri::State<'_, MemoryState>,
) -> Result<String, String> {
    log::info!("[memory] memory_query: entry (skill_id={skill_id}, integration_id={integration_id}, max_chunks={max_chunks:?})");
    let client = state.0.lock().map_err(|e| e.to_string())?.clone();
    match client {
        Some(c) => {
            let result = c
                .query_skill_context(&skill_id, &integration_id, &query, max_chunks.unwrap_or(10))
                .await;
            match &result {
                Ok(ctx) => log::info!("[memory] memory_query: exit — ok (context_len={})", ctx.len()),
                Err(e) => log::warn!("[memory] memory_query: exit — error: {e}"),
            }
            result
        }
        None => {
            log::warn!("[memory] memory_query: exit — client not initialised (no JWT set)");
            Err("Memory layer not configured — JWT token not yet set".into())
        }
    }
}
