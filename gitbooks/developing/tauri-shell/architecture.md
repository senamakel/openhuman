---
description: Architecture of the Tauri desktop host — process model, window management, sidecar.
icon: code-branch
---

# Tauri shell architecture (`app/src-tauri/`)

## Overview

The **`app/src-tauri`** crate (Rust package **`OpenHuman`**, binary **`OpenHuman`**) is a **desktop-only** host. It embeds the React UI, registers plugins (deep link, opener, OS, notifications, autostart, updater), manages the main window and tray, and **relays JSON-RPC** to the separately built **`openhuman`** core binary.

Non-desktop targets fail at compile time (`compile_error!` in `lib.rs`).

## Directory layout (actual)

```
app/src-tauri/src/
├── lib.rs                 # `run()`, tray/menu actions, plugins, `generate_handler!`, core startup
├── main.rs                # Binary entry
├── core_process.rs        # CoreProcessHandle, spawn/monitor openhuman sidecar
├── core_rpc.rs            # HTTP client to core JSON-RPC
├── commands/
│   ├── mod.rs             # Re-exports
│   ├── core_relay.rs      # `core_rpc_relay`, service-managed core bootstrap
│   ├── openhuman.rs       # Daemon host config, systemd-style service helpers
│   └── window.rs          # show/hide/minimize/close window
└── utils/
    ├── mod.rs
    └── dev_paths.rs       # Resolve bundled AI prompts paths
```

There is **no** `src-tauri/src/services/session_service.rs` in this tree; session semantics are handled in the web layer + backend + core as applicable.

## Data flow: UI → core

```
React (invoke)
    → core_rpc_relay { method, params, serviceManaged? }
        → core_rpc::call HTTP POST to OPENHUMAN_CORE_RPC_URL
            → openhuman binary (src/bin/openhuman.rs → core_server)
```

`CoreProcessHandle` in `core_process.rs` starts or waits for the sidecar; `commands/core_relay.rs` optionally ensures a **service-managed** core is running before relaying.

## Window and tray behavior

- The shell creates a tray icon at startup and wires actions to open the main window or quit.
- In daemon mode (`daemon` / `--daemon`), the main window is hidden on launch and can be reopened from tray actions.
- On macOS `RunEvent::Reopen` also restores and focuses the main window.
- Windows and Linux use the same tray actions (`Open OpenHuman`, `Quit`), with desktop-environment-specific tray rendering differences on some Linux setups.

## Bundled resources

`tauri.conf.json` bundles **`../../skills/skills`** and **`../../src/openhuman/agent/prompts`** so skills and prompt markdown ship with the app.

## Related

- IPC surface: [Commands](./02-commands.md)
- HTTP bridge: [Core bridge](./03-services.md)
- Rust domains (implementation): repo root `src/openhuman/`, `src/core_server/`
