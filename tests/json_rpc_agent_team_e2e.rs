//! JSON-RPC E2E cases for the agent team namespace.

#[path = "support/rpc_e2e_harness.rs"]
mod harness;
use harness::*;

#[tokio::test]
async fn json_rpc_agent_registry_manages_defaults_and_custom_agents() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_guard = EnvVarGuard::unset("VITE_BACKEND_URL");

    write_min_config(&openhuman_home, "http://127.0.0.1:9");

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let list = post_json_rpc(
        &rpc_base,
        28_621,
        "openhuman.agent_registry_list",
        json!({ "include_disabled": true }),
    )
    .await;
    let list_result = assert_no_jsonrpc_error(&list, "agent_registry_list");
    let agents = list_result
        .get("agents")
        .and_then(Value::as_array)
        .expect("agent registry list should return agents array");
    let orchestrator = agents
        .iter()
        .find(|agent| agent.get("id").and_then(Value::as_str) == Some("orchestrator"))
        .expect("default registry should include orchestrator");
    assert_eq!(
        orchestrator.get("source").and_then(Value::as_str),
        Some("default")
    );
    assert_eq!(
        orchestrator.get("enabled").and_then(Value::as_bool),
        Some(true)
    );

    let definitions = post_json_rpc(
        &rpc_base,
        286_211,
        "openhuman.agent_list_definitions",
        json!({}),
    )
    .await;
    let definitions_result = assert_no_jsonrpc_error(&definitions, "agent_list_definitions");
    let definitions = definitions_result
        .get("definitions")
        .and_then(Value::as_array)
        .expect("agent_list_definitions should return definitions array");
    // The v1 `agent_memory` built-in was removed with the v1 memory system
    // (Memory v2 is one `memory` tool, not a sub-agent); `critic` is the
    // stand-in safe-library entry.
    assert!(
        !definitions
            .iter()
            .any(|definition| definition.get("id").and_then(Value::as_str) == Some("agent_memory")),
        "the v1 memory agent must stay removed"
    );
    let memory_agent = definitions
        .iter()
        .find(|definition| definition.get("id").and_then(Value::as_str) == Some("critic"))
        .expect("safe agent library should include critic");
    assert!(memory_agent
        .get("display_name")
        .and_then(Value::as_str)
        .is_some());
    assert!(memory_agent
        .get("when_to_use")
        .and_then(Value::as_str)
        .is_some());
    assert!(memory_agent.get("system_prompt").is_none());
    assert!(memory_agent.get("tools").is_some());
    assert!(memory_agent
        .get("direct_tool_count")
        .and_then(Value::as_u64)
        .is_some());
    assert!(memory_agent
        .get("can_run_as_user_facing_worker")
        .and_then(Value::as_bool)
        .is_some());

    let missing = post_json_rpc(
        &rpc_base,
        286_210,
        "openhuman.agent_registry_get",
        json!({ "id": "does_not_exist" }),
    )
    .await;
    assert!(
        assert_no_jsonrpc_error(&missing, "agent_registry_get missing")["agent"].is_null(),
        "missing agents should return agent:null"
    );

    let update_default = post_json_rpc(
        &rpc_base,
        286_211,
        "openhuman.agent_registry_update",
        json!({
            "id": "critic",
            "name": "Review Specialist",
            "description": "Workspace-specific review specialist.",
            "model": "hint:reasoning",
            "tool_allowlist": ["tools.web_search", "memory.recall"],
            "tool_denylist": ["wallet.execute_prepared"],
            "tags": ["memory", "workspace"],
            "metadata": { "pinned_by": "json_rpc_e2e" }
        }),
    )
    .await;
    let update_default_agent =
        assert_no_jsonrpc_error(&update_default, "agent_registry_update default")
            .get("agent")
            .expect("update default should return agent");
    assert_eq!(
        update_default_agent.get("name").and_then(Value::as_str),
        Some("Review Specialist")
    );
    assert_eq!(
        update_default_agent
            .get("metadata")
            .and_then(|metadata| metadata.get("pinned_by"))
            .and_then(Value::as_str),
        Some("json_rpc_e2e")
    );

    let update_missing = post_json_rpc(
        &rpc_base,
        286_212,
        "openhuman.agent_registry_update",
        json!({ "id": "missing_agent", "enabled": false }),
    )
    .await;
    let update_missing_error =
        assert_jsonrpc_error(&update_missing, "agent_registry_update missing");
    assert!(
        update_missing_error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("not found"),
        "unexpected missing-update error: {update_missing_error}"
    );

    let disabled = post_json_rpc(
        &rpc_base,
        28_622,
        "openhuman.agent_registry_set_enabled",
        json!({ "id": "image_agent", "enabled": false }),
    )
    .await;
    let disabled_result = assert_no_jsonrpc_error(&disabled, "agent_registry_set_enabled");
    assert_eq!(
        disabled_result
            .get("agent")
            .and_then(|agent| agent.get("id"))
            .and_then(Value::as_str),
        Some("image_agent")
    );
    assert_eq!(
        disabled_result
            .get("agent")
            .and_then(|agent| agent.get("enabled"))
            .and_then(Value::as_bool),
        Some(false)
    );

    let visible = post_json_rpc(
        &rpc_base,
        28_623,
        "openhuman.agent_registry_list",
        json!({}),
    )
    .await;
    let visible_result = assert_no_jsonrpc_error(&visible, "agent_registry_list visible");
    let visible_agents = visible_result
        .get("agents")
        .and_then(Value::as_array)
        .expect("agent registry list should return visible agents array");
    assert!(
        !visible_agents
            .iter()
            .any(|agent| agent.get("id").and_then(Value::as_str) == Some("image_agent")),
        "disabled default agent should be hidden unless include_disabled=true"
    );

    let all_after_disable = post_json_rpc(
        &rpc_base,
        286_213,
        "openhuman.agent_registry_list",
        json!({ "include_disabled": true }),
    )
    .await;
    let all_after_disable_result =
        assert_no_jsonrpc_error(&all_after_disable, "agent_registry_list include disabled");
    let disabled_image_agent = all_after_disable_result
        .get("agents")
        .and_then(Value::as_array)
        .and_then(|agents| {
            agents
                .iter()
                .find(|agent| agent.get("id").and_then(Value::as_str) == Some("image_agent"))
        })
        .expect("include_disabled should retain disabled image_agent");
    assert_eq!(
        disabled_image_agent.get("enabled").and_then(Value::as_bool),
        Some(false)
    );

    let reenabled = post_json_rpc(
        &rpc_base,
        286_214,
        "openhuman.agent_registry_set_enabled",
        json!({ "id": "image_agent", "enabled": true }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&reenabled, "agent_registry_set_enabled reenable")
            .get("agent")
            .and_then(|agent| agent.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let disabled_orchestrator = post_json_rpc(
        &rpc_base,
        286_231,
        "openhuman.agent_registry_set_enabled",
        json!({ "id": "orchestrator", "enabled": false }),
    )
    .await;
    let disabled_orchestrator_error = assert_jsonrpc_error(
        &disabled_orchestrator,
        "agent_registry_set_enabled orchestrator",
    );
    assert!(
        disabled_orchestrator_error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("orchestrator agent cannot be disabled"),
        "unexpected orchestrator-disable error: {disabled_orchestrator_error}"
    );

    let update_orchestrator_disabled = post_json_rpc(
        &rpc_base,
        286_232,
        "openhuman.agent_registry_update",
        json!({ "id": "orchestrator", "enabled": false }),
    )
    .await;
    let update_orchestrator_error = assert_jsonrpc_error(
        &update_orchestrator_disabled,
        "agent_registry_update orchestrator disabled",
    );
    assert!(
        update_orchestrator_error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("orchestrator agent cannot be disabled"),
        "unexpected orchestrator-update error: {update_orchestrator_error}"
    );

    let created = post_json_rpc(
        &rpc_base,
        28_624,
        "openhuman.agent_registry_create_custom",
        json!({
            "id": "custom_writer",
            "name": "Custom Writer",
            "description": "Drafts polished workspace updates.",
            "model": "hint:reasoning",
            "system_prompt": "Write concise, accurate updates.",
            "tool_allowlist": ["memory.recall", "tools.web_search"],
            "tool_denylist": ["wallet.execute_prepared"],
            "tags": ["writing", "custom"],
            "metadata": { "created_by": "json_rpc_e2e" }
        }),
    )
    .await;
    let created_result = assert_no_jsonrpc_error(&created, "agent_registry_create_custom");
    let custom = created_result
        .get("agent")
        .expect("create_custom should return agent");
    assert_eq!(
        custom.get("id").and_then(Value::as_str),
        Some("custom_writer")
    );
    assert_eq!(custom.get("source").and_then(Value::as_str), Some("custom"));
    assert_eq!(custom.get("enabled").and_then(Value::as_bool), Some(true));
    assert_eq!(
        custom
            .get("tool_allowlist")
            .and_then(Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(Value::as_str),
        Some("memory.recall")
    );

    let get_custom = post_json_rpc(
        &rpc_base,
        28_625,
        "openhuman.agent_registry_get",
        json!({ "id": "custom_writer" }),
    )
    .await;
    let get_custom_result = assert_no_jsonrpc_error(&get_custom, "agent_registry_get custom");
    assert_eq!(
        get_custom_result
            .get("agent")
            .and_then(|agent| agent.get("metadata"))
            .and_then(|metadata| metadata.get("created_by"))
            .and_then(Value::as_str),
        Some("json_rpc_e2e")
    );

    let updated_custom = post_json_rpc(
        &rpc_base,
        286_215,
        "openhuman.agent_registry_update",
        json!({
            "id": "custom_writer",
            "name": "Custom Writer v2",
            "description": "Drafts polished workspace updates and summaries.",
            "enabled": false,
            "model": "hint:coding",
            "system_prompt": "Write concise updates with citations when available.",
            "tool_allowlist": ["memory.recall"],
            "tool_denylist": ["shell"],
            "subagents": ["critic"],
            "tags": ["writing", "custom", "disabled"],
            "metadata": { "updated_by": "json_rpc_e2e" }
        }),
    )
    .await;
    let updated_custom_agent =
        assert_no_jsonrpc_error(&updated_custom, "agent_registry_update custom")
            .get("agent")
            .expect("custom update should return agent");
    assert_eq!(
        updated_custom_agent.get("name").and_then(Value::as_str),
        Some("Custom Writer v2")
    );
    assert_eq!(
        updated_custom_agent.get("enabled").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        updated_custom_agent
            .get("subagents")
            .and_then(|subagents| subagents.get("allowlist"))
            .and_then(Value::as_array)
            .and_then(|allowlist| allowlist.first())
            .and_then(Value::as_str),
        Some("critic")
    );

    let reenabled_custom = post_json_rpc(
        &rpc_base,
        286_216,
        "openhuman.agent_registry_set_enabled",
        json!({ "id": "custom_writer", "enabled": true }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&reenabled_custom, "agent_registry_set_enabled custom")
            .get("agent")
            .and_then(|agent| agent.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let full_upsert = post_json_rpc(
        &rpc_base,
        286_217,
        "openhuman.agent_registry_upsert_custom",
        json!({
            "agent": {
                "id": "custom_reviewer",
                "name": "Custom Reviewer",
                "description": "Reviews agent plans before execution.",
                "source": "default",
                "enabled": false,
                "model": "hint:reasoning",
                "system_prompt": "Review plans for missing validation.",
                "tool_allowlist": ["memory.recall"],
                "tool_denylist": ["shell", "file_write"],
                "subagents": ["critic"],
                "tags": ["review"],
                "metadata": { "entry_shape": "full" }
            }
        }),
    )
    .await;
    let full_upsert_agent = assert_no_jsonrpc_error(&full_upsert, "agent_registry_upsert_custom")
        .get("agent")
        .expect("upsert_custom should return agent");
    assert_eq!(
        full_upsert_agent.get("id").and_then(Value::as_str),
        Some("custom_reviewer")
    );
    assert_eq!(
        full_upsert_agent.get("source").and_then(Value::as_str),
        Some("custom"),
        "upsert_custom should force source=custom even if caller sends another source"
    );
    assert_eq!(
        full_upsert_agent.get("enabled").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        full_upsert_agent
            .get("subagents")
            .and_then(|subagents| subagents.get("allowlist"))
            .and_then(Value::as_array)
            .and_then(|allowlist| allowlist.first())
            .and_then(Value::as_str),
        Some("critic")
    );

    let visible_after_custom_disable = post_json_rpc(
        &rpc_base,
        286_218,
        "openhuman.agent_registry_list",
        json!({}),
    )
    .await;
    let visible_after_custom_disable_result = assert_no_jsonrpc_error(
        &visible_after_custom_disable,
        "agent_registry_list hides disabled custom",
    );
    assert!(
        !visible_after_custom_disable_result
            .get("agents")
            .and_then(Value::as_array)
            .expect("agent_registry_list should return agents array")
            .iter()
            .any(|agent| agent.get("id").and_then(Value::as_str) == Some("custom_reviewer")),
        "disabled custom agent should be hidden from default list"
    );

    let default_collision = post_json_rpc(
        &rpc_base,
        28_626,
        "openhuman.agent_registry_create_custom",
        json!({
            "id": "orchestrator",
            "name": "Bad Override",
            "description": "Should not replace default agents through custom create."
        }),
    )
    .await;
    let collision_error = assert_jsonrpc_error(
        &default_collision,
        "agent_registry_create_custom default collision",
    );
    assert!(
        collision_error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("default agent"),
        "unexpected default-collision error: {collision_error}"
    );

    let removed_reviewer = post_json_rpc(
        &rpc_base,
        286_219,
        "openhuman.agent_registry_remove",
        json!({ "id": "custom_reviewer" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&removed_reviewer, "agent_registry_remove custom reviewer")
            .get("removed")
            .and_then(Value::as_bool),
        Some(true)
    );

    let removed_custom = post_json_rpc(
        &rpc_base,
        28_627,
        "openhuman.agent_registry_remove",
        json!({ "id": "custom_writer" }),
    )
    .await;
    let removed_custom_result =
        assert_no_jsonrpc_error(&removed_custom, "agent_registry_remove custom");
    assert_eq!(
        removed_custom_result
            .get("removed")
            .and_then(Value::as_bool),
        Some(true)
    );

    let removed_missing = post_json_rpc(
        &rpc_base,
        286_220,
        "openhuman.agent_registry_remove",
        json!({ "id": "missing_agent" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&removed_missing, "agent_registry_remove missing")
            .get("removed")
            .and_then(Value::as_bool),
        Some(false)
    );

    let reset_default = post_json_rpc(
        &rpc_base,
        286_221,
        "openhuman.agent_registry_remove",
        json!({ "id": "critic" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&reset_default, "agent_registry_remove default override")
            .get("removed")
            .and_then(Value::as_bool),
        Some(true)
    );

    let reset_image_agent = post_json_rpc(
        &rpc_base,
        286_222,
        "openhuman.agent_registry_remove",
        json!({ "id": "image_agent" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(
            &reset_image_agent,
            "agent_registry_remove image_agent override"
        )
        .get("removed")
        .and_then(Value::as_bool),
        Some(true)
    );

    let image_agent = post_json_rpc(
        &rpc_base,
        286_223,
        "openhuman.agent_registry_get",
        json!({ "id": "image_agent" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&image_agent, "agent_registry_get reset default")
            .get("agent")
            .and_then(|agent| agent.get("enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    // The agent editor's tool picker is fed by available_tools — every entry is
    // a {name, description} pair whose name is a valid tool_allowlist value.
    let available_tools = post_json_rpc(
        &rpc_base,
        286_224,
        "openhuman.agent_registry_available_tools",
        json!({}),
    )
    .await;
    let tools = assert_no_jsonrpc_error(&available_tools, "agent_registry_available_tools")
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .expect("available_tools should return a tools array");
    assert!(
        !tools.is_empty(),
        "the orchestrator should expose at least one built-in tool"
    );
    let first = tools.first().expect("non-empty tools");
    assert!(
        first.get("name").and_then(Value::as_str).is_some(),
        "each tool should have a string name: {first}"
    );
    assert!(
        first.get("description").and_then(Value::as_str).is_some(),
        "each tool should have a string description: {first}"
    );
    // The catalog is the full built-in surface (wildcard agent), not the
    // orchestrator's curated `named` subset — so a core read tool like
    // `file_read`, which the orchestrator does not list directly, must appear.
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    assert!(
        names.contains(&"file_read"),
        "available_tools should expose the full catalog (file_read missing): {names:?}"
    );

    rpc_join.abort();
}

#[test]
fn json_rpc_protocol_auth_and_agent_hello() {
    run_json_rpc_e2e_on_agent_stack(
        "json_rpc_protocol_auth_and_agent_hello",
        json_rpc_protocol_auth_and_agent_hello_inner,
    );
}

async fn json_rpc_protocol_auth_and_agent_hello_inner() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let _scripted_fifo_guard = ScriptedFifoGuard;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    // Always use the in-process Axum mock for /settings + /openai so this test does not pick up
    // BACKEND_URL/VITE_BACKEND_URL from the developer shell (e.g. mock-api that returns 401 for
    // the synthetic JWT used below).
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_guard = EnvVarGuard::unset("VITE_BACKEND_URL");

    let (mock_addr, mock_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let mock_origin = format!("http://{}", mock_addr);

    write_min_config(&openhuman_home, &mock_origin);

    // Pre-create the user-scoped config directory so that when store_session
    // activates user "e2e-user" and reloads config, it finds the correct
    // api_url and secrets.encrypt=false (rather than defaults).
    let user_scoped_dir = openhuman_home.join("users").join("e2e-user");
    write_min_config(&user_scoped_dir, &mock_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{}", rpc_addr);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // --- core.ping (baseline protocol) ---
    let ping = post_json_rpc(&rpc_base, 1, "core.ping", json!({})).await;
    let ping_result = assert_no_jsonrpc_error(&ping, "core.ping");
    assert_eq!(ping_result.get("ok"), Some(&json!(true)));

    // --- unknown method ---
    let unknown = post_json_rpc(&rpc_base, 2, "core.not_a_real_method", json!({})).await;
    assert!(
        unknown.get("error").is_some(),
        "expected error for unknown method: {unknown}"
    );

    // --- auth: session state (no JWT yet) ---
    let state_before = post_json_rpc(&rpc_base, 3, "openhuman.auth_get_state", json!({})).await;
    let state_outer = assert_no_jsonrpc_error(&state_before, "get_state");
    let state_body = state_outer.get("result").unwrap_or(state_outer);
    assert!(
        state_body.get("isAuthenticated").is_some() || state_body.get("is_authenticated").is_some(),
        "unexpected auth state shape: {state_body}"
    );

    // --- auth: store session (validates JWT via mock GET /auth/me) ---
    let store = post_json_rpc(
        &rpc_base,
        4,
        "openhuman.auth_store_session",
        json!({
            "token": "e2e-test-jwt",
            "user_id": "e2e-user"
        }),
    )
    .await;
    assert_no_jsonrpc_error(&store, "store_session");

    // --- agent: single chat turn (mock chat completions) ---
    let chat = post_json_rpc(
        &rpc_base,
        5,
        "openhuman.inference_agent_chat",
        json!({
            "message": "Hello",
        }),
    )
    .await;
    let chat_result = assert_no_jsonrpc_error(&chat, "agent_chat");
    let reply = extract_string_outcome(chat_result);
    assert!(
        reply.contains("e2e mock") || reply.contains("Hello"),
        "unexpected agent reply: {reply:?}"
    );

    // --- web channel RPC + SSE loop ---
    let client_id = "e2e-client-1";
    let thread_id = "thread-1";
    let events_url = format!("{}/events?client_id={}", rpc_base, client_id);
    let sse_task = tokio::spawn(async move { read_terminal_web_chat_event(&events_url).await });
    push_forced_chat_completion_when(
        "Hello from web channel",
        forced_text_completion("Hello from e2e mock agent"),
    );

    let web_chat = post_json_rpc(
        &rpc_base,
        6,
        "openhuman.channel_web_chat",
        json!({
            "client_id": client_id,
            "thread_id": thread_id,
            "message": "Hello from web channel",
            "model_override": "e2e-mock-model",
        }),
    )
    .await;
    let web_chat_result = assert_no_jsonrpc_error(&web_chat, "channel_web_chat");
    assert_eq!(
        web_chat_result
            .get("result")
            .and_then(|v| v.get("accepted")),
        Some(&json!(true))
    );

    let sse_event = sse_task.await.expect("sse task join should succeed");
    assert_eq!(
        sse_event.get("event").and_then(Value::as_str),
        Some("chat_done")
    );
    assert_eq!(
        sse_event.get("thread_id").and_then(Value::as_str),
        Some(thread_id)
    );
    assert_eq!(
        sse_event
            .pointer("/usage/context_tokens")
            .and_then(Value::as_u64),
        Some(20),
        "chat_done carries the final call's input plus output token count: {sse_event}"
    );
    assert!(
        !sse_event
            .get("full_response")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty(),
        "expected non-empty chat_done response payload: {sse_event}"
    );

    mock_join.abort();
    rpc_join.abort();
}

#[tokio::test]
async fn json_rpc_agent_work_list_groups_runs_by_bucket() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_url_guard = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _api_url_guard = EnvVarGuard::unset("OPENHUMAN_API_URL");

    let (api_addr, api_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let api_origin = format!("http://{api_addr}");
    write_min_config(openhuman_home.as_path(), &api_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let config = openhuman_core::config::Config::load_or_init()
        .await
        .expect("load config");

    use tinyagents_session::run_ledger::{AgentRunKind, AgentRunStatus, AgentRunUpsert};
    let seed = |id: &str, status: AgentRunStatus| AgentRunUpsert {
        id: id.to_string(),
        kind: AgentRunKind::Subagent,
        parent_run_id: None,
        parent_thread_id: Some("thread-work-1".to_string()),
        agent_id: Some("task_manager_agent".to_string()),
        status,
        prompt_ref: None,
        worker_thread_id: None,
        checkpoint_path: None,
        checkpoint: None,
        summary: None,
        error: None,
        metadata: json!({ "source": "json_rpc_e2e" }),
        started_at: None,
        completed_at: None,
    };
    // Two awaiting-user (needs_input), one running (working), one completed.
    openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = seed("work-a", AgentRunStatus::AwaitingUser);
        async move { ledger.upsert_agent_run(ledger_arg_0).await }
    })
    .expect("seed a");
    openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = seed("work-b", AgentRunStatus::AwaitingUser);
        async move { ledger.upsert_agent_run(ledger_arg_0).await }
    })
    .expect("seed b");
    openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = seed("work-c", AgentRunStatus::Running);
        async move { ledger.upsert_agent_run(ledger_arg_0).await }
    })
    .expect("seed c");
    openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = seed("work-d", AgentRunStatus::Completed);
        async move { ledger.upsert_agent_run(ledger_arg_0).await }
    })
    .expect("seed d");

    let list = post_json_rpc(&rpc_base, 9131, "openhuman.agent_work_list", json!({})).await;
    let outer = assert_no_jsonrpc_error(&list, "agent_work_list");

    assert_eq!(
        outer.get("total").and_then(serde_json::Value::as_u64),
        Some(4)
    );

    let groups = outer
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .expect("groups array");
    // Always five buckets, in display order.
    let buckets: Vec<&str> = groups
        .iter()
        .filter_map(|g| g.get("bucket").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(
        buckets,
        vec!["needs_input", "working", "completed", "failed", "stopped"]
    );

    let count_of = |bucket: &str| -> u64 {
        groups
            .iter()
            .find(|g| g.get("bucket").and_then(serde_json::Value::as_str) == Some(bucket))
            .and_then(|g| g.get("count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    assert_eq!(count_of("needs_input"), 2);
    assert_eq!(count_of("working"), 1);
    assert_eq!(count_of("completed"), 1);
    assert_eq!(count_of("failed"), 0);
    assert_eq!(count_of("stopped"), 0);

    api_join.abort();
    rpc_join.abort();
}

#[tokio::test]
async fn json_rpc_agent_team_coordination_roundtrip() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_url_guard = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _api_url_guard = EnvVarGuard::unset("OPENHUMAN_API_URL");

    let (api_addr, api_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let api_origin = format!("http://{api_addr}");
    write_min_config(openhuman_home.as_path(), &api_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let config = openhuman_core::config::Config::load_or_init()
        .await
        .expect("load config");

    // Create a team with two members.
    let created = post_json_rpc(
        &rpc_base,
        9341,
        "openhuman.agent_team_create",
        json!({
            "leadAgentId": "lead",
            "parentThreadId": "thread-team-e2e",
            "summary": "ship feature",
            "members": [
                { "name": "alice", "agentId": "orchestrator" },
                { "name": "bob", "agentId": "orchestrator" }
            ]
        }),
    )
    .await;
    let created_outer = assert_no_jsonrpc_error(&created, "agent_team_create");
    let team_id = created_outer
        .get("team")
        .and_then(|t| t.get("id"))
        .and_then(serde_json::Value::as_str)
        .expect("team id")
        .to_string();
    let members = created_outer
        .get("members")
        .and_then(serde_json::Value::as_array)
        .expect("members array");
    assert_eq!(members.len(), 2);
    let alice_id = members[0]
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("alice id")
        .to_string();
    let bob_id = members[1]
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("bob id")
        .to_string();

    // Assign task A, then task B that depends on A.
    let assign_a = post_json_rpc(
        &rpc_base,
        9342,
        "openhuman.agent_team_assign_task",
        json!({ "teamId": team_id, "title": "Task A", "dependsOn": [] }),
    )
    .await;
    let assign_a_outer = assert_no_jsonrpc_error(&assign_a, "agent_team_assign_task A");
    let task_a_id = assign_a_outer
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(serde_json::Value::as_str)
        .expect("task A id")
        .to_string();

    let assign_b = post_json_rpc(
        &rpc_base,
        9343,
        "openhuman.agent_team_assign_task",
        json!({ "teamId": team_id, "title": "Task B", "dependsOn": [task_a_id.clone()] }),
    )
    .await;
    let assign_b_outer = assert_no_jsonrpc_error(&assign_b, "agent_team_assign_task B");
    let task_b_id = assign_b_outer
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(serde_json::Value::as_str)
        .expect("task B id")
        .to_string();

    // Claim B while A is still todo → blocked on A.
    let claim_b_blocked = post_json_rpc(
        &rpc_base,
        9344,
        "openhuman.agent_team_claim_task",
        json!({
            "teamId": team_id,
            "taskId": task_b_id,
            "memberId": bob_id,
            "claimToken": "tok-b"
        }),
    )
    .await;
    let claim_b_blocked_outer =
        assert_no_jsonrpc_error(&claim_b_blocked, "agent_team_claim_task B blocked");
    assert_eq!(
        claim_b_blocked_outer
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("blocked")
    );

    // Claim A → ok.
    let claim_a = post_json_rpc(
        &rpc_base,
        9345,
        "openhuman.agent_team_claim_task",
        json!({
            "teamId": team_id,
            "taskId": task_a_id,
            "memberId": alice_id,
            "claimToken": "tok-a"
        }),
    )
    .await;
    let claim_a_outer = assert_no_jsonrpc_error(&claim_a, "agent_team_claim_task A");
    assert_eq!(
        claim_a_outer
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("claimed")
    );

    // Mark A done directly via the run ledger, then B claims fine.
    let task_a = openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = task_a_id.to_string();
        async move { ledger.get_agent_team_task(&ledger_arg_0).await }
    })
    .expect("get task A")
    .expect("task A present");
    openhuman_core::storage::sessions::with_ledger(&config.workspace_dir, |ledger| {
        let ledger_arg_0 = tinyagents_session::run_ledger::AgentTeamTaskUpsert {
            id: task_a.id.clone(),
            team_id: task_a.team_id.clone(),
            title: task_a.title.clone(),
            objective: task_a.objective.clone(),
            status: tinyagents_session::run_ledger::AgentTeamTaskStatus::Done,
            owner_member_id: task_a.owner_member_id.clone(),
            depends_on: task_a.depends_on.clone(),
            gate_status: Some(task_a.gate_status.clone()),
            gate_reason: task_a.gate_reason.clone(),
            evidence: task_a.evidence.clone(),
            source_run_id: task_a.source_run_id.clone(),
            order_index: task_a.order_index,
            created_at: Some(task_a.created_at),
        };
        async move { ledger.upsert_agent_team_task(ledger_arg_0).await }
    })
    .expect("mark A done");

    let claim_b_ok = post_json_rpc(
        &rpc_base,
        9346,
        "openhuman.agent_team_claim_task",
        json!({
            "teamId": team_id,
            "taskId": task_b_id,
            "memberId": bob_id,
            "claimToken": "tok-b2"
        }),
    )
    .await;
    let claim_b_ok_outer = assert_no_jsonrpc_error(&claim_b_ok, "agent_team_claim_task B ok");
    assert_eq!(
        claim_b_ok_outer
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("claimed")
    );

    // Complete B with no evidence but requireEvidence → quality gate fails.
    let complete_b_blocked = post_json_rpc(
        &rpc_base,
        9360,
        "openhuman.agent_team_complete_task",
        json!({
            "teamId": team_id,
            "taskId": task_b_id,
            "memberId": bob_id,
            "evidence": [],
            "requireEvidence": true
        }),
    )
    .await;
    let complete_b_blocked_outer =
        assert_no_jsonrpc_error(&complete_b_blocked, "agent_team_complete_task B gate-fail");
    assert_eq!(
        complete_b_blocked_outer
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("gateFailed")
    );

    // Complete B again with evidence → passes the gate, task is now done.
    let complete_b_ok = post_json_rpc(
        &rpc_base,
        9361,
        "openhuman.agent_team_complete_task",
        json!({
            "teamId": team_id,
            "taskId": task_b_id,
            "memberId": bob_id,
            "evidence": ["https://ci/run/42"],
            "requireEvidence": true
        }),
    )
    .await;
    let complete_b_ok_outer =
        assert_no_jsonrpc_error(&complete_b_ok, "agent_team_complete_task B done");
    assert_eq!(
        complete_b_ok_outer
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("completed")
    );

    // Message bob from alice (still live), then list messages. The shutdown
    // below must come after: a stopped member cannot send or receive.
    let message = post_json_rpc(
        &rpc_base,
        9347,
        "openhuman.agent_team_message_member",
        json!({
            "teamId": team_id,
            "fromMemberId": alice_id,
            "toMemberId": bob_id,
            "content": "task A done, you are unblocked"
        }),
    )
    .await;
    assert_no_jsonrpc_error(&message, "agent_team_message_member");

    let messages = post_json_rpc(
        &rpc_base,
        9348,
        "openhuman.agent_team_list_messages",
        json!({ "teamId": team_id }),
    )
    .await;
    let messages_outer = assert_no_jsonrpc_error(&messages, "agent_team_list_messages");
    assert_eq!(
        messages_outer
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .map(|m| m.len()),
        Some(1)
    );

    // Shut alice down → member stopped (her task A is already done, so nothing
    // is released back to the queue).
    let shutdown_alice = post_json_rpc(
        &rpc_base,
        9362,
        "openhuman.agent_team_shutdown_member",
        json!({ "teamId": team_id, "memberId": alice_id }),
    )
    .await;
    let shutdown_alice_outer =
        assert_no_jsonrpc_error(&shutdown_alice, "agent_team_shutdown_member alice");
    assert_eq!(
        shutdown_alice_outer
            .get("result")
            .and_then(|r| r.get("member"))
            .and_then(|m| m.get("memberStatus"))
            .and_then(serde_json::Value::as_str),
        Some("stopped")
    );
    assert_eq!(
        shutdown_alice_outer
            .get("result")
            .and_then(|r| r.get("releasedTaskIds"))
            .and_then(serde_json::Value::as_array)
            .map(|ids| ids.len()),
        Some(0)
    );

    // A stopped member is no longer part of the live roster, so it can neither
    // send nor receive team messages (`TeamService::ensure_member`).
    let message_from_stopped = post_json_rpc(
        &rpc_base,
        9363,
        "openhuman.agent_team_message_member",
        json!({
            "teamId": team_id,
            "fromMemberId": alice_id,
            "toMemberId": bob_id,
            "content": "sent after shutdown"
        }),
    )
    .await;
    let stopped_err = assert_jsonrpc_error(
        &message_from_stopped,
        "agent_team_message_member from stopped member",
    );
    assert!(
        stopped_err.to_string().contains("unknown member"),
        "stopped member must be rejected: {stopped_err}"
    );

    // Get the team — 2 members, 2 tasks.
    let get = post_json_rpc(
        &rpc_base,
        9349,
        "openhuman.agent_team_get",
        json!({ "teamId": team_id }),
    )
    .await;
    let get_outer = assert_no_jsonrpc_error(&get, "agent_team_get");
    assert_eq!(
        get_outer
            .get("team")
            .and_then(|t| t.get("members"))
            .and_then(serde_json::Value::as_array)
            .map(|m| m.len()),
        Some(2)
    );
    assert_eq!(
        get_outer
            .get("team")
            .and_then(|t| t.get("tasks"))
            .and_then(serde_json::Value::as_array)
            .map(|t| t.len()),
        Some(2)
    );

    api_join.abort();
    rpc_join.abort();
}

#[tokio::test]
async fn billing_rpc_e2e() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_guard = EnvVarGuard::unset("VITE_BACKEND_URL");

    let (mock_addr, mock_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let mock_origin = format!("http://{}", mock_addr);
    write_min_config(&openhuman_home, &mock_origin);

    // Pre-create the user-scoped config so store_session finds correct settings.
    let user_scoped_dir = openhuman_home.join("users").join("e2e-user");
    write_min_config(&user_scoped_dir, &mock_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{}", rpc_addr);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Store a session first — all billing methods require it.
    let store = post_json_rpc(
        &rpc_base,
        1,
        "openhuman.auth_store_session",
        json!({ "token": "e2e-billing-jwt", "user_id": "e2e-user" }),
    )
    .await;
    assert_no_jsonrpc_error(&store, "store_session");

    // Helper: the RPC outcome wraps backend data in {result: ..., logs: [...]}.
    // We peel off the inner "result" field to get the actual backend payload.
    fn inner(outer: &Value, _ctx: &str) -> Value {
        outer
            .get("result")
            .cloned()
            .unwrap_or_else(|| outer.clone())
    }

    // --- billing_get_current_plan ---
    let plan = post_json_rpc(
        &rpc_base,
        2,
        "openhuman.billing_get_current_plan",
        json!({}),
    )
    .await;
    let plan_outer = assert_no_jsonrpc_error(&plan, "billing_get_current_plan");
    let plan_result = inner(plan_outer, "billing_get_current_plan");
    assert_eq!(
        plan_result.get("plan").and_then(Value::as_str),
        Some("PRO"),
        "expected PRO plan: {plan_result}"
    );
    assert_eq!(
        plan_result
            .get("hasActiveSubscription")
            .and_then(Value::as_bool),
        Some(true),
        "expected active subscription: {plan_result}"
    );

    // --- billing_purchase_plan ---
    let purchase = post_json_rpc(
        &rpc_base,
        3,
        "openhuman.billing_purchase_plan",
        json!({ "plan": "PRO_MONTHLY" }),
    )
    .await;
    let purchase_outer = assert_no_jsonrpc_error(&purchase, "billing_purchase_plan");
    let purchase_result = inner(purchase_outer, "billing_purchase_plan");
    assert!(
        purchase_result
            .get("checkoutUrl")
            .and_then(Value::as_str)
            .is_some(),
        "expected checkoutUrl: {purchase_result}"
    );

    // --- billing_create_portal_session ---
    let portal = post_json_rpc(
        &rpc_base,
        4,
        "openhuman.billing_create_portal_session",
        json!({}),
    )
    .await;
    let portal_outer = assert_no_jsonrpc_error(&portal, "billing_create_portal_session");
    let portal_result = inner(portal_outer, "billing_create_portal_session");
    assert!(
        portal_result
            .get("portalUrl")
            .and_then(Value::as_str)
            .is_some(),
        "expected portalUrl: {portal_result}"
    );

    // --- billing_top_up ---
    let top_up = post_json_rpc(
        &rpc_base,
        5,
        "openhuman.billing_top_up",
        json!({ "amountUsd": 10.0, "gateway": "stripe" }),
    )
    .await;
    let top_up_outer = assert_no_jsonrpc_error(&top_up, "billing_top_up");
    let top_up_result = inner(top_up_outer, "billing_top_up");
    assert_eq!(
        top_up_result.get("amountUsd").and_then(Value::as_f64),
        Some(10.0),
        "expected amountUsd 10.0: {top_up_result}"
    );

    // --- billing_create_coinbase_charge ---
    let charge = post_json_rpc(
        &rpc_base,
        6,
        "openhuman.billing_create_coinbase_charge",
        json!({ "plan": "pro" }),
    )
    .await;
    let charge_outer = assert_no_jsonrpc_error(&charge, "billing_create_coinbase_charge");
    let charge_result = inner(charge_outer, "billing_create_coinbase_charge");
    assert!(
        charge_result
            .get("hostedUrl")
            .and_then(Value::as_str)
            .is_some(),
        "expected hostedUrl: {charge_result}"
    );
    assert_eq!(
        charge_result.get("status").and_then(Value::as_str),
        Some("NEW"),
        "expected NEW status: {charge_result}"
    );

    mock_join.abort();
    rpc_join.abort();
}

/// End-to-end test for team RPC methods.
///
/// Spins up an in-process Axum mock backend and a real JSON-RPC server, stores a
/// session JWT, then exercises every team controller through the RPC surface.

#[tokio::test]
async fn team_rpc_e2e() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_guard = EnvVarGuard::unset("VITE_BACKEND_URL");

    let (mock_addr, mock_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let mock_origin = format!("http://{}", mock_addr);
    write_min_config(&openhuman_home, &mock_origin);

    // Pre-create the user-scoped config so store_session finds correct settings.
    let user_scoped_dir = openhuman_home.join("users").join("e2e-user");
    write_min_config(&user_scoped_dir, &mock_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{}", rpc_addr);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Store a session first — all team methods require it.
    let store = post_json_rpc(
        &rpc_base,
        1,
        "openhuman.auth_store_session",
        json!({ "token": "e2e-team-jwt", "user_id": "e2e-user" }),
    )
    .await;
    assert_no_jsonrpc_error(&store, "store_session");

    // Helper: peel off the inner "result" field from the RPC outcome envelope.
    fn inner(outer: &Value, _ctx: &str) -> Value {
        outer
            .get("result")
            .cloned()
            .unwrap_or_else(|| outer.clone())
    }

    let team_id = "team-1";

    // --- team_list_members ---
    let members = post_json_rpc(
        &rpc_base,
        2,
        "openhuman.team_list_members",
        json!({ "teamId": team_id }),
    )
    .await;
    let members_outer = assert_no_jsonrpc_error(&members, "team_list_members");
    let members_result = inner(members_outer, "team_list_members");
    let members_arr = members_result
        .as_array()
        .expect("expected array of members");
    assert_eq!(members_arr.len(), 2, "expected 2 members: {members_result}");
    assert_eq!(
        members_arr[0].get("username").and_then(Value::as_str),
        Some("alice")
    );

    // --- team_create_invite ---
    let invite = post_json_rpc(
        &rpc_base,
        3,
        "openhuman.team_create_invite",
        json!({ "teamId": team_id, "maxUses": 3, "expiresInDays": 7 }),
    )
    .await;
    let invite_outer = assert_no_jsonrpc_error(&invite, "team_create_invite");
    let invite_result = inner(invite_outer, "team_create_invite");
    assert!(
        invite_result.get("code").and_then(Value::as_str).is_some(),
        "expected invite code: {invite_result}"
    );

    // --- team_list_invites ---
    let invites = post_json_rpc(
        &rpc_base,
        4,
        "openhuman.team_list_invites",
        json!({ "teamId": team_id }),
    )
    .await;
    let invites_outer = assert_no_jsonrpc_error(&invites, "team_list_invites");
    let invites_result = inner(invites_outer, "team_list_invites");
    let invites_arr = invites_result
        .as_array()
        .expect("expected array of invites");
    assert!(
        !invites_arr.is_empty(),
        "expected at least one invite: {invites_result}"
    );

    // --- team_revoke_invite (no payload to check, just assert no error) ---
    let revoke = post_json_rpc(
        &rpc_base,
        5,
        "openhuman.team_revoke_invite",
        json!({ "teamId": team_id, "inviteId": "inv-1" }),
    )
    .await;
    assert_no_jsonrpc_error(&revoke, "team_revoke_invite");

    // --- team_remove_member ---
    let remove = post_json_rpc(
        &rpc_base,
        6,
        "openhuman.team_remove_member",
        json!({ "teamId": team_id, "userId": "user-2" }),
    )
    .await;
    assert_no_jsonrpc_error(&remove, "team_remove_member");

    // --- team_change_member_role ---
    let role_change = post_json_rpc(
        &rpc_base,
        7,
        "openhuman.team_change_member_role",
        json!({ "teamId": team_id, "userId": "user-1", "role": "MEMBER" }),
    )
    .await;
    assert_no_jsonrpc_error(&role_change, "team_change_member_role");

    mock_join.abort();
    rpc_join.abort();
}

#[test]
fn json_rpc_agent_team_live_member_run_roundtrip() {
    run_json_rpc_e2e_on_agent_stack(
        "json_rpc_agent_team_live_member_run_roundtrip",
        json_rpc_agent_team_live_member_run_roundtrip_inner,
    );
}

/// End-to-end proof that a teammate actually *runs* (#3374 PR4): the lead creates
/// two teammate tasks (B depends on A), messages a named teammate, then starts
/// each member live. Each member's worker drives a real sub-agent against the
/// mock backend (pinned via `modelOverride`), completes its task through the
/// quality gate with the worker output as evidence, and returns to idle —
/// surfaced by polling `agent_team_get`.
async fn json_rpc_agent_team_live_member_run_roundtrip_inner() {
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_guard = EnvVarGuard::unset("VITE_BACKEND_URL");

    let (mock_addr, mock_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let mock_origin = format!("http://{}", mock_addr);

    write_min_config(&openhuman_home, &mock_origin);
    let user_scoped_dir = openhuman_home.join("users").join("e2e-user");
    write_min_config(&user_scoped_dir, &mock_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{}", rpc_addr);

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Authenticate so the spawned workers' provider has a backend session.
    let store = post_json_rpc(
        &rpc_base,
        380_001,
        "openhuman.auth_store_session",
        json!({ "token": "e2e-test-jwt", "user_id": "e2e-user" }),
    )
    .await;
    assert_no_jsonrpc_error(&store, "store_session");

    // Lead creates a team with two named teammates.
    let created = post_json_rpc(
        &rpc_base,
        380_002,
        "openhuman.agent_team_create",
        json!({
            "leadAgentId": "lead",
            "summary": "live run e2e",
            "members": [
                { "name": "alice", "agentId": "critic" },
                { "name": "bob", "agentId": "critic" }
            ]
        }),
    )
    .await;
    let created_body = assert_no_jsonrpc_error(&created, "agent_team_create");
    let team_id = created_body
        .get("team")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .expect("team id")
        .to_string();
    let members = created_body
        .get("members")
        .and_then(Value::as_array)
        .expect("members");
    let alice_id = members[0]
        .get("id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let bob_id = members[1]
        .get("id")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    // Task A (no deps) owned by alice; Task B depends on A, owned by bob.
    let assign_a = post_json_rpc(
        &rpc_base,
        380_003,
        "openhuman.agent_team_assign_task",
        json!({ "teamId": team_id, "title": "Task A", "ownerMemberId": alice_id, "dependsOn": [] }),
    )
    .await;
    let task_a_id = assert_no_jsonrpc_error(&assign_a, "assign A")
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let assign_b = post_json_rpc(
        &rpc_base,
        380_004,
        "openhuman.agent_team_assign_task",
        json!({ "teamId": team_id, "title": "Task B", "ownerMemberId": bob_id, "dependsOn": [task_a_id.clone()] }),
    )
    .await;
    let task_b_id = assert_no_jsonrpc_error(&assign_b, "assign B")
        .get("task")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    // Lead messages a named teammate (fromMemberId omitted → lead origin).
    let msg = post_json_rpc(
        &rpc_base,
        380_005,
        "openhuman.agent_team_message_member",
        json!({ "teamId": team_id, "toMemberId": alice_id, "content": "please start Task A" }),
    )
    .await;
    assert_no_jsonrpc_error(&msg, "message_member");

    // Start alice live on Task A → kind "started".
    let start_a = post_json_rpc(
        &rpc_base,
        380_006,
        "openhuman.agent_team_start_member",
        json!({ "teamId": team_id, "memberId": alice_id, "taskId": task_a_id, "modelOverride": "e2e-mock-model" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&start_a, "start alice")
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(Value::as_str),
        Some("started"),
        "alice start should dispatch a worker: {start_a}"
    );

    // Poll until Task A is done.
    assert!(
        poll_team_task_status(&rpc_base, &team_id, &task_a_id, "done").await,
        "Task A must reach done"
    );

    // With A done, start bob live on Task B.
    let start_b = post_json_rpc(
        &rpc_base,
        380_007,
        "openhuman.agent_team_start_member",
        json!({ "teamId": team_id, "memberId": bob_id, "taskId": task_b_id, "modelOverride": "e2e-mock-model" }),
    )
    .await;
    assert_eq!(
        assert_no_jsonrpc_error(&start_b, "start bob")
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(Value::as_str),
        Some("started"),
        "bob start should dispatch a worker: {start_b}"
    );
    assert!(
        poll_team_task_status(&rpc_base, &team_id, &task_b_id, "done").await,
        "Task B must reach done"
    );
    assert!(
        poll_team_members_status(
            &rpc_base,
            &team_id,
            &[alice_id.as_str(), bob_id.as_str()],
            "idle",
        )
        .await,
        "team members must return to idle"
    );

    // Final state: both tasks done with evidence, both members idle, and the
    // lead message is in the team timeline.
    let got = post_json_rpc(
        &rpc_base,
        380_008,
        "openhuman.agent_team_get",
        json!({ "teamId": team_id }),
    )
    .await;
    let team_view = assert_no_jsonrpc_error(&got, "agent_team_get")
        .get("team")
        .cloned()
        .expect("team view");
    let tasks = team_view
        .get("tasks")
        .and_then(Value::as_array)
        .expect("tasks");
    for task in tasks {
        assert_eq!(
            task.get("status").and_then(Value::as_str),
            Some("done"),
            "every task done: {task}"
        );
        assert!(
            task.get("evidence")
                .and_then(Value::as_array)
                .map(|e| !e.is_empty())
                .unwrap_or(false),
            "completed task carries worker-output evidence: {task}"
        );
    }
    let view_members = team_view
        .get("members")
        .and_then(Value::as_array)
        .expect("members");
    for m in view_members {
        assert_eq!(
            m.get("memberStatus").and_then(Value::as_str),
            Some("idle"),
            "members return to idle: {m}"
        );
    }

    let messages = post_json_rpc(
        &rpc_base,
        380_009,
        "openhuman.agent_team_list_messages",
        json!({ "teamId": team_id }),
    )
    .await;
    let msgs = assert_no_jsonrpc_error(&messages, "list_messages")
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages");
    assert!(
        msgs.iter().any(|m| m
            .get("payload")
            .and_then(|p| p.get("from"))
            .and_then(Value::as_str)
            == Some("lead")),
        "lead message present in timeline: {msgs:?}"
    );

    rpc_join.abort();
    mock_join.abort();
}

/// Poll `agent_team_get` until the named task reaches `want` status (or time out).
async fn poll_team_task_status(rpc_base: &str, team_id: &str, task_id: &str, want: &str) -> bool {
    for attempt in 0..160 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let got = post_json_rpc(
            rpc_base,
            38_100_000 + attempt,
            "openhuman.agent_team_get",
            json!({ "teamId": team_id }),
        )
        .await;
        let view = assert_no_jsonrpc_error(&got, "agent_team_get poll");
        // `agent_team_get` answers `{ team: TeamView }`, and TeamView itself nests
        // `{ team, members, tasks }`.
        let tasks = view
            .get("team")
            .and_then(|tv| tv.get("tasks"))
            .and_then(Value::as_array);
        if let Some(tasks) = tasks {
            if let Some(task) = tasks
                .iter()
                .find(|t| t.get("id").and_then(Value::as_str) == Some(task_id))
            {
                if task.get("status").and_then(Value::as_str) == Some(want) {
                    return true;
                }
            }
        }
    }
    false
}

/// Poll `agent_team_get` until every team member reaches `want` (or time out).
/// Task completion and the worker's transition back to idle are separate
/// durable operations, so observing a done task does not yet imply idle.
async fn poll_team_members_status(
    rpc_base: &str,
    team_id: &str,
    expected_member_ids: &[&str],
    want: &str,
) -> bool {
    for attempt in 0..160 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let got = post_json_rpc(
            rpc_base,
            38_200_000 + attempt,
            "openhuman.agent_team_get",
            json!({ "teamId": team_id }),
        )
        .await;
        let members = assert_no_jsonrpc_error(&got, "agent_team_get member poll")
            .get("team")
            .and_then(|tv| tv.get("members"))
            .and_then(Value::as_array);
        if members.is_some_and(|members| {
            members.len() == expected_member_ids.len()
                && expected_member_ids.iter().all(|expected_id| {
                    members.iter().any(|member| {
                        member.get("id").and_then(Value::as_str) == Some(*expected_id)
                    })
                })
                && members
                    .iter()
                    .all(|member| member.get("memberStatus").and_then(Value::as_str) == Some(want))
        }) {
            return true;
        }
    }
    false
}

/// End-to-end: plant a thread's session transcript on disk, then verify the
/// `openhuman.threads_token_usage` RPC reads back the correct cumulative token
/// totals, cost, last-turn usage, model, and inferred context window — the data
/// the composer footer seeds itself from when a thread is selected.

#[tokio::test]
async fn json_rpc_agent_run_mode_set_and_get_round_trip() {
    // `agent.set_run_mode` / `agent.get_run_mode` flip and read back a
    // thread's Plan/Build mode through the per-thread `RunModeHandle`
    // registry (`agent::tinyagents::run_mode`) — no thread/session bootstrap
    // needed since the registry is a bare thread_id-keyed map.
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_url_guard = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _api_url_guard = EnvVarGuard::unset("OPENHUMAN_API_URL");

    let (api_addr, api_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let api_origin = format!("http://{api_addr}");
    write_min_config(openhuman_home.as_path(), &api_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let thread_id = "thread-run-mode-e2e";

    // Defaults to build.
    let initial = post_json_rpc(
        &rpc_base,
        9401,
        "openhuman.agent_get_run_mode",
        json!({ "thread_id": thread_id }),
    )
    .await;
    let initial_result = assert_no_jsonrpc_error(&initial, "agent_get_run_mode initial");
    assert_eq!(
        initial_result.get("mode").and_then(Value::as_str),
        Some("build")
    );

    // Flip to plan.
    let set_plan = post_json_rpc(
        &rpc_base,
        9402,
        "openhuman.agent_set_run_mode",
        json!({ "thread_id": thread_id, "mode": "plan" }),
    )
    .await;
    let set_plan_result = assert_no_jsonrpc_error(&set_plan, "agent_set_run_mode plan");
    assert_eq!(
        set_plan_result.get("mode").and_then(Value::as_str),
        Some("plan")
    );

    // Read it back.
    let after_plan = post_json_rpc(
        &rpc_base,
        9403,
        "openhuman.agent_get_run_mode",
        json!({ "thread_id": thread_id }),
    )
    .await;
    let after_plan_result = assert_no_jsonrpc_error(&after_plan, "agent_get_run_mode after plan");
    assert_eq!(
        after_plan_result.get("mode").and_then(Value::as_str),
        Some("plan")
    );

    // Invalid mode label → error.
    let bad_mode = post_json_rpc(
        &rpc_base,
        9404,
        "openhuman.agent_set_run_mode",
        json!({ "thread_id": thread_id, "mode": "sightsee" }),
    )
    .await;
    assert_jsonrpc_error(&bad_mode, "agent_set_run_mode invalid mode");

    api_join.abort();
    rpc_join.abort();
}

#[tokio::test]
async fn json_rpc_agent_context_breakdown_default_agent_returns_sections() {
    // `agent.context_breakdown` (wire method `openhuman.agent_context_breakdown`,
    // namespace="agent" function="context_breakdown") rebuilds the real
    // orchestrator prompt through `PromptSizeReport::build` and reports its
    // system/tools sections as a stacked-bar-friendly list. Called with no
    // params it must default `agent_id` to "orchestrator" and still work
    // against a bare `write_min_config` setup (no thread bootstrap needed).
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_url_guard = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _api_url_guard = EnvVarGuard::unset("OPENHUMAN_API_URL");

    let (api_addr, api_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let api_origin = format!("http://{api_addr}");
    write_min_config(openhuman_home.as_path(), &api_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let breakdown = post_json_rpc(
        &rpc_base,
        9501,
        "openhuman.agent_context_breakdown",
        json!({}),
    )
    .await;
    let result = assert_no_jsonrpc_error(&breakdown, "agent_context_breakdown default agent");

    assert_eq!(
        result.get("agent_id").and_then(Value::as_str),
        Some("orchestrator"),
        "context_breakdown with no agent_id must default to the orchestrator: {result}"
    );

    let sections = result
        .get("sections")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected non-empty 'sections' array: {result}"));
    assert!(
        !sections.is_empty(),
        "expected at least one prompt section: {result}"
    );
    for section in sections {
        assert!(
            section.get("label").and_then(Value::as_str).is_some(),
            "section missing 'label': {section}"
        );
        assert!(
            section.get("bytes").and_then(Value::as_u64).is_some(),
            "section missing 'bytes': {section}"
        );
        assert!(
            section.get("est_tokens").and_then(Value::as_u64).is_some(),
            "section missing 'est_tokens': {section}"
        );
    }

    api_join.abort();
    rpc_join.abort();
}

#[tokio::test]
async fn json_rpc_commands_list_merges_builtins() {
    // `commands.list` (wire method `openhuman.commands_list`, no params) must
    // at least surface the fixed built-in slash commands (skills.list /
    // flows.list are best-effort and may be empty in this bare setup).
    let _env_lock = json_rpc_e2e_env_lock_async().await;
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let openhuman_home = home.join(".openhuman");

    let _home_guard = EnvVarGuard::set_to_path("HOME", home);
    let _workspace_guard = EnvVarGuard::unset("OPENHUMAN_WORKSPACE");
    let _backend_url_guard = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend_url_guard = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _api_url_guard = EnvVarGuard::unset("OPENHUMAN_API_URL");

    let (api_addr, api_join) = serve_on_ephemeral(mock_upstream_router()).await;
    let api_origin = format!("http://{api_addr}");
    write_min_config(openhuman_home.as_path(), &api_origin);

    let (rpc_addr, rpc_join) = serve_on_ephemeral(build_core_http_router(false)).await;
    let rpc_base = format!("http://{rpc_addr}");

    let list = post_json_rpc(&rpc_base, 9502, "openhuman.commands_list", json!({})).await;
    let result = assert_no_jsonrpc_error(&list, "commands_list");

    let commands = result
        .get("commands")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("expected 'commands' array: {result}"));
    assert!(
        !commands.is_empty(),
        "expected at least the fixed built-ins: {result}"
    );

    let new_entry = commands
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some("new"))
        .unwrap_or_else(|| panic!("expected a built-in '/new' entry: {result}"));
    assert_eq!(
        new_entry.get("kind").and_then(Value::as_str),
        Some("builtin"),
        "'/new' entry should be kind=builtin: {new_entry}"
    );
    assert_eq!(
        new_entry.get("insert").and_then(Value::as_str),
        Some("/new"),
        "'/new' entry should insert literal '/new': {new_entry}"
    );
    assert!(
        new_entry.get("label").and_then(Value::as_str).is_some(),
        "'/new' entry missing 'label': {new_entry}"
    );
    assert!(
        new_entry
            .get("description")
            .and_then(Value::as_str)
            .is_some(),
        "'/new' entry missing 'description': {new_entry}"
    );

    // Every other documented built-in must also be present.
    for builtin_id in ["clear", "plan", "build", "goal", "todo", "stop"] {
        assert!(
            commands
                .iter()
                .any(|c| c.get("id").and_then(Value::as_str) == Some(builtin_id)),
            "expected built-in '{builtin_id}' in commands.list: {result}"
        );
    }

    api_join.abort();
    rpc_join.abort();
}
