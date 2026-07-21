# Library benchmarking environment

## Purpose

"opencompany" wants to embed the OpenHuman Rust core as a library: no always-on
RPC server, no Tauri shell, just the core linked in-process and driven
directly. That changes what "resource usage" means. There is no single steady
process to profile; there are per-use-case workloads (a long-running agent
loop, a delegated multi-agent turn, a saved workflow run, a background
subconscious pass, a memory ingest, a bare embed) that each have their own
startup cost, steady-state footprint, and growth curve.

This document describes the benchmark environment built to measure that: a
pinned `library-profile` binary with seven scenarios, three driver scripts
under `scripts/profile/`, and the comparison point the team cares about
(ZeroClaw). It builds on the manual investigation in
[`docs/resource-profiling-session-2026-07-21.md`](resource-profiling-session-2026-07-21.md);
read that document for the deep memory/CPU attribution work. This document is
about running repeatable benchmarks, not re-deriving those findings.

## The seven scenarios

All scenarios run in `target/release/library-profile <scenario>`, replace
network inference with a deterministic provider (`rss-bench` feature), and
print one pretty-printed JSON result object to stdout (diagnostics go to
stderr). Each models a distinct embedding use case:

| Scenario | Models |
| --- | --- |
| `memory-ingest` | Canonicalizing and ingesting a batch of chat messages through the real extraction/admission/tree-queue pipeline. |
| `subagents` | A delegation turn: an orchestrator session spawns real subagents via `spawn_parallel_agents` and merges their findings. |
| `agent-turn` | The minimal embed case: one agent, one turn, no delegation, no workflow. The smallest useful "hello world" for a host that just wants a single reply. |
| `long-agent` | A long-running agent loop (`OPENHUMAN_PROFILE_TURNS`, default 25) in one process, to see whether RSS plateaus or grows per turn. |
| `workflow` | A saved automation run (`flows_create` + `flows_run`), representing the flows/automation embedding path rather than ad hoc chat. |
| `subconscious` | A background subconscious turn (the always-on reflective pass), distinct from an interactive chat turn. |
| `cold-phases` | Bootstrap attribution: per-phase checkpoints (config load, registry init, agent build, memory construction, first turn) so cold-start cost can be attributed to a phase instead of one lump sum. |

## How to run

Three scripts under `scripts/profile/` (each has `-h`/`--help`):

- **`library-bench.sh`** — the primary RSS/duration benchmark. Builds the
  binaries, runs each scenario N fresh-process repeats (default 5), and
  aggregates median/min/max into `summary.json` + `summary.md`.

  ```bash
  ./scripts/profile/library-bench.sh                     # default build, all scenarios
  ./scripts/profile/library-bench.sh --slim               # --no-default-features recipe
  ./scripts/profile/library-bench.sh --scenarios "long-agent,subagents" --turns 50 --warm
  ```

- **`library-cpu.sh`** — a `samply` wrapper for one scenario's CPU profile,
  isolated from persistence/timezone noise by default.

  ```bash
  ./scripts/profile/library-cpu.sh subagents
  samply load target/profile/rust-library/subagents-cpu.json.gz
  ```

- **`library-heap.sh`** — builds the `rss-bench-dhat` variant and runs a
  scenario under dhat for live-heap attribution (allocation sites, retained
  bytes). RSS/timing under dhat are perturbed; don't compare those numbers to
  `library-bench.sh` output.

  ```bash
  ./scripts/profile/library-heap.sh memory-ingest
  # load target/profile/rust-library/dhat-memory-ingest.json at
  # https://nnethercote.github.io/dh_view/dh_view.html
  ```

### Default vs slim builds

Default-feature builds link every compile-time domain gate (`voice`, `web3`,
`media`, `meet`, `skills`, `flows`, `mcp`, `tui`) — the byte-identical desktop
recipe. The slim recipe drops everything not required by the harness:

```bash
GGML_NATIVE=OFF cargo build --release \
  --no-default-features --features rss-bench \
  --bin library-profile --bin rss-bench
```

`--slim` on `library-bench.sh` builds this recipe. Per the prior session,
compile-time gates shrink the binary substantially but only move settled RSS
by a few MiB — most of the RSS story is initialization and allocator
behavior, not linked code size.

### Useful env knobs

| Variable | Effect |
| --- | --- |
| `OPENHUMAN_PROFILE_TURNS` | Turn count for `long-agent` (default 25). |
| `OPENHUMAN_PROFILE_PREWARM_SUBAGENTS=1` | Run one warm-up turn before measuring (`subagents`/`subconscious`), isolating first-use cost from steady state. |
| `OPENHUMAN_PROFILE_DISABLE_MEMORY_WRITES=1` | Disable `memory.auto_save` and episodic capture, isolating orchestration from persistence. |
| `OPENHUMAN_PROFILE_FORCE_UTC=1` | Skip `iana_time_zone`/CoreFoundation timezone resolution. |
| `OPENHUMAN_PROFILE_HOLD_SECS` / `HOLD_BEFORE_SECS` | Pause the process at settled/baseline state for external inspection (`vmmap`, `heap`, `malloc_history`, Instruments). |
| `OPENHUMAN_PROFILE_DHAT_OUT` | Output path for dhat JSON (set by `library-heap.sh`). |

## Metrics and interpretation

Every run reports `baseline`/`settled`/`peak_rss_kib` (macOS: `proc_pid_rusage`
RSS, `getrusage` peak, `proc_pidinfo` thread count), `retained_delta_kib`
(settled minus baseline), `peak_delta_kib`, and `duration_ms`. `long-agent`
additionally reports `checkpoints[]` so first-turn vs. last-turn growth is
visible directly.

**RSS is not private heap.** The prior session's deep attribution found a
~42 MiB slim-build snapshot broken down as roughly 15.2 MiB private physical
footprint, 3.18 MiB live heap, 18.7 MiB resident executable text, and ~9.4 MiB
of resident-but-mostly-inactive malloc pages (allocator high-water
retention). See
[`docs/resource-profiling-session-2026-07-21.md`](resource-profiling-session-2026-07-21.md#deep-memory-attribution)
for the full breakdown, the executable-paging finding (a cold turn faults in
~15 MiB of previously nonresident OpenHuman code), and the warmed-process
control showing steady-state turns cost ~0.5-1.9 MiB once warm rather than
the ~26-31 MiB a cold turn costs. Use `library-bench.sh` for the RSS/duration
headline numbers, `library-cpu.sh` when CPU attribution is the question, and
`library-heap.sh` only when RSS numbers need live-allocation attribution
(accepting the dhat perturbation).

## The ZeroClaw comparison

ZeroClaw is reported to idle under 5 MiB RAM and use approximately 7.8-12 MiB
under load. OpenHuman's Rust core currently settles around 35-50 MiB
depending on scenario and feature set (see the baseline table below).

Treat this as a **north star, not an apples-to-apples benchmark**. ZeroClaw's
scope and feature set differ substantially from the OpenHuman core: OpenHuman
links a full agent/memory/tool/orchestration stack (SQLite-backed unified
memory, TinyCortex PII detection, prompt-injection detection, a builtin-agent
registry, tool catalogs, provider routing) that a narrower harness may not
carry at all. A closer gap is a meaningful signal that the initialization
graph is leaner; it is not evidence of feature parity, and a wider gap is not
automatically a regression if it comes from carrying more capability. Every
`library-bench.sh` summary includes a labeled comparison row/note for exactly
this reason: visible, but explicitly called out as external.

## Profiling escalation path

Start cheap, escalate only as needed:

1. **`library-bench.sh`** — RSS/duration medians across fresh processes. Answers "did this change move the needle" for most changes.
2. **`library-cpu.sh` (samply)** — symbolized CPU profile when a scenario is slower than expected, or to attribute cold-path CPU to a specific phase (registry init, agent build, memory construction, SQLite init, TinyAgents turn runner were the top contributors in the prior session).
3. **`library-heap.sh` (dhat)** — live-heap allocation sites and retained bytes when RSS is high but the cause isn't obvious from CPU alone (e.g. the TinyCortex PII `RegexSet` finding came from stack-logged allocation attribution, not CPU sampling).
4. **Instruments / `vmmap` / `heap` / `malloc_history`** — deepest macOS-native attribution, using the `OPENHUMAN_PROFILE_HOLD_SECS` / `HOLD_BEFORE_SECS` hooks to pause the process at baseline or settled state:

   ```bash
   OPENHUMAN_PROFILE_HOLD_SECS=120 target/release/library-profile subagents &
   vmmap -summary <pid>
   heap -sH <pid>

   MallocStackLogging=1 OPENHUMAN_PROFILE_HOLD_SECS=120 \
     target/release/library-profile subagents &
   malloc_history <pid> -allBySize
   ```

   This is what surfaced the PII-sanitizer regex cache and the first-turn
   executable-paging finding in the prior session; reach for it only once
   `library-bench.sh`/`library-cpu.sh`/`library-heap.sh` have narrowed the
   question to a specific scenario and metric.

## Current baseline numbers

From the 2026-07-21 profiling session (medians over five fresh processes
unless noted; see that document for methodology and caveats):

| Scenario | Build | Median settled RSS | Median retained Δ |
| --- | --- | ---: | ---: |
| 1-agent roster | default | 38.7 MiB | - |
| 8-agent roster | default | 41.5 MiB | +2.8 MiB total (~0.40 MiB/agent) |
| 1-agent roster | slim | 35.5 MiB | - |
| 8-agent roster | slim | 38.7 MiB | +3.2 MiB total |
| `memory-ingest` (100 msgs) | default | 25.5 MiB | 9.31 MiB |
| `memory-ingest` (100 msgs) | slim | 23.8 MiB | 8.58 MiB |
| `subagents` (cold, 2 children) | default | 48.5 MiB | 30.8 MiB |
| `subagents` (cold, 2 children) | slim | 42.4 MiB | 25.7 MiB |
| `subagents` (warmed repeat, persistence off) | default | - | 0.52 MiB |
| `subagents` (warmed repeat, normal capture) | default | - | 1.84 MiB |
| ZeroClaw (external, idle) | - | < 5 MiB | - |
| ZeroClaw (external, under load) | - | 7.8-12 MiB | - |

`long-agent`, `workflow`, `subconscious`, `agent-turn`, and `cold-phases` are
new scenarios in the pinned contract; run `library-bench.sh` to populate their
baseline rows once the binaries land, then update this table.

## See also

- [`docs/resource-profiling-session-2026-07-21.md`](resource-profiling-session-2026-07-21.md) — the full manual investigation (deep attribution, cold-path CPU, library-design implications, recommended optimization order).
- [`scripts/profile/README.md`](../scripts/profile/README.md) — script quick reference.
- `src/bin/library_profile/main.rs` — the scenario implementations.
