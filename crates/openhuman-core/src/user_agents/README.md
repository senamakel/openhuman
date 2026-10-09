# user_agents

In SaaS mode (`core::runtime::Mode::Saas`), each user is served as one agent.
This domain maps a gateway user to that agent, lays out the agent's private
state, forces the config it runs with, and keeps the open agents of the
process. A single-user core never serves it: its controllers belong to
`DomainGroup::Operator`, which only `DomainSet::saas()` enables.

## Files

| File | Purpose |
| --- | --- |
| `types.rs` | `UserAgentId` (`u-` + 32 hex chars of `sha256(user_id)`), its metadata, and the operator-plane result types |
| `layout.rs` | `<root>/agents/<id>/{agent.toml, config.toml, workspace/, sandbox/}`, archived agents, and `agent_config`: the forced paths, memory binding and autonomy policy |
| `host.rs` | `AgentHost`: provisioning, lazy open, LRU and idle eviction (never of an agent in use), each agent's derived `CoreContext`, `current()` |
| `gateway.rs` | Which context a gateway request runs under: the operator plane, or the agent of the user named in `X-OpenHuman-User`, after the signature check |
| `surface.rs` | What a user may call: `USER_METHODS`, the exact allowlist applied at dispatch, in the controller list and in `/schema`; the operator scope sees only the operator plane; user thread-id rules |
| `background.rs` | The SaaS background loop: every minute it sweeps idle agents and runs each agent's queued memory jobs (deferred ingests, belief builds) under that agent's context |
| `credentials.rs` | A user agent's TinyHumans credential, stored beside its config |
| `ops.rs` | `provision` / `deprovision` / `list` / `status` / `set_credential` / `clear_credential`, returning `Outcome<T>` |
| `schemas.rs` | The `user_agents.*` controllers |

## Rules

- **Gateway user ids never leave `ops.rs`.** They are hashed into an agent id
  immediately and are never logged, stored or returned.
- **An agent's config is forced, not configured.**
  - Every path sits under the agent's directory.
  - Memory is bound to the agent (`[memory] agent_id`, `root = user:<id>`).
    That binding wins over definition pins and team roots.
  - The autonomy policy is on and supervised, with no auto-approval, no tool
    installation and no trusted roots.
- **The isolation boundary is the agent's `CoreContext`.** It carries the forced
  config and `session_agent = <id>`, and the user families (threads, channels
  for web chat, memory), narrowed further by `surface::USER_METHODS`. Work for a
  user runs under it, which is what the config loader, the session store and
  the per-thread caches key on.
- **Deprovisioning archives.** The agent's directory moves to
  `<root>/deprovisioned/<id>-<unix-secs>-<uuid>/`. Nothing is deleted. An agent
  still in use is not archived; the call fails and can be retried.

## Gateway contract

```text
Authorization: Bearer <service token>                                  every request
X-OpenHuman-User: <gateway user id>                                    work for a user
X-OpenHuman-User-Sig: t=<unix secs>,v1=<hex hmac-sha256(token, "<t>.<user id>")>
```

- No user header: the request runs on the operator plane.
- With one, `openhuman-rpc`'s `saas_gateway` layer handles it in this order:
  1. It checks the bearer **before** anything else. An unauthenticated caller
     cannot tell which users exist, and cannot open agents.
  2. It checks the signature: ±60 s, bound to the user id. It is required
     unless the operator sets `require_user_signature = false`.
  3. It opens the user's agent, which must already be provisioned
     (`403` otherwise).
  4. It runs the request under that agent's context. A user's context cannot
     reach the operator plane.
- Routes a SaaS core never serves answer `404`: `/v1`, `/events*`, `/ws/*`,
  `/socket.io`, `/dev/connect` and `/oauth/*`.

## Credentials

The gateway installs each user's session JWT or API key with
`user_agents.set_credential`. The credential is stored in the agent's own
auth-profile store, so any backend call made under that agent's context
resolves that user's credential and no other. The process-wide
`auth.set_credential` / `clear_credential` refuse in SaaS mode, because they
activate a user directory and rebind process globals. The core never validates
or echoes a credential.

## User surface

- **What a user's context can reach.** It enables the user families
  (`host::user_domains`: threads so far). Within them, only the methods on
  `surface::USER_METHODS` are live. Anything unlisted answers as an unknown
  method and is absent from `/schema`.
- **Why the operator registers those families too.** `DomainSet::saas()`
  registers them so user contexts can derive them. The surface gate keeps the
  operator scope on the operator plane, so the operator never serves user
  methods on its own workspace.
- **Per-user storage.** Threads live under each agent's workspace. The on-disk
  session store is installed for SaaS and resolves the workspace of the
  calling context, so transcripts and turn states are per user too.
- **Thread ids.** A user may choose ids for their own threads (`threads.upsert`)
  of 1–128 characters from `[A-Za-z0-9_-]`. The core-reserved prefixes
  `channel:`, `proactive:` and `subagent:` are refused.
- **Web chat.** `channel.web_chat`, `web_cancel` and the `web_queue_*` methods
  are open, along with the turn-starting thread methods. Every `WebChannelEvent` is
  stamped with the publishing context's agent (`WebChannelEvent::agent`, never
  serialized). A user's `GET /events?client_id=` stream runs under that user's
  gateway scope and carries only events stamped with that user's agent. Two
  users on the same client id never see each other's turns. An unstamped event
  belongs to no user. The operator has no chat stream, and browser bind tokens
  are not accepted in SaaS.
- **Per-agent keys.** The web chat session cache, the in-flight turns and the
  parallel (forked) turns are keyed by agent and id, so caller-chosen thread
  and request ids never collide across users.
- **Deprovisioning** also clears the agent's credential, which lives in the
  process keyring under the agent id. Otherwise a re-provisioned user would
  inherit the old credential.
- **Prompt.** In SaaS the runtime section says `Host: hosted` instead of the
  server's hostname. The `## User` identity block stays empty, because no
  process-wide identity is ever set.

## Background work

A single-user core drains memory jobs from a cron system job behind the process-wide scheduler gate. A SaaS core cannot use either: the cron service is off, and the gate reflects the operator, who holds no credential. So `background::spawn` (started by `saas::build`) runs one loop per process instead. Each tick:

- sweeps idle agents;
- visits every provisioned agent whose workspace has queued memory jobs (`memory::lifecycle::jobs::has_pending`);
- opens that agent and runs its due jobs under the agent's own context, with its config, credential and memory root.

Agents with nothing queued are not opened.

On the **first** open of an agent in a process, `recover_workspace` settles anything a previous process left in that agent's workspace:

- turns that were mid-flight are marked interrupted;
- run-ledger rows left running are closed.

A re-open after an eviction never sweeps again, because this process may still be running one of that agent's turns.

User cron jobs are not served yet: the cron RPCs are not on the user surface.
