---
description: >-
  The storage domain in crates/openhuman-core/src/storage/: the ports, URL
  resolution, Cargo features, per-agent scopes, drivers and on-disk layout.
icon: database
---

# Storage

OpenHuman keeps durable state on the ports of [`tinystoragedrivers`](https://github.com/tinyhumansai/tinystoragedrivers), vendored through `vendor/tinyagents/vendor/tinystoragedrivers`. One URL picks the backend for the whole process. The host adapter is `crates/openhuman-core/src/storage/`. It resolves the URL, opens the backend, holds it in a process slot and decides which scope a call runs in. The ports, the drivers and their conformance suites belong to `tinystoragedrivers`, and the typed records stay with the domain that owns them.

With no URL configured, which is the desktop default, nothing here is opened and every domain keeps its classic on-disk layout under the workspace. Setting a URL switches the domains that have moved onto the ports. The rest still use their files.

## The ports

A backend is a `StorageBackend`. It hands out a `ScopedStorage` for each `Scope`, and every handle on it is already bound to that scope, so no method takes a scope argument.

| Port            | Holds                                                                                                                                                      | Used for                                                                                                                    |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `DocumentStore` | Versioned JSON documents in named collections, with filters, paging, unique indexes, compare-and-swap (`Precondition`), atomic `claim` and optional expiry | Approvals, devices, notifications, task sources, cron jobs and runs, the flow catalog, graph checkpoints, encrypted secrets |
| `StreamStore`   | Append-only logs with dense offsets                                                                                                                        | Transcripts, journals                                                                                                       |
| `BlobStore`     | Opaque bytes by key                                                                                                                                        | Attachments, artifacts                                                                                                      |
| `SearchIndex`   | Full-text membership                                                                                                                                       | Where the driver supports it                                                                                                |
| `SecretStore`   | Encrypted values (`enc2:` envelope)                                                                                                                        | Keyring user secrets and the credential stores                                                                              |

The core's stores are mostly synchronous, so `storage::block_on` runs a storage future on one shared runtime thread. It never uses `block_in_place`, which panics on a current-thread runtime. `storage::documents::Repo` is the base the domain stores build on. It declares a domain's collections once per process and runs each call.

## Choosing a backend

The storage URL is the first of these that is set. Blank values count as unset.

1. `OPENHUMAN_STORAGE_URL`
2. `[storage] url` in `config.toml`
3. Nothing: the classic layout

`[storage]` is bootstrap configuration. It is always read from the environment or the file and never from storage itself.

| URL                                                                    | Driver                                                                                                                                                                                                               | Cargo feature     |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| `memory`                                                               | In-process maps, keeps nothing                                                                                                                                                                                       | Always            |
| `sqlite:<path>` (a `.db` file, or a directory that holds `storage.db`) | SQLite                                                                                                                                                                                                               | `storage-sqlite`  |
| `file:<dir>`                                                           | `<dir>/scopes/<scope>/docs/<collection>/<id>.json` for documents, with collection metadata in `<dir>/_meta/collections/<collection>.json`. Streams are JSONL and blobs are raw files under the same scope directory. | `storage-file`    |
| `mongodb://…/<db>`, `mongodb+srv://…/<db>`                             | MongoDB, one database shared by every scope                                                                                                                                                                          | `storage-mongodb` |

A URL for a driver the build lacks fails at `storage::open`, and the error names the Cargo feature. A misconfigured deployment therefore stops at boot rather than at its first write. Credentials in a URL are redacted in logs and in `Debug` output.

Each feature is forwarded through the whole library chain (`openhuman-core`, `openhuman-embed`, `openhuman-tinyhumans`, `openhuman-rpc`, then the `openhuman-cli` and `openhuman-tui` hosts). `scripts/ci/check-feature-forwarding.mjs` checks the links. A desktop build never links a MongoDB client, and a cloud build does not need SQLite.

The host opens the configured backend before the core boots and installs it with `storage::install`. It then installs TinyAgents' `DriverSessionStores` over the same backend, so transcripts, turn states, the run ledger and journals live in it too. Library hosts use `RuntimeBuilder::storage(..)`, which installs only the backend: conversations move onto it with `RuntimeBuilder::session_store(..)`, configured separately.

## Scopes

Every record lives in a scope, and two scopes on one backend never see each other's data. A single-user install scopes by **agent id**, and a SaaS host scopes by **profile** (the tenant). `storage::scope_for_agent` and `storage::scope_for_profile` are the single mappings, and the session store uses the same ones, so every domain agrees. An agent's scope is its id when that is a valid scope name, else `sha256:<hex digest of the id>` (TinyAgents' `DriverSessionStores::scope_for`). A profile's scope is `profile:<id>`, or `profile-sha256:<hex digest of the id>` when that is not a valid scope name. The prefixes keep the two kinds of key from colliding, and `local` is the literal operator scope.

`storage::current_scope()` resolves the scope of the current call from the task's tenant (`storage::scope_from(profile, agent, saas)`), in this order:

1. A **profile**, when the work runs for one: the scope is the profile's, whatever agent acts inside it. In SaaS mode the agents of one profile therefore share that profile's scope.
2. Otherwise the acting **agent** (`CoreContext::session_agent`): the scope is that agent's.
3. Otherwise `local`: the operator, the CLI and the desktop shell in single-user mode.

In SaaS mode a call with no profile is refused instead of falling into a bucket other users could share, whether or not an agent acts. If the task's tenant cannot be resolved at all (`current_tenant()` fails), `current_scope()` returns that error in every mode instead of falling back to `local`.

A backend decides how it enforces the scope. SQLite and the file driver keep each scope's data apart, and `local` is the scope a single-user install uses. MongoDB injects a scope key into every filter and every index prefix.

### Background work

Background work runs under the process default context, which names no agent, and on its own would only see `local`. `storage::agents` closes the gap. Agents are known when they are live (`AgentContextRegistry`) or when an earlier process recorded their id in the `local` scope (`storage_agents`). `for_each_scope` runs a step once for `local` and once under each known agent. The cron scheduler's agent pass (`cron::scheduler::tick_live_agents`, driven in tests through the public `cron::scheduler::run_live_agent_pass`) runs each live agent's due jobs under that agent's own context, so their results are recorded in the agent's own scope. A recorded agent that is not live waits until it is live again, because its jobs need its host tools and prompt.

On a shared backend (MongoDB) other processes may write the same records, so boot-time recovery such as the orphaned-run sweep is skipped (`storage::installed_is_shared`).

## Drivers and on-disk layout

| Driver             | Where the data lives                                                                                                                                                                                                 |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `memory`           | Nowhere. Records last as long as the backend value.                                                                                                                                                                  |
| `sqlite:<dir>`     | `<dir>/storage.db` in WAL mode (with its `-wal` and `-shm` files). The scope is a column on every row. The large relational stores keep their tables through the driver's native mode.                               |
| `sqlite:<file>.db` | That file, laid out the same way.                                                                                                                                                                                    |
| `file:<dir>`       | `<dir>/scopes/<scope>/docs/<collection>/<id>.json` for documents, with collection metadata in `<dir>/_meta/collections/<collection>.json`. Streams are JSONL and blobs are raw files under the same scope directory. |
| `mongodb://…/<db>` | One database. Each named database is a collection prefix, and every record carries its scope.                                                                                                                        |

Without a URL, the classic layout is unchanged: per-domain SQLite databases (`approval/approval.db`, `devices/devices.db`, `notifications/notifications.db`, `task_sources/sources.db`, `cron/jobs.db`, `flows/flows.db`, `graph_checkpoints.db`), JSON and JSONL files, and the OS keyring or `secrets.enc`. Records written to a configured backend are not copied back to those files. The domain stores do not import their files either, with one exception: credentials. On first read with a backend installed, the auth-profile and HTTP-credential stores copy the records in their files into the backend, and the keyring adopts a secret it finds in the process backend. The source files are left untouched.

Secrets with a backend installed are encrypted documents in the current storage scope: the acting agent's scope on a desktop or CLI host, and the profile's scope under SaaS, where every agent in one profile shares the profile's credentials and data key. Each scope's data key is derived with HKDF-SHA256 from the keyring master key (`OPENHUMAN_KEYRING_MASTER_KEY` or `_FILE`, else the OS keychain), and with no master key they fail closed. The config encryption key stays on the process keyring because `config.toml` is loaded before any agent acts.

## Tests and CI

Every storage test target runs the same body once per driver, and each run is a separate test case named after the driver (`memory::…`, `sqlite::…`, `file::…`, `mongodb::…`), so a failure says which driver broke. The shared helper is `tests/support/storage_drivers.rs`.

| Driver           | Runs                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------- |
| `memory`         | Always                                                                                                  |
| `sqlite`, `file` | When `storage-sqlite` or `storage-file` is compiled in, on temp directories                             |
| `mongodb`        | When `storage-mongodb` is compiled in and `TSD_MONGO_URL` is set. Each case uses a database of its own. |

The targets are `storage_approvals_e2e`, `storage_domains_e2e`, `storage_flows_e2e`, `storage_secrets_e2e`, `storage_scope_e2e`, `storage_agent_scopes_e2e`, `storage_delegation_e2e` and `cli_storage_url_e2e`. Run one locally with the drivers you want:

```bash
cargo test -p openhuman-cli --features storage-sqlite,storage-file --test storage_approvals_e2e
```

For MongoDB, start a single-node replica set and set the URL (a replica set is what lets the driver report and exercise transactions):

```bash
bash vendor/tinyagents/vendor/tinystoragedrivers/.github/scripts/start-mongo-replset.sh 27017
export TSD_MONGO_URL='mongodb://127.0.0.1:27017/tsd_test?directConnection=true'
cargo test -p openhuman-cli --features storage-mongodb --test storage_approvals_e2e
```

CI has two lanes for this:

- The `storage-drivers` lane in `scripts/ci/self-hosted/lanes-plan.mjs` builds with `storage-sqlite,storage-file`. It runs every storage target plus the lib tests of the domains on the ports. The `storage` area in `.github/ci-paths-filter.yml` arms it when storage, cron, flows, approvals, devices, notifications, task sources, keyring or credentials, orchestration, the cost tracker, app and desktop-control state or the rpc session store change.
- `.github/workflows/storage-mongodb.yml` builds with `storage-mongodb` against a real replica set. It runs on pull requests that touch the same paths, nightly on `main` and on demand.

## Where to read next

- `crates/openhuman-core/src/storage/README.md` for the entry points and the consumers.
- The `tinystoragedrivers` specs: `docs/specs/storage-ports.md` and `docs/plans/storage-rollout.md` in `vendor/tinyagents/vendor/tinystoragedrivers`.
- [Security](security.md) for the keyring and credential stores.
