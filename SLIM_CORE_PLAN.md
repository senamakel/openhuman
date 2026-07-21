# Slim-core module removal — execution plan

Branch: `chore/slim-core-modules` (worktree). Central wiring: `src/openhuman/mod.rs`
(module decls), `src/core/all.rs` (controller registration, DomainGroup-tagged),
`Cargo.toml [features]`. UI: `app/src` (React/Vite), `app/src-tauri` (Tauri Rust shell).

Decisions (from user):
- **Meet**: keep both `meet` + `meet_agent` — NOT duplicates. No change.
- **todos / thread_goals**: full tinyagents crate cutover, then delete.
- **whatsapp_data**: full relocate to Tauri.
- Also drop each removed module from the **UI**.

## Phase 1 — clean deletes (low risk)
- [ ] **codegraph**: `mod.rs:36`; `tools/mod.rs:21` (glob); `tools/ops.rs:258-265` (2 tool regs);
  `security/policy/types.rs:188` (WORKSPACE_INTERNAL_DIRS entry); rm dir.
  Prompt/doc cleanup: code_executor agent.toml/prompt.md, tools/README.md. UI: codegraph refs.
- [ ] **council_registry**: `mod.rs:42`; `all.rs:423-427`; rm dir.
  UI: councilRegistryApi.ts(+test), scripts/seed-councils.mjs, i18n modelCouncil.*.
- [ ] **model_council**: `mod.rs:92`; `all.rs:428-433`; delete e2e tests in tests/json_rpc_e2e.rs
  (1072, 2253, 2308, 2317, 2394, 2435-2482, 2512-2558); rm dir.
  UI: ModelCouncilTab.tsx(+test), modelCouncilApi.ts(+test), i18n.
- [ ] **desktop_companion**: `mod.rs:47`; `all.rs:742-746`; `socketio.rs:909-940` (bus subscriber)
  + `socketio.rs:642` (io_companion binding); `about_app/catalog_data.rs` companion.session/pointing;
  rm dir. Then re-gate `meet_agent::wav` under `meet` (carve-out no longer needed — verify).
  UI: companionSlice(+test), store/index.ts, CompanionPanel.tsx, settingsRouteRegistry,
  Skills.tsx:503/1076, SidebarNav, socketService (companion:state_changed), i18n.

## Phase 1b — webview simple-deletes / relocate
- [ ] **webview_apis** (dead — Gmail retired): `mod.rs:154`; `all.rs:264,1003`; rm dir.
  Verify `tokio-tungstenite` (Cargo.toml:297,322) has no other core WS client → drop if unique.
- [ ] **webview_accounts** (orphaned): `mod.rs:153`; fix doc ref `connectivity/rpc.rs:528`; rm dir.
- [ ] **webview_notifications** (relocate): move `format_title`/`OPENHUMAN_TITLE_PREFIX`/wire types
  into Tauri crate (near webview_accounts/native_notifications); `mod.rs:155`; `all.rs:709`; rm dir.
  Keep prefix byte-identical (app/src-tauri/src/webview_accounts/mod.rs:1148 doc contract).

## Phase 2 DESIGN (updated per user)

Bridge mechanisms (established):
- shell→core: `relay_http_rpc` Tauri command → core `/rpc` (exists; frontend + shell use it).
- core→shell: loopback WS (the retired `webview_apis` pattern). Tauri server infra still lives at
  `app/src-tauri/src/webview_apis/{server,router}.rs` (router empty). Core-side client was deleted
  in Phase 1 — revive a generalized version for agent-tool proxying.

### Phase 2a — desktop_companion → Tauri (REVERSAL: migrate, don't delete)
Phase 1 removed it from core (commit b4e9e80f9). Now ADD it to the Tauri shell to complete the
migration. Recover source from `b4e9e80f9^:src/openhuman/desktop_companion/*`.
- Move orchestration (session state machine, pipeline turn loop, pointing parse, handoff) into
  `app/src-tauri/src/` (companion already has shell-side hotkey/mic/screen/overlay).
- Tauri companion calls core over `relay_http_rpc` for: STT (`voice::cloud_transcribe`), TTS
  (`voice::reply_speech`), config, LLM (backend chat-completions). wav packing + accessibility
  foreground context: use shell-side equivalents.
- Replace core `companion.*` controllers with Tauri commands + emit `companion:state_changed`
  from the shell. KEEP the overlay/notch companion bubble + CompanionPointer (do NOT remove).
- Re-add about_app catalog entries pointing at the Tauri feature (or a shell-provided catalog).

### Phase 2b — whatsapp_data → Tauri (relocate + agent tools via bridge)
Core removal: `all.rs:754,818,1038,1230`; `runtime/context.rs` boot-plan field;
`observability.rs:158,611-614,804-835,2175-2184` error classifiers; `security/policy/types.rs:186`;
`tools/mod.rs:67`; `tools/ops.rs:458,1332`; rm dir.
Tauri: net-new SQLite store + ingest + list/search in app/src-tauri; scanner
(app/src-tauri/src/whatsapp_scanner/mod.rs:1015,1090) writes locally instead of POST to core.
Frontend: memory.ts:390,409 → Tauri command. Agent-query bridge decision: how does the agent still
query WhatsApp history once the store is shell-side? (tool → Tauri IPC round-trip, or drop agent tools).

## Phase 3 — todos + thread_goals tinyagents cutover (largest, breaking)
Crate targets: `tinyagents::graph::goals`, `tinyagents::graph::todos` (vendor/tinyagents).
- **thread_goals**: wire `migrate_legacy_goals_into_crate_store` into boot; flip reads to crate;
  re-home RPC controllers (schemas.rs → crate store) [all.rs:540,939]; re-home runtime.rs
  (context block/budget hook/turn accounting) [turn/core.rs:770-1577, 7 sites]; heartbeat
  continuation [heartbeat/engine.rs:106]; tool wrappers [tools/ops.rs:802-809];
  agent_prepare_context.rs:349; about_app/catalog_data.rs:498; rm dir.
- **todos**: reconcile divergences (Scratch fallback, task-<uuid> vs task-<seq> ids, RFC3339 vs
  epoch-ms); migrate `agent::task_board::TaskBoardStore` (authoritative, ~35 consumers, NOT crate-backed)
  onto `graph::todos::store`; re-home ops.rs/runs.rs/schemas.rs/tools.rs; [all.rs:703; tools/mod.rs:64;
  ~20 consumers: task_dispatcher, triage, task_sources, skills, agent/tools/{todo,update_task}]; rm dir.

## Validation
- `cargo check --workspace` green after each phase.
- Rebuild image-affecting services under docker compose; targeted tests per slice, full suite at end.
- Commit per module/slice. Don't push unless asked.
