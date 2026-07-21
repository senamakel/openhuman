# `scripts/profile/`

Reproducible benchmarking scripts for the OpenHuman Rust core as an embedded
library (no RPC server), built around the `library-profile` and `rss-bench`
binaries (see `src/bin/library_profile/main.rs`). Full write-up:
[`docs/library-benchmarking.md`](../../docs/library-benchmarking.md). Prior
findings: [`docs/resource-profiling-session-2026-07-21.md`](../../docs/resource-profiling-session-2026-07-21.md).

## Scripts

### `library-bench.sh` — RSS/duration benchmark

Builds `library-profile` + `rss-bench`, runs each scenario N times as a fresh
process, and aggregates median/min/max duration, settled RSS, retained delta,
and peak delta into `summary.json` + `summary.md`.

```bash
./scripts/profile/library-bench.sh                              # all 7 scenarios, default build, 5 repeats
./scripts/profile/library-bench.sh --slim --repeat 7             # slim (no-default-features) build
./scripts/profile/library-bench.sh --scenarios "long-agent,subagents" --turns 50 --warm
```

Results land in `target/profile/rust-library/bench-<timestamp>/` (or `--out DIR`).

### `library-cpu.sh` — CPU profile via samply

Wraps `samply record` around one scenario, isolated from persistence/timezone
noise by default (matching the documented cold-path CPU recipe).

```bash
./scripts/profile/library-cpu.sh subagents
./scripts/profile/library-cpu.sh long-agent -- OPENHUMAN_PROFILE_TURNS=50
samply load target/profile/rust-library/subagents-cpu.json.gz
```

### `library-heap.sh` — live heap attribution via dhat

Builds the `rss-bench-dhat` variant and runs one scenario under dhat. RSS and
timing numbers from this build are perturbed by instrumentation; use it only
for allocation-site/retained-bytes attribution, not for RSS comparisons.

```bash
./scripts/profile/library-heap.sh memory-ingest
# open https://nnethercote.github.io/dh_view/dh_view.html and load
# target/profile/rust-library/dhat-memory-ingest.json
```

## Quick start

```bash
# 1. Baseline RSS/duration across all scenarios
./scripts/profile/library-bench.sh

# 2. CPU attribution for the slowest/most interesting scenario
./scripts/profile/library-cpu.sh subagents

# 3. If a scenario's RSS looks off, drill into live heap
./scripts/profile/library-heap.sh subagents
```

All scripts require `jq` for JSON parsing/aggregation; `library-cpu.sh` also
requires `samply` (`cargo install samply`). Build commands use
`GGML_NATIVE=OFF` to work around the Apple Silicon whisper-rs/llama.cpp NEON
fp16 build issue.
