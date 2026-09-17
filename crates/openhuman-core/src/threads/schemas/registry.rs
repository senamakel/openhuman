//! Controller schema and registered-controller lists for the `threads`
//! namespace.

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;

use super::handlers::{
    handle_create_new, handle_delete, handle_generate_title, handle_list, handle_message_append,
    handle_message_update, handle_messages_list, handle_purge, handle_task_board_get,
    handle_task_board_put, handle_token_usage, handle_transcript_get, handle_turn_state_clear,
    handle_turn_state_get, handle_turn_state_get_turn, handle_turn_state_history,
    handle_turn_state_list, handle_update_labels, handle_update_title, handle_upsert,
};
use super::schema_defs::schemas;

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("upsert"),
        schemas("create_new"),
        schemas("messages_list"),
        schemas("message_append"),
        schemas("generate_title"),
        schemas("update_labels"),
        schemas("update_title"),
        schemas("message_update"),
        schemas("delete"),
        schemas("purge"),
        schemas("turn_state_get"),
        schemas("turn_state_list"),
        schemas("turn_state_history"),
        schemas("turn_state_get_turn"),
        schemas("turn_state_clear"),
        schemas("task_board_get"),
        schemas("task_board_put"),
        schemas("token_usage"),
        schemas("transcript_get"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("upsert"),
            handler: handle_upsert,
        },
        RegisteredController {
            schema: schemas("create_new"),
            handler: handle_create_new,
        },
        RegisteredController {
            schema: schemas("messages_list"),
            handler: handle_messages_list,
        },
        RegisteredController {
            schema: schemas("message_append"),
            handler: handle_message_append,
        },
        RegisteredController {
            schema: schemas("generate_title"),
            handler: handle_generate_title,
        },
        RegisteredController {
            schema: schemas("update_labels"),
            handler: handle_update_labels,
        },
        RegisteredController {
            schema: schemas("update_title"),
            handler: handle_update_title,
        },
        RegisteredController {
            schema: schemas("message_update"),
            handler: handle_message_update,
        },
        RegisteredController {
            schema: schemas("delete"),
            handler: handle_delete,
        },
        RegisteredController {
            schema: schemas("purge"),
            handler: handle_purge,
        },
        RegisteredController {
            schema: schemas("turn_state_get"),
            handler: handle_turn_state_get,
        },
        RegisteredController {
            schema: schemas("turn_state_list"),
            handler: handle_turn_state_list,
        },
        RegisteredController {
            schema: schemas("turn_state_history"),
            handler: handle_turn_state_history,
        },
        RegisteredController {
            schema: schemas("turn_state_get_turn"),
            handler: handle_turn_state_get_turn,
        },
        RegisteredController {
            schema: schemas("turn_state_clear"),
            handler: handle_turn_state_clear,
        },
        RegisteredController {
            schema: schemas("task_board_get"),
            handler: handle_task_board_get,
        },
        RegisteredController {
            schema: schemas("task_board_put"),
            handler: handle_task_board_put,
        },
        RegisteredController {
            schema: schemas("token_usage"),
            handler: handle_token_usage,
        },
        RegisteredController {
            schema: schemas("transcript_get"),
            handler: handle_transcript_get,
        },
    ]
}
