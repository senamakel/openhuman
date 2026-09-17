//! Controller schema and registered-controller lists for the `todos`
//! namespace.

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;

use super::handlers::{
    handle_add, handle_clear, handle_decide_plan, handle_edit, handle_list, handle_reclaim_stale,
    handle_remove, handle_replace, handle_revise_plan, handle_run_get, handle_run_list,
    handle_set_session_thread, handle_update_status,
};
use super::schema_defs::schemas;

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("add"),
        schemas("edit"),
        schemas("update_status"),
        schemas("set_session_thread"),
        schemas("decide_plan"),
        schemas("revise_plan"),
        schemas("remove"),
        schemas("replace"),
        schemas("clear"),
        schemas("run_list"),
        schemas("run_get"),
        schemas("reclaim_stale"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: handle_list,
        },
        RegisteredController {
            schema: schemas("add"),
            handler: handle_add,
        },
        RegisteredController {
            schema: schemas("edit"),
            handler: handle_edit,
        },
        RegisteredController {
            schema: schemas("update_status"),
            handler: handle_update_status,
        },
        RegisteredController {
            schema: schemas("set_session_thread"),
            handler: handle_set_session_thread,
        },
        RegisteredController {
            schema: schemas("decide_plan"),
            handler: handle_decide_plan,
        },
        RegisteredController {
            schema: schemas("revise_plan"),
            handler: handle_revise_plan,
        },
        RegisteredController {
            schema: schemas("remove"),
            handler: handle_remove,
        },
        RegisteredController {
            schema: schemas("replace"),
            handler: handle_replace,
        },
        RegisteredController {
            schema: schemas("clear"),
            handler: handle_clear,
        },
        RegisteredController {
            schema: schemas("run_list"),
            handler: handle_run_list,
        },
        RegisteredController {
            schema: schemas("run_get"),
            handler: handle_run_get,
        },
        RegisteredController {
            schema: schemas("reclaim_stale"),
            handler: handle_reclaim_stale,
        },
    ]
}
