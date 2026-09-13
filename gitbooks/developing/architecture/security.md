---
description: >-
  Trust boundary for the autonomous core - autonomy / risk policy, pluggable
  sandbox backends (Docker, Bubblewrap, Firejail, Landlock, Noop), audit log,
  encrypted secret store, public-bind / pairing guard, and the redact() helper.
icon: shield-halved
---

# Security (`crates/openhuman-core/src/security/`)

`crates/openhuman-core/src/security/` is the **trust boundary for the autonomous core**. It owns the autonomy / risk policy that decides whether a given tool call is allowed, the pluggable sandbox backends that confine those calls when the host supports it, the append-only audit log of every agent action, the encrypted secret store, the pairing guard that gates public binding of the RPC server, and the `redact()` helper every other domain uses to keep logs free of plaintext credentials.

It does **not** own:

- The cross-domain `EncryptionEngine`, which lives in `crates/openhuman-core/src/security/encryption/`.
- Per-channel credential storage, which lives in `crates/openhuman-core/src/security/credentials/`.

This module is the place to look first when asking "is this agent action allowed, and if so, how is it confined?"

## Public surface

| Item                                                                                                                          | File         | Purpose                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------- | ------------ | ----------------------------------------------------------------------- |
| `SecurityPolicy`                                                                                                              | `policy/types.rs` (path checks in `policy/path_checks.rs`, command classification in `policy/command_checks.rs`, gating in `policy/enforcement.rs`) | Assembles runtime policy from `AutonomyConfig` + workspace dir.         |
| `AutonomyLevel` (`Supervised` / `SemiAutonomous` / `Autonomous`)                                                              | `policy/types.rs` | Three-step autonomy ladder.                                             |
| `CommandRiskLevel`, `ToolOperation`, `ActionTracker`                                                                          | `policy/types.rs` | Risk classification + per-session counting.                             |
| `Sandbox` trait, `NoopSandbox`                                                                                                | `traits.rs`  | The pluggable sandbox abstraction; every backend implements `Sandbox`.  |
| `create_sandbox(&SecurityConfig) -> Arc<dyn Sandbox>`                                                                         | `detect.rs`  | Picks the best backend available on the host at runtime.                |
| `pub mod docker / bubblewrap / firejail / landlock`                                                                           | (siblings)   | Per-backend implementations of `Sandbox`.                               |
| `SecretStore`                                                                                                                 | `keyring/encrypted_store.rs` (`secrets.rs` re-exports it) | OS-keychain / encrypted-file secret persistence with round-trip helpers. |
| `AuditLogger`, `AuditEventType`, `AuditEvent`, `Actor`, `Action`, `ExecutionResult`, `SecurityContext`, `CommandExecutionLog` | `audit.rs`   | Append-only audit trail.                                                |
| `PairingGuard`, `constant_time_eq`, `is_public_bind`                                                                          | `pairing.rs` (`PairingGuard` and `constant_time_eq` are re-exported from `tinychannels_bus::security`) | Pairing-token check before binding the RPC server publicly.             |
| `redact(value: &str) -> String`                                                                                               | `core.rs`    | Uniform 4-char-prefix redaction for logs.                               |
| `security_policy_info_for_config(&Config) -> RpcOutcome<serde_json::Value>`                                                    | `ops.rs`     | RPC handler for the doctor / settings UI.                               |

## Sandbox backend selection

`detect::create_sandbox` walks a preference list and returns the **first available** backend on the host. The exact order is encoded in `detect.rs`; in practice it favours the strongest available isolation:

```text
                ┌──────────────┐
SecurityConfig ─►│ create_sandbox│
                └──────┬───────┘
                       │ probes
                       ├─► Docker      (best isolation; needs daemon)
                       ├─► Bubblewrap  (Linux user-namespace sandbox)
                       ├─► Firejail    (Linux setuid sandbox)
                       ├─► Landlock    (Linux LSM; in-process)
                       └─► Noop        (last resort; logs only)
```

The agent never sees the choice; it just calls into `Sandbox::run(...)` and the active backend handles the rest. Every backend lives in a sibling file (`docker.rs`, `bubblewrap.rs`, `firejail.rs`, `landlock.rs`); the noop fallback is in `traits.rs`.

## Autonomy ladder

`AutonomyLevel` is a three-step ladder that controls how aggressively the policy gates tool calls:

- **Supervised**: every higher-risk tool call requires an explicit approval round-trip.
- **SemiAutonomous**: low / medium-risk tool calls flow through; higher-risk ones still approval-gate.
- **Autonomous**: the policy lets the agent run unattended within budget and risk caps.

`CommandRiskLevel` + `ToolOperation` classify a given tool call; `ActionTracker` keeps the per-session counts that the policy compares against caps. The agent harness asks `SecurityPolicy` for a decision before every executable tool dispatch.

## Audit log

`audit.rs` writes an append-only stream of `AuditEvent`s under the workspace dir. Every executable tool call lands here with its `Actor` (agent / user), `Action`, `ExecutionResult`, and the `SecurityContext` (autonomy level, sandbox backend, etc.) it ran under. The log is the post-hoc story of what the agent did and why it was allowed.

## Pairing guard

`PairingGuard` (in `pairing.rs`) stands between the RPC server and any attempt to bind to a non-loopback address. `is_public_bind` detects the dangerous case; `PairingGuard` requires a constant-time-compared pairing token (`constant_time_eq`) before such a bind is permitted. This is the iOS / LAN-companion pairing flow's defence against an unpaired peer attaching to the desktop core.

## Secret store

`SecretStore` (implemented in `keyring/encrypted_store.rs`, re-exported through `secrets.rs`) encrypts config-field secrets with ChaCha20-Poly1305 (`enc2:` prefix) under a keychain-backed master key, migrating the legacy XOR `enc:` format on decrypt. Backend selection and the encrypted-file fallback are described in `crates/openhuman-core/src/security/keyring/README.md`.

## `redact()`

`redact(value)` returns a uniform 4-char-prefix string (e.g. `"sk-a"` -> `"sk-a…"`) for use in logs and error messages. Use it whenever a secret, credential, token, or PII string is about to be formatted into a `log::` / `tracing::` call. Other domains call it directly: `credentials/`, `webhooks/`, `composio/`, the integration adapters.

## Layout

| Path                                                          | Role                                                                      |
| ------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `policy/` (`mod.rs`, `types.rs`, `path_checks.rs`, `command_checks.rs`, `enforcement.rs`, `policy_command*.rs`, `policy_tests*.rs`, `proptest_tests.rs`) | `SecurityPolicy`, `AutonomyLevel`, risk classification, path and command checks, action tracking. |
| `traits.rs`                                                   | `Sandbox` trait + `NoopSandbox` fallback.                                 |
| `detect.rs`                                                   | `create_sandbox`: best-available-backend selection.                       |
| `docker.rs` / `bubblewrap.rs` / `firejail.rs` / `landlock.rs` | Per-backend `Sandbox` implementations.                                    |
| `core.rs`, `core_tests.rs`                                    | `redact()` + small shared helpers.                                        |
| `audit.rs`                                                    | Append-only audit log types.                                              |
| `secrets.rs`, `keyring/`                                      | `SecretStore` (implemented in `keyring/encrypted_store.rs`) + round-trip tests. |
| `pairing.rs`, `pairing_tests.rs`                              | `PairingGuard` + constant-time helpers.                                   |
| `ops.rs`                                                      | RPC handler (`security_policy_info_for_config`).                          |
| `schemas.rs`                                                  | Controller schemas + handler dispatch.                                    |
| `mod.rs`                                                      | Re-exports of the public surface above.                                   |

## Calls into

- `crates/openhuman-core/src/config/`: `SecurityConfig`, `AutonomyConfig` for policy + sandbox selection.
- OS-level sandbox tools: `docker`, `bwrap`, `firejail`, Landlock syscalls (per backend).
- Workspace filesystem, for the audit log and secret store.

## Called by

- `crates/openhuman-core/src/cron/ops.rs`: wraps shell jobs in `SecurityPolicy::from_config`.
- `crates/openhuman-core/src/tools/ops.rs` and most `tools/impl/{system,network,memory,agent}/*.rs`: every executable tool consults `SecurityPolicy`.
- `crates/openhuman-core/src/tools/impl/network/{curl,http_request,web_fetch,mcp}.rs`: risk-classify outbound calls.
- `crates/openhuman-core/src/memory/tools/{store,forget}.rs`: sensitive-write tracking.
- `crates/openhuman-core/src/agent/tools/delegate.rs`: sub-agent dispatch goes through the autonomy gate.
- `crates/openhuman-core/src/security/credentials/`: uses `SecretStore` and `redact`.

## Tests

- Unit: `pairing_tests.rs`, `policy/policy_tests*.rs`, `policy/proptest_tests.rs`, `keyring/encrypted_store_tests*.rs`.
- `core_tests.rs` covers `redact()`.
- Sandbox-backend smoke tests: `docker_tests.rs`, `bubblewrap_tests.rs`, `firejail_tests.rs`, `landlock_tests.rs`, `detect_tests.rs`.

## Related

- [`security/README.md`](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-core/src/security/README.md): authoritative internal-audience overview this page mirrors.
- [Architecture overview](../architecture.md): wider system context.
- [Agent Harness](agent-harness.md): where `SecurityPolicy` is consulted on every tool dispatch.
