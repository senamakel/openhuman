# CI Fast: the lane flow and the EX63 runners

CI Fast runs every pre-merge check as parallel **lanes** in fewer jobs. It is
the whole pull-request flow: the `ci-lite.yml` workflow it was originally built
alongside was deleted on 2026-10-01. The one check to require is **CI Gate**
(see [CI Gate](#ci-gate)).

| Who opened the PR         | Workflow                              | Where it runs                                                             |
| ------------------------- | ------------------------------------- | ------------------------------------------------------------------------- |
| `tinyhumansai` org member | `ci-fast.yml` (`pull_request_target`) | one job on a throwaway Firecracker microVM on the Hetzner EX63            |
| anyone else               | `ci-fast-hosted.yml` (`pull_request`) | the same lanes split over GitHub-hosted jobs, with GitHub's Actions cache |

Both call `.github/workflows/ci-lanes.yml`, which runs
`scripts/ci/self-hosted/lanes.mjs`. The plan itself is in
`scripts/ci/self-hosted/lanes-plan.mjs`, and
`scripts/__tests__/self-hosted-lanes.test.mjs` pins its shape.

The host side (microVM supervisor, guest image, firewall, deploy) lives in
the private repo `tinyhumansai/gh-hosted-runner`. Its README covers
provisioning, deploys and the runner token.

## Lanes

| Lane             | What runs                                                                                                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `static`         | fmt, layout, runtime boundary, ignored-tests, TLS policy, gated-test allowlist, orch-ip gate, feature forwarding, module pins and monotonicity, toolchain drift, test inventory |
| `frontend`       | pnpm install, tsc, prettier, eslint, i18n, docs, script self-tests                                                                                                              |
| `frontend-tests` | the complete vitest suite with coverage                                                                                                                                         |
| `rust-cov`       | test modules from the registry, then `scripts/ci/rust-coverage.sh`                                                                                                              |
| `rust-lint`      | clippy (product set; embed's clippy covers the core's contributor set), embed and tinyhumans lint, embed gates-off check                                                        |
| `rust-gates-off` | gates-off checks and gate-contract tests, kernel floor, dep-sim calibration                                                                                                     |
| `tauri`          | Tauri clippy and coverage                                                                                                                                                       |
| `pester`         | `install.ps1` tests                                                                                                                                                             |

How lanes behave:

- **Selection is by area only.** The changed-area filters in
  `.github/ci-paths-filter.yml`, which CI Lite also uses, decide whether a
  whole suite runs. Nothing narrows a suite to the changed files.
- **Nothing stops early.** A failed check never stops the checks after it
  in its lane, and never stops other lanes. Inside the coverage check,
  `scripts/ci/rust-coverage.sh` runs every crate and integration target even
  after one fails, merges the lcov report from what ran, and then lists every
  failure. A check whose dependency failed reports `blocked`. Only a
  cancellation stops a run.
- **One workflow step per lane.** `Start lanes` launches every lane in the
  background, so they still run in parallel. Each `Lane: …` step streams its
  own lane's log live, with one folded group per check, and passes or fails
  on that lane alone. `Lane summary` carries the overall result.
- **Changed-line coverage** must be at least 80% through
  `scripts/ci/self-hosted/diff-cover.sh`, the same gate as `PR CI Gate`.

Some checks do not run on pull requests. CI Lite runs them on every push to
`main` that touches the Rust core:

- the core doctests, and the coverage of `openhuman-tui` (each a core build of
  its own);
- the TinyJuice host-module regression;
- `cargo test -p openhuman-embed` / `-p openhuman-tinyhumans` with default
  features, since the coverage lane already runs both crates' tests with the
  product features;
- `cargo check -p openhuman --no-default-features`, which
  `embed-check-no-default` already covers: it builds the core with the same
  (empty) feature set;
- `cargo check -p openhuman --no-default-features --features
e2e-test-support`: `rust-gates-off` already compiles that feature set for
  its tests, in one build together with `mcp`.

## CI Gate

`.github/workflows/ci-gate.yml` posts one commit status, `CI Gate`, on each PR
head commit. It is the check branch protection should require, whichever flow
ran the PR. It is re-evaluated whenever CI Fast, CI Fast (hosted) or CI Lite
starts or finishes, from the decisive job of each:

| Flow             | Decisive job                    |
| ---------------- | ------------------------------- |
| CI Fast          | `Lanes / CI Fast (EX63)`        |
| CI Fast (hosted) | `Lanes / CI Fast (hosted) Gate` |
| CI Lite          | `PR CI Gate`                    |

- **success** as soon as one of those jobs passes. The GitHub-hosted runs
  still working on that commit are then cancelled. The EX63 run is never
  cancelled.
- **pending** while nothing has passed and a flow is still running. An EX63
  failure does not fail the gate while CI Lite is still running.
- **failure** once every flow has finished without a pass.

A skipped job is never a pass. That is why CI Lite skips `PR CI Gate` along
with everything else when the EX63 runs the commit.

Who runs what:

- **Org members:** `ci-fast.yml` on `pull_request_target`, routed by
  `scripts/ci/ci-fast-route.sh` (shared with `ci-fast-hosted.yml`). The route
  decision falls back to the hosted lanes when no EX63 run starts within three
  minutes, or when routing errors.
- **Outsiders:** CI Lite and CI Fast (hosted) both run, and the first to pass
  cancels the other.
- **Pushes to `main`:** CI Lite runs, and the gate ignores them.

The workflow runs from the default branch through `workflow_run`, with
`statuses: write` and `actions: write`. It never checks out PR code; the
decision (`scripts/ci/ci-gate.mjs`, tested in
`scripts/__tests__/ci-gate.test.mjs`) reads run and job state from the API.
`CI_GATE_DRY_RUN=1` with `GH_TOKEN`, `REPO` and `HEAD_SHA` prints the verdict
for any commit without posting it.

## Profiles

- **`ex63`**: every lane at once on one VM (10 vCPU, 28 GiB).
  - Each Rust lane has its own target dir on the per-job scratch disk, so
    lanes never wait on cargo's build lock.
  - Every Rust build, the instrumented coverage builds included, goes
    through sccache. Its cache is one store on the host shared by both VMs,
    capped at 8 GB on a 10 GB disk. If the store is down, sccache falls back
    to the VM's own cache disk.
  - sccache keys include the target dir, so a lane reuses the same lane's
    work from any earlier job, not other lanes' work.
  - The cargo registry and pnpm store live on a 10 GB cache disk per slot.
  - The step summary reports sccache's Rust hit rate.
  - Target dirs are not kept between jobs.
  - vitest runs with 8 workers.
- **`hosted`**: lanes grouped into jobs sized for 4-core, ~14 GB runners:
  `checks`, `rust-lint` (lint and gates-off in turn), `rust-cov`, `tauri`
  and `pester`. Groups whose areas are untouched don't start.

## Why the EX63 cannot run outsider code

- `ci-fast.yml` runs from `main` (`pull_request_target`), so a PR cannot
  edit its routing.
- The runner group `openhuman-ex63` only admits `ci-fast.yml` and
  `ci-lanes.yml` at `refs/heads/main`. A fork's own `pull_request` workflow
  can't claim the runners even if it names their label.
- The host supervisor re-checks the run's actors against org membership and
  kills the VM for a non-member.

**Optional repo secret `CI_MEMBERSHIP_TOKEN`:** a fine-grained token with only
_Organization → Members: Read_. It lets the route job recognise _private_ org
members. Without it, only the PR's `author_association` is used, and private
members may land on GitHub-hosted runners instead.

## Measuring

```bash
node scripts/ci/self-hosted/compare-runs.mjs            # wall clock per commit
node scripts/ci/self-hosted/compare-runs.mjs --lanes    # plus per-lane times, sccache hits
```

Each run uploads `ci-out-*` artifacts with per-lane logs, `ci-timings.json`
(per-check timings, target-dir sizes, sccache stats) and lcov.

## Local use

```bash
CI_AREA_RUST_CORE=true CI_SCRATCH_DIR=/tmp/x node scripts/ci/self-hosted/lanes.mjs --profile ex63 --dry-run
CI_AREA_DOCS=true node scripts/ci/self-hosted/lanes.mjs --profile hosted --lanes static,frontend
```
