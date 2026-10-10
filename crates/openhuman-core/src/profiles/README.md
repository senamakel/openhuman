# profiles

In SaaS mode (`core::runtime::Mode::Saas`) one process serves many users, and
each user is served as one **profile**. A profile is the tenant: one user's
config, credential, workspace, sandbox and conversation threads, one thread per
chat interface (web chat, and each hosted platform the gateway relays). This
domain maps a gateway user to that profile, lays out the profile's private
state, forces the config it runs with, leases it to one process, and keeps the
open profiles of the process.

A single-user core (desktop, CLI, `openhuman_embed::Runtime`) never serves it:
its controllers belong to `DomainGroup::Operator`, which only
`DomainSet::saas()` enables, and `host::host()` is `None` outside SaaS.

Two hosts drive it:

- `openhuman-core run --mode saas --saas-config <file>`: the JSON-RPC server
  behind a trusted gateway (`openhuman-rpc`'s `saas_gateway`). See
  [SaaS profiles](../../../../gitbooks/developing/saas-profiles.md) for
  deploying it.
- `openhuman_embed::ProfileRuntime`: the same host in-process, without the
  gateway (`crates/openhuman-embed/src/profiles.rs`).

## Layout

```text
<root>/users/<profile-id>/          the desktop's users/<id> shape (config::schema::ProfileLayout)
  profile.toml                      ProfileMeta (only without a storage backend)
  .lease, .lease.json               the file-lock lease (only without a storage backend)
  config.toml                       the profile's config_path (never read in SaaS)
  auth-profiles.json                its credential (unless secrets live in the backend)
  workspace/                        threads, sessions, memory, cron, cost and run ledgers
  sandbox/                          action_dir: the only place its tools may act
<root>/deprovisioned/<id>-<secs>-<uuid>/   an archived profile
<root>/operator/                    the operator plane's own state (operator_dir overrides it)
<root>/service.token                the gateway bearer (service_token_file overrides it)
```

With a storage backend (`storage_url` / `OPENHUMAN_STORAGE_URL`):

| Where | What |
| --- | --- |
| scope `cluster`, collection `profiles` | the profile registry (replaces `profile.toml`) |
| scope `cluster`, collection `leases` | the profile leases (replace the file locks) |
| scope `profile:<id>` (`storage::scope_for_profile`) | that profile's records on the storage ports: secrets and auth profiles, approvals, cron, flows, ... |
| session key `<id>~<agent or default>` (`tenant::session_key`) | its session store: transcripts, turn states |

A profile's files stay under `<root>/users/<id>/` either way, so the nodes of a
cluster share `<root>` on one volume (see [Known limits](#known-limits)).

The desktop config loader resolves a signed-in user's `users/<id>/config.toml`
and `workspace/` through the same `ProfileLayout`, so the two layouts cannot
drift. The older `<root>/agents/u-<hash>/` layout is gone; data left there is
not migrated.

## Profile ids

The top-level `profile_ids` setting in the operator config
(`profile_ids = "hashed"`, not under a table) picks how a gateway user id
becomes a `ProfileId`:

- `"raw"` (the default): a user id matching `^[a-z0-9][a-z0-9_-]{0,63}$` is
  used unchanged, so the desktop's 24-hex backend ids pass through. `local`,
  `operator` and anything starting with `h-` are reserved.
- Anything else, or every id under `"hashed"`, becomes `h-` plus the first 32
  hex characters of `sha256(user_id)`.

Provisioning also refuses a profile id equal to the file name of the node's
`operator_dir`: keyring secrets are namespaced by their store's directory name,
so such a profile would share the operator's credential slots.

Both forms fit the agent-id charset and never contain a path separator.
Changing the mode may re-map users onto different profiles. Gateway user ids
are turned into profile ids at once (`ops.rs`, `gateway.rs`) and are never
logged. Under raw mode a user id that fits the charset *is* the profile id, so
it reaches paths, storage and the provisioning response; use `"hashed"` where
user ids must not. `ProfileRuntime` may include the profile id in debug logs
during `provision` and `open`; do not treat debug logs as profile-id-free.

## The tenant key

The isolation boundary is the profile's own `CoreContext`, derived from the
operator's (`ProfileHost::open`) with:

- the forced config ([`layout::profile_config`](layout.rs));
- its own security policy (`agent_policy`, built over its workspace and
  sandbox), never the operator's live policy;
- `profile = <profile id>`: the **tenant key**;
- `session_agent = <profile id>` (see the rules below);
- fresh state slots, and the user families only (`host::user_domains`:
  threads, channels, memory), narrowed further by `surface::USER_METHODS`.

Work for a user runs under that context, and everything per-tenant keys on
`core::runtime::current_tenant()`, which in SaaS reads only the task's own
scope and fails closed (`NoTenant`) instead of falling back to the operator:

| What | Keyed by |
| --- | --- |
| storage backend records | `storage::scope_for_profile` |
| session store | `tenant::session_key` |
| web chat sessions, in-flight and parallel turns, background completions | `tenant::tenant_key(tenant, thread or request id)` |
| `/events` | `WebChannelEvent::profile`, stamped at publish, filtered per stream |
| cost ledger | the profile's own `CostTracker`, seeded at open |
| config loader | the context's config (`config/schema/load/saas_scope.rs`) |

So two users' `t1` are two threads, a cancel on one never stops the other,
and neither sees the other's events.

## Leases: one process per profile

Exactly one process hosts a profile at a time. `ProfileHost::open` takes the
profile's lease (`storage::lease`) before it opens it:

- **With a storage backend** it uses `DocumentLeases` in the backend's
  `cluster` scope: one compare-and-swapped record per profile with an owner
  (`node_id`), an endpoint (`advertise_url`), an epoch and an expiry
  `lease_ttl_secs` (default 30) out. Every node sharing the backend contends
  for the same records. A clustered node (one that sets `advertise_url`) must
  use a driver whose CAS holds across processes
  (`storage::driver_has_cross_process_cas`: SQLite, MongoDB); the boot guard
  refuses anything else.
- **Without one** it uses `LocalLeases`: an exclusive `flock` on
  `<root>/users/<id>/.lease`, which keeps two processes on one root apart and
  drops when its holder dies.

What the lease drives:

- **Refusal.** A live lease held elsewhere makes `open` return
  `OpenError::HeldElsewhere(record)`. The gateway answers `409` (below);
  `ProfileRuntime::open` returns the error with the record.
- **Recovery.** A lease taken over from a holder that never released it
  (`previous_unclean`: it crashed or stopped renewing) means that holder's
  turns may have died mid-flight. `recover_workspace` marks them interrupted
  and settles orphaned run-ledger rows, and a storage-backed session store is
  swept for the profile too. A clean hand-over (eviction, release, shutdown)
  recovers nothing, so a re-open never interrupts live work. This is why the
  SaaS session store never recovers on open
  (`session_store::install_for_saas`).
- **Heartbeat.** `lease::heartbeat` renews every open profile's lease every
  third of the TTL. A renewal that finds the record changed (another node
  took over, or an operator released it), or that keeps failing until the
  grant would have expired, **fences** the profile: it is marked fenced, its
  in-flight turns are stopped (`web_chat::cancel_all_turns` under its
  context) and it is closed. `ensure_hosted` refuses new turns for a profile
  this node no longer hosts.
- **In use.** A profile is in use while anyone holds its `Profile` (a gateway
  request, a `ProfileHandle`) or a turn still runs on its context
  (`CoreContext::tenant_in_use`). A profile in use is never evicted, released
  or deprovisioned from under its work.
- **Release.** Idle eviction (`idle_evict_secs`, or LRU when
  `max_profiles_open` is reached), `profiles.release {profile_id}`,
  deprovisioning and a clean shutdown release the lease, so another node can
  take the profile at once. On shutdown a busy profile keeps its lease, so
  its successor treats the hand-over as unclean and recovers its turns.
- **Node ids** (`node_id`, else `OPENHUMAN_NODE_ID`, else random per process)
  must be unique among live nodes. A stable one lets a restarted node take its
  own profiles back at once instead of waiting out their leases.

## Chat interfaces: one thread each

A profile holds one thread per conversation, whatever interface it came in on:

- **Web chat.** The user picks the thread id (`threads_upsert`,
  `channel_web_chat`): 1–128 characters of `[A-Za-z0-9_-]`. The core-reserved
  prefixes `channel:`, `proactive:` and `subagent:` are refused
  (`surface::validate_user_thread_id`).
- **Hosted platforms (relay).** The gateway owns the hosted Telegram, iMessage
  and Discord webhooks and account links. It relays each message in as its
  user with `channel_relay_inbound`
  ([`channels/providers/relay`](../channels/providers/relay/README.md)). The
  message lands on the user's `channel:<channel>/<sender>/<chat>` thread,
  which the user cannot mint themselves; its turn runs through the channel
  dispatch pipeline under the profile's context; each reply is published as a
  `channel_outbound` event on that user's `/events` stream for the gateway to
  deliver. A retried `message_id` runs nothing twice.
- **Deferred.** Native per-profile listeners (a user's own bot token) and
  several agents per profile need the host-agent resolver and
  `AgentContextRegistry` keyed by `(profile, agent)` first.

## Rules

- **A profile's config is forced, not configured.** `profile_config` is pure:
  `config.toml` is never read in SaaS, and storage needs no config document.
  - Every path sits under the profile's directory; artifacts land in its
    `sandbox/files`.
  - Memory is bound to the profile (`[memory] agent_id`, `root = user:<id>`).
    That binding wins over definition pins and team roots.
  - The memory layout is pinned to the legacy tree (confined to that root by
    `memory::user_scope`). Layout v3 would bind the engine below
    `memory::scope::user_root`, which reads `users/<id>` from the config path
    and would answer `org:<id>` or a minted local root instead.
  - The autonomy policy is on and supervised, with no auto-approval, no tool
    installation and no trusted roots.
- **`session_agent` stays the profile id.** The default agent was meant to run
  with no `session_agent`, as on the desktop. That needs every site that keys
  on the acting agent to key on the tenant instead; the approval gate's thread
  routes, the MCP host, skill homes, origin delivery and the transcript
  fallback still read `agent_scope::current_agent_id()`, so a profile without
  an agent id would share their keys with every other profile. Until those
  move to `current_tenant`, the profile id doubles as the agent id.
- **No ambient fallback.** Tenant-keyed code reads `current_tenant()`, never
  `CoreContext::current()` or `.session_agent()`; `pnpm saas:ambient`
  ratchets the reads that remain (`ambient-context`), alongside bare spawns,
  direct `load_or_init` calls, environment writes and `home_dir()` lookups.
  Detached work uses `spawn_scoped` so it keeps its profile.
- **Host tools are opt-in and confined.** A user's context has no `Platform`
  family, so shell and file tools are absent unless the operator lists their
  group in `tool_allowlist`:
  - `host_files` (`file_read`, `file_write`, `edit`, `apply_patch`, `grep`,
    `glob`, `list`, `csv_export`, `read_workspace_state`) runs in-process,
    confined by the forced policy to the profile's `sandbox/`. In SaaS the
    policy grants neither `~/OpenHuman/projects` nor `/tmp/openhuman`, which
    every user would share.
  - `host_shell` runs every command in a fresh Docker container
    (`[sandbox]`: image, network, memory and CPU limits): read-only root
    filesystem, all capabilities dropped, no host environment, and the
    profile's `sandbox/` as its only writable mount. If the container cannot
    start, the command fails; it never falls back to the host.
  - Tools that change the process, install code or reach shared state are
    hard-denied whatever the allowlist says (`tools::HARD_DENIED`).
  - There is no per-user approval surface, so the approval gate never parks
    in SaaS: it allows tools from an allowlisted group and refuses the rest.
- **The user surface is an allowlist.** Within the user families, only the
  methods on `surface::USER_METHODS` are live: thread reads and writes
  (including `threads_delete` and `threads_purge`, confined to the caller),
  the turn-starting thread methods, web chat with cancel and queue control,
  `channel_relay_inbound`, and memory reads and writes confined to the user's
  tree. Anything unlisted answers as an unknown method and is absent from
  `/schema`. `DomainSet::saas()` registers the user families so profile
  contexts can derive them, and the surface gate keeps the operator scope on
  the operator plane, so the operator never serves user methods on its own
  workspace.
- **Deprovisioning archives.** The profile's directory moves to
  `<root>/deprovisioned/<id>-<unix-secs>-<uuid>/`; nothing is deleted. Its
  credential is cleared first (it lives in the keyring or the storage
  backend, not only in the directory), so a re-provisioned user does not
  inherit it. If clearing it fails the profile is kept, not archived, and its
  lease is given back so the retry can take it. A profile in use, or hosted
  by another node, is not archived: the call fails and can be retried.

## Gateway contract

```text
Authorization: Bearer <service token>                                  every request
X-OpenHuman-User: <gateway user id>                                    work for a user
X-OpenHuman-User-Sig: t=<unix secs>,v1=<hex hmac-sha256(service token, "<t>.<user id>")>
```

- No user header: the request runs on the operator plane (`profiles.*`).
- With one, `openhuman-rpc`'s `saas_gateway` layer handles it in this order:
  1. The bearer, **before** anything else (`401`). An unauthenticated caller
     cannot tell which users exist, and cannot open profiles.
  2. A duplicate or unreadable user header (`400`).
  3. The signature: ±60 s, bound to the user id (`401`). Required unless the
     operator sets `require_user_signature = false`.
  4. The user's profile, opened and leased: `403` when it is not provisioned,
     `503` when every slot is busy or storage fails, and **`409`** when
     another node hosts it:

     ```text
     X-OpenHuman-Profile-Owner: <owner node id>

     {"error": "profile_held", "owner": "<node id>",
      "endpoint": "<owner's advertise_url, or null>", "retry_after_ms": <ms>}
     ```

     The gateway routes the user to `endpoint`, or retries after
     `retry_after_ms`; a dead owner's lease lapses within `lease_ttl_secs`.
  5. The request runs under that profile's context. A user's context cannot
     reach the operator plane.
- `GET /events?client_id=` with a user header streams only that profile's
  events (chat progress, `chat_done`, `channel_outbound`, ...). Without one it
  answers `404`, and browser bind tokens are not accepted in SaaS.
- Routes a SaaS core never serves answer `404`: `/v1`, `/events/*`, `/ws/*`,
  `/socket.io`, `/dev/connect` and `/oauth/*`.

## Credentials

The gateway installs each user's TinyHumans session JWT or API key with
`profiles.set_credential` (`ProfileRuntime::set_credential` in-process). It is
stored in the profile's own auth-profile store (scoped to the profile in a
storage backend), so any backend call made under that profile's context
resolves that user's credential and no other. The process-wide
`auth.set_credential` / `clear_credential` refuse in SaaS mode, because they
activate a user directory and rebind process globals. The core never
validates or echoes a credential. With `shared_backend_api_key`, users may
instead ride the operator's `OPENHUMAN_BACKEND_API_KEY`.

## Background work

A single-user core drains memory jobs from a cron system job behind the
process-wide scheduler gate. A SaaS core cannot use either: the cron service
is off, and the gate reflects the operator, who holds no credential. So
`background::spawn` (started by `saas::build`) runs one loop per process. Each
tick it sweeps idle profiles, then opens every provisioned profile whose
workspace has queued memory jobs (`memory::lifecycle::jobs::has_pending`) and
runs its due jobs under that profile's own context. Profiles with nothing
queued are not opened. User cron jobs are not served yet: the cron RPCs are
not on the user surface.

The prompt says `Host: hosted` instead of the server's hostname, and the
`## User` identity block stays empty, because no process-wide identity is ever
set.

## Files

| File | Purpose |
| --- | --- |
| `types.rs` | `ProfileId` and `ProfileIdMode`, `ProfileMeta`, and the operator-plane result types |
| `layout.rs` | The SaaS side of the shared `ProfileLayout`, archived profiles, and `profile_config`: the forced paths, memory binding (pinned to the legacy layout) and autonomy policy |
| `host.rs` | `ProfileHost`: provisioning, lazy open behind the lease, LRU and idle eviction (never of a profile in use), release, fencing, each profile's derived `CoreContext` and policy, `current()`, `ensure_hosted()` |
| `lease.rs` | The profile lease as the host uses it: `OpenError`, the lease store choice (`DocumentLeases` over a backend, else `LocalLeases`), and the heartbeat that renews every open profile's lease and fences the ones lost |
| `registry.rs` | `ProfileRegistry`: which profiles are provisioned, in the backend's `cluster` scope or as `profile.toml` files |
| `gateway.rs` | Which context a gateway request runs under: the operator plane, or the profile of the user named in `X-OpenHuman-User`, after the signature check; the typed refusal (`409` holder) |
| `surface.rs` | What a user may call: `USER_METHODS`, applied at dispatch, in the controller list and in `/schema`; user thread-id rules |
| `background.rs` | The SaaS background loop: idle sweeps and each profile's queued memory jobs under its context |
| `tools.rs` | Which agent tools a user gets: the host tool groups, the hard-deny list, the tool-list filter, the approval gate's SaaS verdict, and the container policy for a user's shell |
| `credentials.rs` | A profile's TinyHumans credential, stored beside its config or in the backend |
| `ops.rs` | `provision` / `deprovision` / `list` / `status` / `set_credential` / `clear_credential` / `release`, returning `Outcome<T>` |
| `schemas.rs` | The `profiles.*` controllers (`profile_id` in and out) |

Each file's tests sit beside it (`*_tests.rs`), including property suites for
ids (`types_proptest_tests.rs`) and the tenant key
(`core/runtime/tenant_proptest_tests.rs`). End to end:
`tests/saas_mode_e2e.rs` (the gateway, two users on one thread id, relayed
channel threads, `409`) and `crates/openhuman-embed/tests/saas_profiles.rs`
(the in-process `ProfileRuntime`).

## Known limits

- Thread JSONL, the local memory engine, the cost and run ledgers, background
  completions and the sandbox stay as files under `users/<id>/workspace`, so a
  multi-node deployment needs `<root>` on a shared read-write-many volume and
  a remote memory engine. Moving those onto the storage ports is a follow-up.
- Storage writes carry no epoch fence, so a node that is partitioned but still
  running could write after its lease expired. The mitigations are a TTL much
  longer than the renew interval, the fence check before each turn, and
  stopping turns when the lease is lost.
- Each node keeps its own operator keyring (`operator_dir`), so a credential
  installed through one node is only readable there unless secrets live in
  the storage backend.
- Existing `agents/u-*` data is orphaned.
