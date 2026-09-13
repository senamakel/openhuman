//! Controller schemas for the learning domain.

mod cache_helpers;
mod facet_handlers;
mod profile_handlers;

use crate::core::all::RegisteredController;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};

use cache_helpers::{facet_to_json, forget_facet_log, full_key, get_cache};
use facet_handlers::{
    handle_forget_facet, handle_get_facet, handle_list_facets, handle_pin_facet,
    handle_reset_cache, handle_unpin_facet, handle_update_facet,
};
use profile_handlers::{
    handle_cache_stats, handle_linkedin_enrichment, handle_rebuild_cache, handle_save_profile,
};

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;

pub fn all_learning_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        learning_schemas("learning_linkedin_enrichment"),
        learning_schemas("learning_save_profile"),
        learning_schemas("learning_rebuild_cache"),
        learning_schemas("learning_cache_stats"),
        learning_schemas("learning_list_facets"),
        learning_schemas("learning_get_facet"),
        learning_schemas("learning_update_facet"),
        learning_schemas("learning_pin_facet"),
        learning_schemas("learning_unpin_facet"),
        learning_schemas("learning_forget_facet"),
        learning_schemas("learning_reset_cache"),
    ]
}

pub fn all_learning_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: learning_schemas("learning_linkedin_enrichment"),
            handler: handle_linkedin_enrichment,
        },
        RegisteredController {
            schema: learning_schemas("learning_save_profile"),
            handler: handle_save_profile,
        },
        RegisteredController {
            schema: learning_schemas("learning_rebuild_cache"),
            handler: handle_rebuild_cache,
        },
        RegisteredController {
            schema: learning_schemas("learning_cache_stats"),
            handler: handle_cache_stats,
        },
        RegisteredController {
            schema: learning_schemas("learning_list_facets"),
            handler: handle_list_facets,
        },
        RegisteredController {
            schema: learning_schemas("learning_get_facet"),
            handler: handle_get_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_update_facet"),
            handler: handle_update_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_pin_facet"),
            handler: handle_pin_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_unpin_facet"),
            handler: handle_unpin_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_forget_facet"),
            handler: handle_forget_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_reset_cache"),
            handler: handle_reset_cache,
        },
    ]
}

pub fn learning_schemas(function: &str) -> ControllerSchema {
    match function {
        "learning_linkedin_enrichment" => ControllerSchema {
            namespace: "learning",
            function: "linkedin_enrichment",
            description: "Search Gmail for LinkedIn profile URLs, scrape the profile via Apify, \
                          and persist the result to memory. Runs the full enrichment pipeline.",
            inputs: vec![FieldSchema {
                name: "profile_url",
                ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                comment: "Pre-found LinkedIn profile URL (skips the Gmail-search stage). \
                          The frontend supplies this when it has already located the URL via \
                          the webview-driven `gmail_find_linkedin_profile_url` Tauri command.",
                required: false,
            }],
            outputs: vec![
                FieldSchema {
                    name: "profile_url",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "LinkedIn profile URL found in Gmail, if any.",
                    required: false,
                },
                FieldSchema {
                    name: "profile_data",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Scraped LinkedIn profile JSON from Apify, if successful.",
                    required: false,
                },
                FieldSchema {
                    name: "log",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Human-readable log of each pipeline stage.",
                    required: true,
                },
            ],
        },
        "learning_save_profile" => ControllerSchema {
            namespace: "learning",
            function: "save_profile",
            description: "Persist a markdown profile to `{workspace_dir}/PROFILE.md`. \
                          When `summarize=true`, runs the body through the LLM compressor \
                          first (same prompt as the LinkedIn-enrichment pipeline) so callers \
                          can hand in raw scraped material and get the same end-state.",
            inputs: vec![
                FieldSchema {
                    name: "markdown",
                    ty: TypeSchema::String,
                    comment: "Markdown body to persist (or to summarize first).",
                    required: true,
                },
                FieldSchema {
                    name: "summarize",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "Compress through LLM before writing (default false).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "path",
                    ty: TypeSchema::String,
                    comment: "Absolute path of the written PROFILE.md.",
                    required: true,
                },
                FieldSchema {
                    name: "bytes",
                    ty: TypeSchema::U64,
                    comment: "Bytes written.",
                    required: true,
                },
            ],
        },
        "learning_rebuild_cache" => ControllerSchema {
            namespace: "learning",
            function: "rebuild_cache",
            description: "Manually trigger a stability-detector rebuild cycle. \
                          Drains the candidate buffer, scores all (class, key) pairs, \
                          applies class budgets, and persists the updated ambient cache. \
                          Returns rebuild statistics.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "added",
                    ty: TypeSchema::U64,
                    comment: "Facet rows newly created in this cycle.",
                    required: true,
                },
                FieldSchema {
                    name: "evicted",
                    ty: TypeSchema::U64,
                    comment: "Facet rows demoted to Dropped or deleted.",
                    required: true,
                },
                FieldSchema {
                    name: "kept",
                    ty: TypeSchema::U64,
                    comment: "Facet rows carried over unchanged.",
                    required: true,
                },
                FieldSchema {
                    name: "total_size",
                    ty: TypeSchema::U64,
                    comment: "Total Active rows after the rebuild.",
                    required: true,
                },
            ],
        },
        "learning_cache_stats" => ControllerSchema {
            namespace: "learning",
            function: "cache_stats",
            description: "Return current ambient cache statistics — total row count, \
                          per-state breakdown, and per-class breakdown.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "total",
                    ty: TypeSchema::U64,
                    comment: "Total rows in the cache (all states).",
                    required: true,
                },
                FieldSchema {
                    name: "active",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=active.",
                    required: true,
                },
                FieldSchema {
                    name: "provisional",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=provisional.",
                    required: true,
                },
                FieldSchema {
                    name: "candidate",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=candidate.",
                    required: true,
                },
                FieldSchema {
                    name: "dropped",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=dropped.",
                    required: true,
                },
                FieldSchema {
                    name: "by_class",
                    ty: TypeSchema::Json,
                    comment: "Map of class name → row count (active rows only).",
                    required: true,
                },
            ],
        },
        "learning_list_facets" => ControllerSchema {
            namespace: "learning",
            function: "list_facets",
            description: "List all facets in the ambient cache (active + provisional). \
                          Optionally filter by class.",
            inputs: vec![FieldSchema {
                name: "class",
                ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                comment:
                    "Optional class filter: style | identity | tooling | veto | goal | channel.",
                required: false,
            }],
            outputs: vec![
                FieldSchema {
                    name: "facets",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment:
                        "Array of facet objects with key, value, state, user_state, stability.",
                    required: true,
                },
                FieldSchema {
                    name: "count",
                    ty: TypeSchema::U64,
                    comment: "Total number of facets returned.",
                    required: true,
                },
            ],
        },
        "learning_get_facet" => ControllerSchema {
            namespace: "learning",
            function: "get_facet",
            description:
                "Fetch a single facet by class and key suffix (e.g. class=style, key=verbosity).",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class: style | identity | tooling | veto | goal | channel.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment:
                        "Key suffix within the class (e.g. \"verbosity\" for style/verbosity).",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "facet",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "The facet object, or null if not found.",
                    required: false,
                },
                FieldSchema {
                    name: "found",
                    ty: TypeSchema::Bool,
                    comment: "Whether the facet was found.",
                    required: true,
                },
            ],
        },
        "learning_update_facet" => ControllerSchema {
            namespace: "learning",
            function: "update_facet",
            description: "Update the value of an existing facet and pin it (user_state=Pinned). \
                          Returns the updated facet.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class: style | identity | tooling | veto | goal | channel.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
                FieldSchema {
                    name: "value",
                    ty: TypeSchema::String,
                    comment: "New value to set.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_pin_facet" => ControllerSchema {
            namespace: "learning",
            function: "pin_facet",
            description:
                "Pin a facet (user_state=Pinned): locks Active regardless of stability score.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_unpin_facet" => ControllerSchema {
            namespace: "learning",
            function: "unpin_facet",
            description:
                "Unpin a facet (user_state=Auto): returns stability management to the detector.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_forget_facet" => ControllerSchema {
            namespace: "learning",
            function: "forget_facet",
            description:
                "Forget a facet (user_state=Forgotten): locks Dropped and blocks re-promotion.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                comment: "The facet in its Dropped state, or null if it didn't exist.",
                required: false,
            }],
        },
        "learning_reset_cache" => ControllerSchema {
            namespace: "learning",
            function: "reset_cache",
            description: "Reset the ambient cache: delete all Auto rows, preserve Pinned rows. \
                          The next rebuild repopulates from the substrate.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "deleted",
                    ty: TypeSchema::U64,
                    comment: "Number of Auto rows deleted.",
                    required: true,
                },
                FieldSchema {
                    name: "pinned_preserved",
                    ty: TypeSchema::U64,
                    comment: "Number of Pinned rows kept.",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "learning",
            function: "unknown",
            description: "Unknown learning controller.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}
