# Phase 4 — Fleet supervisor: team/cloud hosting

**Status:** planned. Depends on phase 1 only — can run in parallel with
phases 2/3, because the fleet model needs process-per-user, not context
threading.

**Goal:** a supervisor (`openhuman-fleet`, new crate or repo) that hosts one
core per team member and fronts them behind a single endpoint, so a team
admin can provision/manage members' assistants while every existing client
(`CloudHttpTransport`) keeps working unchanged.

## Decision recap (README §2.3)

Process-per-user, not in-process multi-tenancy: it is the shape the
architecture already has (Tauri = one core per user), it sidesteps the
process-scoped items inventoried in phase 3 (env mutation for child tools,
keyring, Sentry), and it gives per-tenant **security** isolation for free —
agents execute arbitrary tools, so tenant separation should be an OS/container
boundary, not a Rust discipline.

## Architecture

```
client (CloudHttpTransport, per-user base URL + Bearer)
   │
   ▼
openhuman-fleet supervisor
   ├─ edge auth: mint/validate per-tenant session tokens
   ├─ reverse proxy  /:user_id/rpc  →  127.0.0.1:<that user's core port>
   ├─ membership sync ⇄ tinyhumansai/backend  (teams stay backend-truth)
   └─ lifecycle manager
        └─ per user: provision workspace volume →
           run core (child process or container, HostKind::Docker) with
           TokenSource::Fixed(per-tenant token), bind 127.0.0.1:0,
           ServiceSet::headless_api() + per-plan opt-ins (cron, heartbeat)
```

Key properties:

- **Wire contract unchanged** inside and out — the proxy strips
  `/:user_id` and forwards `POST /rpc` verbatim; `CloudHttpTransport`
  (`app/src/services/transport/CloudHttpTransport.ts`) already speaks this
  with a profile Bearer token and per-profile `rpcUrl`.
- **Port arbitration**: cores bind `:0`; supervisor reads the bound port from
  the existing `EmbeddedReadySignal` (child-process variant: ready line on
  stdout or a ready file — decide during implementation).
- **Teams**: membership/roles/invites remain in `tinyhumansai/backend`
  (`src/openhuman/team/` proxy untouched). The supervisor consumes the same
  backend API to decide _which_ cores exist and who may reach them; it adds
  hosting, not authorization semantics.
- **Secrets**: per-tenant token + per-tenant keyfile/env in the child's
  environment — no shared keyring across tenants (phase 3 inventory item).
- **Storage**: per-user workspace volume (`StorageBackend::WorkspaceFs`).
  This is why storage abstraction (phase 2.c) is off this phase's critical
  path; a managed-DB backend would slot in later as a new `StorageBackend`.
- **Health/restart**: existing health controllers over each core's `/rpc`;
  restart backoff via the existing `apply_startup_restart_delay_from_env`
  seam; container resource limits are the v1 noisy-neighbor answer.

## Scope

1. `openhuman-fleet` crate: tenant registry (backed by backend membership),
   lifecycle manager, token mint/validate, reverse proxy, health loop.
2. Container image + deployment doc for `HostKind::Docker` cores
   (`ServiceSet::headless_api()`).
3. Embedder guide in `gitbooks/developing/` covering all four consumption
   modes: Tauri embed, CLI one-shot, library (`AgentRuntime`), fleet.
4. E2E: supervisor + 2 tenants + `CloudHttpTransport`-shaped client script;
   assert tenant A's token cannot reach tenant B's core.

## Risks & mitigations

| Risk                                                               | Mitigation                                                                                                                                   |
| ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Token confusion between edge session tokens and core bearer tokens | supervisor is the only holder of core bearers; clients only ever see edge tokens; naming kept distinct in code (`EdgeToken` vs `CoreBearer`) |
| Resource blow-up (N cores × background services)                   | default `ServiceSet::headless_api()`; cron/heartbeat opt-in per plan tier                                                                    |
| Backend membership drift vs running fleet                          | reconcile loop (same pattern as cron scheduler); deprovision = stop core, retain volume per retention policy                                 |
| Supervisor as SPOF                                                 | stateless proxy + externalized tenant registry; ops concern, out of scope for v1 doc beyond noting it                                        |

## Out of scope (v1)

- In-process multi-workspace hosting (deferred behind phase 3; optimization
  for read-mostly tenants, never a security boundary).
- Managed-DB storage backend (needs phase 2.c traits; separate plan).
- Autoscaling / placement across machines.
