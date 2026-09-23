// The lane plan behind ci-fast.yml / ci-fast-hosted.yml (via ci-lanes.yml).
//
// Pure data: `buildPlan()` turns a profile and the changed-area flags into
// lanes of named checks. `lanes.mjs` executes the plan. Keeping the plan pure
// lets scripts/__tests__/self-hosted-lanes.test.mjs pin its shape without
// running a single cargo command.
//
// Every check here is a check ci-lite.yml runs, moved as-is. Two rules hold for
// all of them:
//
//   * An area flag decides whether a WHOLE suite runs. No command is ever
//     narrowed to changed files: commands are static strings that never
//     interpolate the diff (the self-test asserts it).
//   * Every check in a lane runs even when an earlier one failed (#6451), and
//     the lane still fails. `needs` names only real data dependencies.

/** Changed-area flags, as exported by the workflow from dorny/paths-filter. */
export const AREA_ENV = {
  frontend: "CI_AREA_FRONTEND",
  i18n: "CI_AREA_I18N",
  docs: "CI_AREA_DOCS",
  rustCore: "CI_AREA_RUST_CORE",
  rustTauri: "CI_AREA_RUST_TAURI",
  scripts: "CI_AREA_SCRIPTS",
  inventory: "CI_AREA_INVENTORY",
  toolchainImage: "CI_AREA_TOOLCHAIN_IMAGE",
  installPs1: "CI_AREA_INSTALL_PS1",
};

/** Read the area flags from an environment object. Missing means false. */
export function areasFromEnv(env) {
  const areas = {};
  for (const [key, name] of Object.entries(AREA_ENV)) {
    areas[key] = env[name] === "true";
  }
  return areas;
}

/** Hosted-runner job groups: lanes that share one GitHub-hosted job. */
export const HOSTED_GROUPS = [
  // Quick checks and the frontend suite fit one 4-core runner side by side.
  {
    group: "checks",
    lanes: ["static", "frontend", "frontend-tests"],
    maxParallel: 3,
    container: true,
  },
  // Lint and gates-off share one `target/` on a ~14 GB disk, so run in turn.
  {
    group: "rust-lint",
    lanes: ["rust-lint", "rust-gates-off"],
    maxParallel: 1,
    container: true,
  },
  { group: "rust-cov", lanes: ["rust-cov"], maxParallel: 1, container: true },
  { group: "tauri", lanes: ["tauri"], maxParallel: 1, container: true },
  // pwsh ships on the bare ubuntu-latest image, not in the CI container.
  { group: "pester", lanes: ["pester"], maxParallel: 1, container: false },
];

const PRODUCT = '"$(bash scripts/ci/product-features.sh)"';

/**
 * Build the lane plan.
 *
 * @param {object} opts
 * @param {"ex63"|"hosted"} opts.profile
 * @param {Record<string, boolean>} opts.areas  from areasFromEnv()
 * @param {object} opts.env  process environment (profile paths only)
 * @param {boolean} [opts.isPullRequest]
 * @returns {{profile: string, lanes: Lane[]}}
 */
export function buildPlan({ profile, areas, env = {}, isPullRequest = true }) {
  if (profile !== "ex63" && profile !== "hosted") {
    throw new Error(`unknown profile "${profile}" (expected ex63 or hosted)`);
  }
  const ex63 = profile === "ex63";
  const scratch = env.CI_SCRATCH_DIR;
  if (ex63 && !scratch) {
    throw new Error(
      "profile ex63 needs CI_SCRATCH_DIR (set by the microVM guest)",
    );
  }
  const rust = areas.rustCore || areas.rustTauri;
  const core = areas.rustCore;

  // ex63: one throwaway target dir per lane so lanes never queue on cargo's
  // build-dir lock; sccache warms the build from the host's shared, capped
  // store (or the slot's /cache disk when the store is down). sccache keys
  // include the target dir, so a lane shares with the same lane of every
  // earlier job on any VM, not with the other lanes.
  // hosted: cargo's default target dirs, which Swatinem/rust-cache restores.
  const targetDir = (lane) => (ex63 ? `${scratch}/target/${lane}` : null);
  const sccache = ex63 ? { RUSTC_WRAPPER: "sccache" } : {};
  const rustEnv = {
    CARGO_INCREMENTAL: "0",
    RUSTFLAGS: "-C link-arg=-fuse-ld=mold",
  };
  // Instrumented builds: no DWARF, a large test stack, and on hosted the
  // serialized build ci-lite needs to fit the runner's disk. On ex63 they go
  // through sccache too: cargo-llvm-cov chains an existing RUSTC_WRAPPER in
  // both its wrapper and RUSTFLAGS modes, and cache hits give byte-identical
  // lcov (checked with cargo-llvm-cov 0.8 and sccache 0.10). Hosted keeps
  // ci-lite's wrapper-free setup.
  const covEnv = {
    ...rustEnv,
    CARGO_PROFILE_DEV_DEBUG: "0",
    RUST_MIN_STACK: "67108864",
    ...(ex63 ? sccache : { CARGO_BUILD_JOBS: "1" }),
  };
  const modulesDir = ex63
    ? `${scratch}/test-modules`
    : "/opt/openhuman-test-modules/${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}";
  const modulesEnvFile = "ci-out/test-modules.env";
  const withModules = (cmd) =>
    `set -a && . ${modulesEnvFile} && set +a && ${cmd}`;

  // Not run on pull requests: the core doctests (an uninstrumented core build
  // of their own), openhuman-tui's coverage (a core build with default
  // features, just for it) and the TinyJuice host-module regression (one test
  // against the downloaded module). CI Lite runs all three on every push to
  // `main` that touches the Rust core (rust-coverage.sh and its
  // rust-core-coverage job).

  /** @type {Lane[]} */
  const lanes = [
    {
      name: "static",
      checks: [
        { name: "rust-fmt", when: rust, run: "cargo fmt --all -- --check" },
        {
          name: "rust-layout",
          when: core,
          run: "node scripts/ci/check-openhuman-rust-layout.mjs",
        },
        {
          name: "agent-runtime-boundary",
          when: core,
          run: "pnpm agent:runtime-boundary",
        },
        {
          name: "ignored-tests-ratchet",
          when: rust,
          run: "pnpm rust:ignored-tests",
        },
        {
          name: "linux-tls-policy",
          when: rust,
          run: "bash scripts/check-linux-tls-dependencies.sh",
        },
        {
          name: "gated-test-allowlist",
          when: rust,
          run: "bash scripts/ci/check-gated-test-allowlist.sh",
        },
        {
          name: "orch-ip-gate",
          when: true,
          run: "bash scripts/ci/orch-ip-gate.sh",
        },
        {
          name: "feature-forwarding",
          when: true,
          run: "node scripts/ci/check-feature-forwarding.mjs",
        },
        {
          name: "module-pins",
          when: true,
          run: "bash scripts/ci/fetch-submodule-tags.sh && node scripts/ci/check-module-pins.mjs",
        },
        {
          name: "submodule-monotonic",
          when: isPullRequest,
          needs: ["module-pins"],
          run: "node scripts/ci/check-submodule-monotonic.mjs",
        },
        {
          name: "toolchain-image-drift",
          when: areas.toolchainImage,
          run: "node scripts/ci/check-toolchain-image.mjs",
        },
        {
          name: "test-inventory",
          when: areas.inventory,
          run: "pnpm test:inventory",
        },
      ],
    },
    {
      name: "frontend",
      env: { NODE_ENV: "test" },
      checks: [
        {
          name: "pnpm-install",
          when: areas.frontend || areas.i18n || areas.scripts,
          run: "pnpm install --frozen-lockfile",
        },
        {
          name: "tsc",
          when: areas.frontend,
          needs: ["pnpm-install"],
          run: "pnpm --filter openhuman-app compile",
        },
        {
          name: "prettier",
          when: areas.frontend,
          needs: ["pnpm-install"],
          run: "pnpm --filter openhuman-app format:check",
        },
        {
          name: "eslint",
          when: areas.frontend,
          needs: ["pnpm-install"],
          run: "pnpm --filter openhuman-app lint",
        },
        {
          name: "i18n",
          when: areas.i18n,
          needs: ["pnpm-install"],
          run: "pnpm i18n:check",
        },
        {
          name: "docs-generator-tests",
          when: areas.docs,
          run: "pnpm docs:test",
        },
        { name: "docs-drift", when: areas.docs, run: "pnpm docs:check" },
        {
          name: "scripts-self-tests",
          when: areas.scripts,
          needs: ["pnpm-install"],
          run: "pnpm test:scripts",
        },
      ],
    },
    {
      // Split from `frontend` so the long vitest run overlaps the lint checks.
      name: "frontend-tests",
      env: { NODE_ENV: "test", VITEST_MAX_WORKERS: ex63 ? "8" : "3" },
      checks: [
        {
          name: "vitest-coverage",
          when: areas.frontend,
          needs: ["frontend:pnpm-install"],
          run:
            "pnpm --filter openhuman-app test:coverage" +
            " && test -f app/coverage/lcov.info" +
            " && sed -E -e 's#^SF:(src/)#SF:app/\\1#' -e 's#^SF:(\\./src/)#SF:app/src/#'" +
            " app/coverage/lcov.info > ci-out/lcov/lcov-frontend.info",
        },
      ],
    },
    {
      // First among the Rust lanes: it is the long pole, and rust-lint waits
      // on its test-modules check.
      name: "rust-cov",
      // Compiles the core crate: holds one of the VM's heavy-compile slots
      // (lanes.mjs). Lower number = served first.
      heavy: 0,
      targetDir: targetDir("cov"),
      env: covEnv,
      checks: [
        {
          name: "test-modules",
          when: core,
          env: ex63
            ? {
                TINYCONNECTORS_TARGET_DIR: `${scratch}/target/tinyconnectors`,
                ...rustEnv,
                ...sccache,
              }
            : {},
          run: `bash scripts/ci/self-hosted/install-test-modules.sh "${modulesDir}" ${modulesEnvFile}`,
        },
        {
          name: "rust-core-coverage",
          when: core,
          needs: ["test-modules"],
          // Doctests and tui coverage run on pushes to main instead (see above).
          // ex63: the core's unit tests run under cargo-nextest, one process
          // per test and in parallel (the guest image ships cargo-nextest).
          env: {
            OUT: "ci-out/lcov/lcov-core.info",
            OH_COV_DOCTESTS: "0",
            OH_COV_TUI: "0",
            ...(ex63 ? { OH_COV_RUNNER: "nextest" } : {}),
          },
          run: withModules("bash scripts/ci/rust-coverage.sh"),
        },
      ],
    },
    {
      name: "rust-lint",
      // Compiles the core crate: holds one of the VM's heavy-compile slots
      // (lanes.mjs). Lower number = served first.
      heavy: 1,
      targetDir: targetDir("lint"),
      env: { ...rustEnv, ...sccache },
      checks: [
        // `--features` is load-bearing: `default` is the contributor set.
        {
          name: "clippy-product",
          when: core,
          run: `cargo clippy -p openhuman --features ${PRODUCT} -- -D warnings`,
        },
        // No separate `cargo clippy -p openhuman` (contributor set): clippy
        // lints every workspace crate in the graph, and openhuman-embed's
        // default features are exactly the core's, so embed-clippy already
        // lints the core's library with them under -D warnings.
        {
          name: "embed-clippy",
          when: core,
          run: "cargo clippy -p openhuman-embed --all-targets -- -D warnings",
        },
        {
          name: "embed-check-no-default",
          when: core,
          run: "cargo check -p openhuman-embed --no-default-features",
        },
        {
          name: "embed-test",
          when: core,
          run: "cargo test -p openhuman-embed",
        },
        {
          name: "tinyhumans-clippy",
          when: core,
          run: "cargo clippy -p openhuman-tinyhumans --all-targets -- -D warnings",
        },
        {
          name: "tinyhumans-test",
          when: core,
          run: "cargo test -p openhuman-tinyhumans",
        },
        {
          name: "prompt-budget",
          when: core,
          run: "bash scripts/check-prompt-budget.sh --verbose",
        },
      ],
    },
    {
      name: "rust-gates-off",
      // Compiles the core crate: holds one of the VM's heavy-compile slots
      // (lanes.mjs). Lower number = served first.
      heavy: 1,
      targetDir: targetDir("gatesoff"),
      env: { ...rustEnv, ...sccache, RUST_MIN_STACK: "67108864" },
      checks: [
        {
          name: "check-gates-off",
          when: rust,
          run: "cargo check --manifest-path Cargo.toml -p openhuman --no-default-features",
        },
        {
          name: "check-e2e-test-support",
          when: rust,
          run: "cargo check --manifest-path Cargo.toml -p openhuman --no-default-features --features e2e-test-support",
        },
        {
          name: "gate-contract-tests",
          when: rust,
          run:
            "cargo test --manifest-path Cargo.toml -p openhuman --no-default-features --lib --" +
            " core::all:: core::cli:: core::jsonrpc:: core::legacy_aliases:: core::runtime::" +
            " agent::registry::agents::loader:: memory::people::contacts_gate_tests::" +
            " openhuman::config:: openhuman::platform::socket::event_handlers:: tools::schemas:: tools::ops::tests::",
        },
        {
          name: "gate-contract-tests-mcp",
          when: rust,
          run: "cargo test --manifest-path Cargo.toml -p openhuman --no-default-features --features mcp --lib -- mcp::server::resources::",
        },
        {
          // Scoped to `introspect::` on purpose; see ci-lite.yml for the hole.
          name: "gate-contract-tests-e2e-support",
          when: rust,
          run:
            "cargo test --manifest-path Cargo.toml -p openhuman --no-default-features --features e2e-test-support --lib --" +
            " test_support::introspect::",
        },
        {
          name: "kernel-floor",
          when: rust,
          run: "bash scripts/check-kernel-floor.sh --verbose",
        },
        {
          name: "dep-sim-calibration",
          when: rust,
          run: "bash scripts/ci/check-dep-sim-calibration.sh",
        },
      ],
    },
    {
      name: "tauri",
      // Compiles the core crate: holds one of the VM's heavy-compile slots
      // (lanes.mjs). Lower number = served first.
      heavy: 2,
      targetDir: targetDir("tauri"),
      env: { ...rustEnv },
      checks: [
        {
          name: "clippy-tauri",
          when: areas.rustTauri,
          env: sccache,
          run: "cargo clippy --manifest-path crates/openhuman-app/Cargo.toml -- -D warnings",
        },
        {
          // llvm-cov's RUSTFLAGS mode with the linker flag cleared, as in
          // ci-lite. ci-lite also clears RUSTC_WRAPPER because its container
          // config installs sccache; on ex63 covEnv sets sccache on purpose.
          name: "tauri-coverage",
          when: areas.rustTauri,
          env: { ...covEnv },
          run:
            (ex63 ? "unset RUSTFLAGS" : "unset RUSTFLAGS RUSTC_WRAPPER") +
            " && cargo llvm-cov clean --manifest-path crates/openhuman-app/Cargo.toml" +
            " && cargo llvm-cov --no-rustc-wrapper --manifest-path crates/openhuman-app/Cargo.toml" +
            " --lcov --output-path ci-out/lcov/lcov-tauri.info",
        },
      ],
    },
    {
      name: "pester",
      checks: [
        {
          name: "install-ps1-pester",
          when: areas.installPs1,
          run: "pwsh -NoProfile -File scripts/tests/OpenHumanWindowsInstall.Tests.ps1",
        },
      ],
    },
  ];

  for (const lane of lanes) {
    lane.checks = lane.checks.map((c) => ({
      ...c,
      when: Boolean(c.when),
      needs: c.needs ?? [],
    }));
    lane.active = lane.checks.some((c) => c.when);
  }
  return { profile, lanes };
}

/** Restrict a plan to the named lanes (hosted groups run a subset each). */
export function selectLanes(plan, names) {
  if (!names || names.length === 0) return plan;
  const known = new Set(plan.lanes.map((l) => l.name));
  for (const n of names) {
    if (!known.has(n)) throw new Error(`unknown lane "${n}"`);
  }
  return { ...plan, lanes: plan.lanes.filter((l) => names.includes(l.name)) };
}

/**
 * Hosted matrix: one entry per group with at least one active lane, so an
 * untouched area never spins up a runner.
 */
export function hostedMatrix(plan) {
  const active = new Set(plan.lanes.filter((l) => l.active).map((l) => l.name));
  return HOSTED_GROUPS.filter((g) => g.lanes.some((l) => active.has(l))).map(
    (g) => ({
      group: g.group,
      lanes: g.lanes.join(","),
      "max-parallel": g.maxParallel,
      container: g.container ? "ghcr.io/tinyhumansai/openhuman_ci:latest" : "",
    }),
  );
}

/**
 * Check every cross-lane and in-lane `needs` resolves, and that no dependency
 * cycle exists. Returns a list of problems (empty when the plan is sound).
 */
export function validatePlan(plan) {
  const problems = [];
  const ids = new Set();
  for (const lane of plan.lanes) {
    for (const c of lane.checks) ids.add(`${lane.name}:${c.name}`);
  }
  for (const lane of plan.lanes) {
    for (const c of lane.checks) {
      for (const need of c.needs) {
        const id = need.includes(":") ? need : `${lane.name}:${need}`;
        if (!ids.has(id))
          problems.push(`${lane.name}:${c.name} needs unknown check ${id}`);
      }
    }
  }
  return problems;
}

/**
 * @typedef {object} Check
 * @property {string} name
 * @property {boolean} when      run it at all (area-selected)
 * @property {string} run        static bash command
 * @property {string[]} needs    checks (`name` or `lane:name`) that must succeed first
 * @property {object} [env]
 * @property {boolean} [reportOnly]  failure never fails the lane
 *
 * @typedef {object} Lane
 * @property {string} name
 * @property {Check[]} checks
 * @property {string|null} [targetDir]  CARGO_TARGET_DIR for this lane
 * @property {object} [env]
 * @property {number} [nice]
 * @property {number} [heavy]  compiles the core crate; priority for a
 *   heavy-compile slot (lower first). Absent for light lanes.
 * @property {boolean} active
 */
