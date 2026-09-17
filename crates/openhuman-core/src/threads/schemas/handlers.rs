//! Thin JSON-RPC handlers that parse params and delegate to [`super::super::ops`].

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::agent::task_board::{TaskBoard, TaskBoardCard, TaskBoardStore};
use crate::core::all::ControllerFuture;
use crate::memory::{
    AppendConversationMessageRequest, ConversationMessagesRequest, CreateConversationThreadRequest,
    DeleteConversationThreadRequest, EmptyRequest, GenerateConversationThreadTitleRequest,
    UpdateConversationMessageRequest, UpdateConversationThreadLabelsRequest,
    UpdateConversationThreadTitleRequest, UpsertConversationThreadRequest,
};
use crate::threads::turn_state::{
    ClearTurnStateRequest, GetTurnStateForRequestRequest, GetTurnStateRequest,
};

use super::super::ops;

pub(super) fn handle_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::threads_list(EmptyRequest {}).await?) })
}

pub(super) fn handle_upsert(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<UpsertConversationThreadRequest>(params)?;
        to_json(ops::thread_upsert(p).await?)
    })
}

pub(super) fn handle_create_new(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<CreateConversationThreadRequest>(params)?;
        to_json(ops::thread_create_new(p).await?)
    })
}

pub(super) fn handle_messages_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ConversationMessagesRequest>(params)?;
        to_json(ops::messages_list(p).await?)
    })
}

pub(super) fn handle_message_append(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<AppendConversationMessageRequest>(params)?;
        to_json(ops::message_append(p).await?)
    })
}

pub(super) fn handle_generate_title(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<GenerateConversationThreadTitleRequest>(params)?;
        to_json(ops::thread_generate_title(p).await?)
    })
}

pub(super) fn handle_update_labels(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<UpdateConversationThreadLabelsRequest>(params)?;
        to_json(ops::thread_update_labels(p).await?)
    })
}

pub(super) fn handle_update_title(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<UpdateConversationThreadTitleRequest>(params)?;
        to_json(ops::thread_update_title(p).await?)
    })
}

pub(super) fn handle_message_update(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<UpdateConversationMessageRequest>(params)?;
        to_json(ops::message_update(p).await?)
    })
}

pub(super) fn handle_delete(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<DeleteConversationThreadRequest>(params)?;
        to_json(ops::thread_delete(p).await?)
    })
}

pub(super) fn handle_purge(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::threads_purge(EmptyRequest {}).await?) })
}

pub(super) fn handle_turn_state_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<GetTurnStateRequest>(params)?;
        to_json(ops::turn_state_get(p).await?)
    })
}

pub(super) fn handle_turn_state_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(ops::turn_state_list(EmptyRequest {}).await?) })
}

pub(super) fn handle_turn_state_history(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<GetTurnStateRequest>(params)?;
        to_json(ops::turn_state_history(p).await?)
    })
}

pub(super) fn handle_turn_state_get_turn(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<GetTurnStateForRequestRequest>(params)?;
        to_json(ops::turn_state_get_turn(p).await?)
    })
}

pub(super) fn handle_turn_state_clear(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ClearTurnStateRequest>(params)?;
        to_json(ops::turn_state_clear(p).await?)
    })
}

#[derive(serde::Deserialize)]
struct TaskBoardGetParams {
    thread_id: String,
}

#[derive(serde::Deserialize)]
struct TaskBoardPutParams {
    thread_id: String,
    cards: Vec<TaskBoardCard>,
}

pub(super) fn handle_task_board_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<TaskBoardGetParams>(params)?;
        let thread_id = p.thread_id.trim().to_string();
        tracing::debug!(thread_id = %thread_id, "[rpc][task_board] get entry");
        let config = crate::config::Config::load_or_init().await.map_err(|e| {
            tracing::debug!(
                thread_id = %thread_id,
                error = %e,
                "[rpc][task_board] get load_config_error"
            );
            format!("load config: {e}")
        })?;
        tracing::trace!(thread_id = %thread_id, "[rpc][task_board] get loading_board");
        let board = crate::agent::task_board::board_for_thread(&config.workspace_dir, &thread_id)
            .await
            .map_err(|e| {
                tracing::debug!(
                    thread_id = %thread_id,
                    error = %e,
                    "[rpc][task_board] get board_error"
                );
                e
            })?;
        tracing::debug!(
            thread_id = %thread_id,
            card_count = board.cards.len(),
            "[rpc][task_board] get exit"
        );
        Ok(serde_json::json!({ "taskBoard": board }))
    })
}

pub(super) fn handle_task_board_put(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<TaskBoardPutParams>(params)?;
        let thread_id = p.thread_id.trim().to_string();
        tracing::debug!(
            thread_id = %thread_id,
            card_count = p.cards.len(),
            "[rpc][task_board] put entry"
        );
        let config = crate::config::Config::load_or_init().await.map_err(|e| {
            tracing::debug!(
                thread_id = %thread_id,
                error = %e,
                "[rpc][task_board] put load_config_error"
            );
            format!("load config: {e}")
        })?;
        let board = TaskBoard {
            thread_id: thread_id.clone(),
            cards: p.cards,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let saved = TaskBoardStore::new(config.workspace_dir)
            .put(board)
            .await
            .map_err(|e| {
                tracing::debug!(
                    thread_id = %thread_id,
                    error = %e,
                    "[rpc][task_board] put store_error"
                );
                e
            })?;
        tracing::debug!(
            thread_id = %thread_id,
            card_count = saved.cards.len(),
            "[rpc][task_board] put exit"
        );
        Ok(serde_json::json!({ "taskBoard": saved }))
    })
}

pub(super) fn handle_token_usage(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ops::ThreadTokenUsageRequest>(params)?;
        to_json(ops::token_usage(p).await?)
    })
}

pub(super) fn handle_transcript_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let p = parse::<ops::TranscriptGetRequest>(params)?;
        to_json(ops::transcript_get(p).await?)
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

pub(super) fn parse<T: DeserializeOwned>(params: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(params)).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: crate::rpc::RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
