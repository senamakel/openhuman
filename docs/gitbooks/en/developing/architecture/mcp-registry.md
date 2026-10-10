---
description: >-
  How OpenHuman browses MCP registries, connects the servers you declare in
  mcp.json, and gives their tools to agents.
icon: plug
---

# MCP registry

`crates/openhuman-core/src/mcp/registry/` is the host half of OpenHuman's Model Context Protocol (MCP) client support. It browses the supported upstream registries (Smithery and the official modelcontextprotocol registry) and reconciles the install store against your `mcp.json` document. For servers launched as local subprocesses or HTTP-remote endpoints, it also supervises the connection. The tools of installed servers reach agents through the unified tool registry (`crate::tools::registry`).

There is no install-from-catalog RPC and no setup agent. In the Registry tab, a hosted server that needs no setup (one http(s) endpoint, no required or secret inputs) is added in one click: the app declares it in mcp.json through `config_get` / `config_set`, then connects it or opens the sign-in dialog. For any other server you open its own page, read the install instructions there, and declare the server in the mcp.json tab: `{ "mcpServers": { name: { command, args, env } | { url, headers } } }`. The document's contract and its reconciliation live in `tinymcp::registry::config_doc`. `config_ops.rs` here is the RPC envelope over them (`mcp_clients_config_get` and `mcp_clients_config_set`).

The client half lives in the vendored `tinymcp` crate (`vendor/tinymcp`): both transports, the Smithery and official catalogs, the SQLite store, the live connection map, the subprocess supervisor, browser sign-in and the write-audit log. This directory holds only what belongs to the application:

- `host.rs` (one level up, `crates/openhuman-core/src/mcp/host.rs`) holds the one `tinymcp` service this process runs per workspace, and converts config to `tinymcp` types.
- `registry/` holds the `mcp_clients` RPC surface, the agent-facing tools and the prompt-injection scan applied to remote tool definitions.
- `audit/` (next to `registry/`) is the RPC surface over `tinymcp`'s write-audit log.
- `server/` (next to `registry/`) is the `openhuman-core mcp` stdio and HTTP server that exposes this application's own tools to external MCP hosts. See [MCP server](../mcp-server.md). That is the server side, a separate thing from this client code.

The Rust module path is `crate::mcp::registry`, but the RPC namespace and the on-disk SQLite filename are `mcp_clients`. They stay that way so existing frontend code and stored user state keep working. Search for both names when you trace call sites.

All payload types (`InstalledServer`, `McpTool`, `ConnStatus` and the Smithery and official-registry DTOs) come from `tinymcp_bus` and are re-exported under `registry::types`, not redefined here. Types for the static, config-declared server set (`[[mcp_client.servers]]` in `config.toml`) and the shared HTTP/stdio transport primitives are re-exported from `crate::mcp::config_servers` and `crate::mcp::http_client` (thin `pub use tinymcp::...` modules in `mcp/mod.rs`, not directories).

```text
                 ┌────────────────────────────────────────────────┐
   Registries ───►         tinymcp (catalogs, store, supervisor)   │
                 └────────────────────┬───────────────────────────┘
                                      │ browse / declare
                                      ▼
                          ┌──────────────────────┐
   Frontend (Skills UI) ─►│  ops.rs / schemas.rs │  RPC controllers (mcp_clients_*)
                          └──────────┬───────────┘
                                     │ delegates to
                                     ▼
                          ┌──────────────────────┐
                          │   host::for_config    │  the tinymcp service this
                          │   / host::try_service │  workspace's host holds
                          └──────────┬───────────┘
                                     │ tools_safe_for_agent (prompt-injection scan)
                                     ▼
                          tool_registry (agents)

                          supervisor_events.rs ── reconnect-supervisor ticks → DomainEvent
```

## Server transport model

An `InstalledServer` (from `tinymcp_bus`, re-exported as `registry::types::InstalledServer`) carries a `Transport` discriminator with stdio and HTTP-remote variants: a local subprocess (`npx`, `uvx`, or a direct binary) speaking stdio JSON-RPC, or a hosted server (the majority of what Smithery lists) dialled over streamable HTTP. A declaration in `mcp.json` becomes such a row directly (`tinymcp::registry::config_doc`): `command`/`args` → stdio, `url` → HTTP-remote, `env`/`headers` → the credential table. `tinymcp` dials whatever the row says; this domain only decides what the row says.

## Boot-time spawn and the reconnect supervisor

Installed servers are connected as the core comes up, and `mcp::registry::supervisor` (see `crates/openhuman-core/src/mcp/registry/mod.rs`) runs `tinymcp::Supervisor::tick` on an interval against every open workspace host, turning each tick's report into `supervisor_events::publish` calls. Errors never block boot. A broken MCP install should not stop the desktop app from starting. Non-nominal ticks (stays-down, restored, parked) become `DomainEvent`s that reach the Event Log and the desktop notification bridge.

## Layout

| Path                        | Role                                                                                                                                                                                    |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`                    | Module docs, `types`/`connections` re-export shims over `tinymcp_bus`/`super::host`, the reconnect-supervisor loop, the OAuth-callback completion helper, and `tools_safe_for_agent` (the prompt-injection scan applied to every remote tool definition). |
| `host.rs` (parent `mcp/`)   | One `tinymcp` service per workspace, opened on first use; proxy resolution for MCP traffic (`proxy_for_mcp`).                                                                          |
| `helpers.rs`                | Shared RPC-handler plumbing: `Outcome` encoding, identifier guards, workspace-service resolution, credential-name injection.                                                        |
| `ops.rs`                    | `mcp_clients_*` RPC handler implementations (uninstall, list, browse, connect/disconnect, tool call, `update_env`, registry settings). One-to-one with `schemas.rs` handlers; publishes `DomainEvent`s `tinymcp` does not. |
| `config_ops.rs`             | `mcp_clients_config_get` / `config_set`: the `Outcome` envelope, domain events and background connects over `tinymcp`'s `McpRegistry::render_config_doc` / `apply_config_doc`. The `mcp.json` shape, its refusals and the reconciliation (add, rewrite in place under the same `server_id`, or uninstall, merging write-only credentials) live in `tinymcp::registry::config_doc`. |
| `action_tool.rs`            | One deferred agent tool per action on a connected server, built from `tinymcp_bus::agent_tools::action_tool_specs` with `tools_safe_for_agent` as the admission filter. Execution goes through `InstalledServerInvoker::invoke`, which re-checks the live safe-tool list and then calls `service.dynamic().invoke(...)` on the `tinymcp` service, rather than back through `ops.rs`. |
| `schemas/` (`mod.rs`, `registry.rs`, `handlers.rs`, `params.rs`) | Controller schemas + handler dispatch. Re-exported from `mod.rs` as `all_mcp_registry_controller_schemas` / `all_mcp_registry_registered_controllers`.                    |
| `bus.rs`                    | `DomainEvent` subscriber (`McpClientEventSubscriber`) that logs `McpServer*` / `McpClientToolExecuted` lifecycle events.                                                               |
| `supervisor_events.rs`      | Turns a `tinymcp::Supervisor` tick report into this domain's `DomainEvent`s, stamped with the workspace whose host was ticked.                                                          |
| `tools.rs`                  | Agent-facing `mcp_registry_*` tools (search catalog, inspect/list/connect/disconnect/call), thin shims over `ops.rs`. The one mutator (`mcp_registry_uninstall`) ships default-OFF behind the `mcp_manage` toggle; there is no install tool. Distinct from the generic `mcp_list_servers`/`mcp_call_tool` bridge tools. |
| `stub.rs`                   | The disabled facade compiled when the `mcp` feature is off.                                                                                                                            |

## Public surface

The exports from `mod.rs` are narrow on purpose:

```rust
pub use schemas::{
    all_controller_schemas as all_mcp_registry_controller_schemas,
    all_registered_controllers as all_mcp_registry_registered_controllers,
    schemas as mcp_registry_schemas,
};

pub use types::{ConnStatus, InstalledServer, McpTool};
```

`types` and `connections` are thin `pub mod`s. They re-export the `tinymcp_bus` wire vocabulary and the `super::host`-backed lookups. This domain does not define its own copies. Everything else (`bus`, `ops`, `action_tool`, `supervisor_events`, `tools`) is `pub mod` for in-crate callers but not re-exported from `mod.rs`.

## Calls into

- `crate::mcp::host`: resolves the `tinymcp` service for a workspace (`host::for_config`, `host::try_service`, `host::all_hosts`).
- `tinymcp` / `tinymcp_bus`: the catalogs, store, connection map, supervisor, OAuth flow, and wire types.
- `crate::security::prompt_injection::scan_tool_definition`: the scan `tools_safe_for_agent` applies before a remote tool reaches the model.
- `crate::tools::registry`: installed servers' tools land here so agents see them alongside native tools.
- `crate::core::bus::BUS`: publishes `DomainEvent`s (`McpToolRejected`, lifecycle events, supervisor observations).

## Called by

- Core startup, via `mcp::host::init` and the reconnect-supervisor loop.
- The frontend Connections UI. The MCP Servers page (`McpServersTab`) has three tabs over the `openhuman.mcp_clients_*` RPC namespace: Servers (rows, status and the credential form: connect, disconnect, `update_env`, sign-in), mcp.json (`config_get` and `config_set`, the only way to add or remove a server) and Registry (`registry_search` and `registry_get`; a hosted server that needs no setup is declared into mcp.json in one click, any other row opens the server's own page in the browser). `registry_settings_get` / `registry_settings_set` hold the Smithery / official-registry credentials (secret values are write-only). Agents use connected servers directly. The orchestrator finds a connected server's tool through `tool_search`, or loads the `mcp` skill (`use_skill`, guide in `crates/openhuman-core/src/tools/toolpacks/guides/mcp.md`) for the `mcp_registry_*` status, list-tools and tool-call fallback. Agents do not install servers.

## Tests

Focused `*_tests.rs` siblings cover each file: `bus_tests.rs`, `ops_tests.rs`, `schemas_tests.rs`, `action_tool_tests.rs`, `supervisor_events_tests.rs`, `tools_tests.rs`.

## Related

- [`mcp/registry/mod.rs`](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-core/src/mcp/registry/mod.rs) and [`mcp/mod.rs`](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-core/src/mcp/mod.rs): the rustdoc this page mirrors.
- `crate::mcp::config_servers` / `crate::mcp::http_client` (`mcp/mod.rs`): re-export modules for the static config-declared server set and the shared transport primitives, both implemented in `tinymcp`.
- `crates/openhuman-core/src/mcp/audit/`: the RPC surface over `tinymcp`'s write-audit log.
- [MCP server](../mcp-server.md): the `openhuman-core mcp` stdio and HTTP server side.
- [Agent harness](agent-harness.md): how the agent ends up calling MCP tools through `tool_registry`.
- [Architecture](../architecture.md): where this fits in the wider system.
