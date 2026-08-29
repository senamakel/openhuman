//! The compiled-in pack table.
//!
//! Membership is a build-time decision, deliberately: a pack that config or RPC
//! could edit would let a caller move a dangerous tool out of the advertised
//! surface (or back into it) without review. Adding a pack is a source change.

use super::types::ToolPack;

/// Every pack this build knows about.
///
/// Chosen by measured schema cost against how often the orchestrator actually
/// needs them: together ~5.9k tokens of the Master Agent's tool-schema budget,
/// idle in the large majority of turns. Measured with tiktoken `o200k_base`
/// against a real `agent dump-all`, not estimated.
///
/// Frequency of use is the whole criterion — see
/// `DELIBERATELY_UNPACKED_FLEET_TOOLS` below for the family that is expensive
/// but must stay advertised.
pub const PACKS: &[ToolPack] = &[
    ToolPack {
        id: "workflows",
        summary: "Build, discover, run and inspect saved automation workflows (flows) and their run logs.",
        tools: &[
            "build_workflow",
            "discover_workflows",
            "run_workflow",
            "await_workflow",
            "describe_workflow",
            "list_workflows",
            "list_workflow_runs",
            "read_workflow_run_log",
            // Flow authoring and inspection. Owned by `workflow_builder` /
            // `flow_discovery`; any other agent that lists them reaches them
            // through `use_skill`.
            "propose_workflow",
            "revise_workflow",
            "edit_workflow",
            "validate_workflow",
            "save_workflow",
            "create_workflow",
            "duplicate_flow",
            "dry_run_workflow",
            "list_flows",
            "get_flow",
            "get_flow_history",
            "get_flow_run",
            "list_flow_runs",
            "list_flow_connections",
            "cancel_flow_run",
            "resume_flow_run",
            "suggest_workflows",
            "search_tool_catalog",
            "get_tool_contract",
            "get_tool_output_sample",
            "list_node_kinds",
            "get_node_kind_contract",
            "list_agent_profiles",
            "list_connectable_toolkits",
        ],
        owners: &["workflow_builder", "flow_discovery"],
    },
    ToolPack {
        id: "crypto",
        summary: "Crypto wallet and market actions: balances, transfers, swaps, bridges, contract calls and x402 paid requests.",
        tools: &[
            "do_crypto",
            "wallet_status",
            "wallet_balances",
            "wallet_network_defaults",
            "wallet_supported_assets",
            "wallet_chain_status",
            "wallet_encode_erc20_transfer",
            "wallet_prepare_transfer",
            "wallet_execute_prepared",
            "wallet_tx_status",
            "wallet_tx_receipt",
            "wallet_lookup_tx",
            "web3_swap_routes",
            "web3_swap_quote",
            "web3_swap_execute",
            "web3_bridge_quote",
            "web3_bridge_execute",
            "web3_dapp_call",
            "web3_dapp_execute",
            "x402_request",
        ],
        owners: &["crypto_agent"],
    },
    ToolPack {
        id: "integrations",
        summary: "MCP server setup, connection status, and calling tools on a connected MCP server.",
        tools: &[
            "use_mcp_server",
            "setup_mcp_server",
            "mcp_registry_status",
            "mcp_registry_search",
            "mcp_registry_get",
            "mcp_registry_installed_list",
            "mcp_registry_list_tools",
            "mcp_registry_connect",
            "mcp_registry_disconnect",
            "mcp_registry_tool_call",
            "mcp_registry_config_assist",
            "mcp_registry_install",
            "mcp_registry_uninstall",
        ],
        owners: &["mcp_agent", "mcp_setup", "planner"],
    },
    ToolPack {
        id: "composio",
        summary: "Connect and use third-party Composio toolkits: list connections and toolkits, raise a connect card, list and execute a toolkit's actions.",
        tools: &[
            "composio",
            "composio_authorize",
            "composio_connect",
            "composio_execute",
            "composio_list_connections",
            "composio_list_toolkits",
            "composio_list_tools",
        ],
        owners: &["integrations_agent", "workflow_builder", "planner"],
    },
    ToolPack {
        id: "skills",
        summary: "Find, install and run agent skills from the community registries.",
        tools: &[
            "run_skill",
            "setup_skills",
            "skill_registry_browse",
            "skill_registry_search",
            "skill_registry_install",
            "skill_registry_sources",
            "skill_registry_uninstall",
            "skill_runtime_resolve_runtimes",
            "install_workflow_from_url",
            "uninstall_workflow",
            "read_workflow_resource",
        ],
        owners: &[
            "skill_setup",
            "skill_executor",
            "skill_creator",
            "context_scout",
        ],
    },
    ToolPack {
        id: "documents",
        summary: "Generate a .docx document or a .pptx presentation as a workspace artifact.",
        tools: &["generate_document", "generate_presentation"],
        owners: &["presentation_agent"],
    },
    ToolPack {
        id: "audio",
        summary: "Generate a spoken podcast from text and optionally email the audio.",
        tools: &[
            "audio_generate_podcast",
            "audio_email_podcast",
            "audio_generate_and_email_podcast",
        ],
        owners: &[],
    },
    ToolPack {
        id: "system",
        summary: "OpenHuman's own health, diagnostics, cost dashboard, service lifecycle, proxy and read-only config.",
        tools: &[
            "config_snapshot",
            "config_get_client_config",
            "config_get_autonomy",
            "config_get_search",
            "config_get_runtime_flags",
            "config_resolve_api_url",
            "config_get_data_paths",
            "doctor_health",
            "doctor_models",
            "health_snapshot",
            "health_system_info",
            "dashboard_model_health",
            "cost_get_dashboard",
            "cost_get_daily_history",
            "cost_get_summary",
            "security_policy_info",
            "service_status",
            "service_start",
            "service_stop",
            "service_restart",
            "service_shutdown",
            "service_install",
            "service_uninstall",
            "daemon_host_prefs_get",
            "daemon_host_prefs_set",
            "proxy_config",
        ],
        owners: &["settings_agent"],
    },
    ToolPack {
        id: "goals",
        summary: "Read, set and complete the user's long-term goals.",
        tools: &["goal_set", "goal_get", "goal_complete"],
        owners: &[],
    },
    ToolPack {
        id: "app_update",
        summary: "Check for and apply OpenHuman application updates.",
        tools: &["update_check", "update_apply"],
        owners: &["settings_agent"],
    },
];

/// The fleet tools are deliberately NOT a pack, and this is worth stating
/// because they look like an obvious 1.6k-token candidate.
///
/// `steer_subagent`, `wait_subagent`, `close_subagent`, `list_subagents`,
/// `continue_subagent`, `wait`, `wait_loop` and `spawn_parallel_agents` are
/// needed *reactively*, mid-turn — exactly when an async worker returns or
/// pauses on `ask_user_clarification`. A load round-trip at that moment is the
/// worst possible time to add one, and a `continue_subagent` the model cannot
/// see is the known infinite-re-delegation failure mode (#4291): the only
/// continuation left is a fresh stateless sub-agent that asks the same
/// question again.
#[cfg(test)]
pub(crate) const DELIBERATELY_UNPACKED_FLEET_TOOLS: &[&str] = &[
    "steer_subagent",
    "wait_subagent",
    "close_subagent",
    "list_subagents",
    "continue_subagent",
    "wait",
    "wait_loop",
    "spawn_parallel_agents",
];

pub fn pack(id: &str) -> Option<&'static ToolPack> {
    PACKS.iter().find(|p| p.id == id)
}

/// The pack owning `tool`, if any.
pub fn pack_for_tool(tool: &str) -> Option<&'static ToolPack> {
    PACKS.iter().find(|p| p.owns(tool))
}

/// Every packed tool name across all packs.
pub fn all_packed_tool_names() -> Vec<&'static str> {
    PACKS.iter().flat_map(|p| p.tools.iter().copied()).collect()
}

/// Every packed tool name that applies to `agent_id`.
///
/// A pack is skipped entirely for the specialist that owns its family — see
/// [`ToolPack::owners`]. The orchestrator owns no pack, so it sees the full
/// withholding.
pub fn packed_tool_names_for_agent(agent_id: &str) -> Vec<&'static str> {
    PACKS
        .iter()
        .filter(|p| !p.is_owner(agent_id))
        .flat_map(|p| p.tools.iter().copied())
        .collect()
}

/// The always-on index: one line per pack, rendered into `load_skill`'s own
/// description so the model can pick a pack without a round trip.
pub fn pack_index_markdown() -> String {
    let mut out = String::new();
    for p in PACKS {
        out.push_str(&format!("- `{}` — {}\n", p.id, p.summary));
    }
    out
}
