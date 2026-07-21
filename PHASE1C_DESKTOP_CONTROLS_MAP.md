# Phase 1c — Desktop-controls removal map (Option A: slim accessibility, don't fully delete)

CONSTRAINT: `accessibility` is a shared backbone. screen_intelligence + voice (both STAY) consume
its permissions/capture/foreground-context/globe-hotkey surface. KEEP that; remove only the
desktop-control half.

## KEEP in accessibility (used by screen_intelligence + voice)
foreground_context, parse_foreground_output, AppContext; globe_listener_{start,poll,stop},
GlobeHotkey{PollResult,Status}; detect_permissions, permission_to_str, PermissionKind/State/Status,
request_microphone_access, detect_microphone_permission, microphone_denied_message,
capture_screen_image_ref_for_context; macOS request_accessibility_access/request_screen_recording_access/
open_macos_privacy_pane; focused_text_context_verbose + validate_focused_target (voice/server + voice/text_input use these — decide: keep or excise from voice too).

## REMOVE from accessibility (desktop-control half)
automate.rs, ax_interact.rs, uia_interact.rs, overlay.rs, vision_click.rs, element_match.rs,
paste.rs, keys.rs, app_fastpaths/, focus.rs text-apply helpers + their *_tests.rs. Drop `uiautomation`
dep (Cargo.toml:317, only used by uia_interact.rs) + review Win32_System_Com feature.

## REMOVE wholesale
- text_input/ (CLEAN): mod.rs:126; all.rs:615-621,968; cli.rs:81; delete dir.
- autocomplete/ (HAS-DEPENDENTS): fix credentials/ops.rs:161,214; app_state/ops.rs:21,895,1295
  (runtime.autocomplete payload → coordinate w/ frontend coreState/store.ts); core/runtime/services.rs:42;
  delete core/autocomplete_cli_adapter.rs + its mod decl; logging.rs:55 comment; all.rs:368-373,866;
  mod.rs:33; delete dir; delete tests/autocomplete_memory_e2e.rs + json_rpc_e2e autocomplete test +
  runtime.autocomplete assertion (~5715-5718, 7104+).
- computer tools: tools/ops.rs:248-257 (LaunchApp/AxInteract/Automate) + :1000-1006 (Mouse/Keyboard gated
  on computer_control.enabled); tools/impl/computer/ dir; impl/mod.rs:2,10; impl/system/mod.rs:6,28
  (keep :30 launch_platform until voice fixed, then remove w/ launch_app.rs); ops_tests.rs:938-1009,1621,1624.
- computer_control config: config/schema/tools/integrations.rs:100-116; tools/mod.rs:13; schema/mod.rs:97;
  types.rs:322,791; agent/harness/session/builder/factory.rs:332-342 (ax auto-approve block);
  tools/user_filter.rs:51-57 group entry + :364 comment.
- voice coupling to excise: voice/always_on.rs:518-533 (accessibility::automate::{RealBackend,run,
  AutomateOptions} + tools::...::launch_platform). Then launch_app.rs + impl/system/mod.rs:30 removable.

## Frontend footprint (separate pass, AFTER 2a UI lands — conflicts on settingsRoute/i18n)
autocomplete: tauriCommands/autocomplete.ts; AutocompletePanel + autocomplete/{CompletionStyle,AppFilter}Section
+ tests; AutocompleteSetupModal; settingsRouteRegistry/Elements + navIcons; coreState/store.ts runtime.autocomplete;
config.ts autocomplete_enabled; i18n settings.autocomplete.* + settings.desktopAgent.* (all 14 locales).
DesktopAgentPanel/ToolsPanel/DeveloperOptionsPanel toggles. text_input: no frontend. computer_control: no frontend.
screen_intelligence permission UI STAYS.
scripts/generate-test-inventory.mjs + scripts/test-rust-e2e.sh: autocomplete test names.

## Docs: AGENTS.md:250 cpal note; docs/{RELEASE-MANUAL-SMOKE,TEST-COVERAGE-MATRIX,tinyagents-drift-ledger}.md.
