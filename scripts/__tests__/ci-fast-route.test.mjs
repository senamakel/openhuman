// ci-fast-route.sh: how ci-fast-hosted.yml decides whether a PR commit runs
// on GitHub-hosted runners or is left to the EX63 (ci-fast.yml).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(
  new URL("../ci/ci-fast-route.sh", import.meta.url),
);

// A fake `gh api` that answers from fixture JSON through the real jq, and a
// no-op `sleep` so the 3-minute poll finishes at once.
function route({ runs = [], jobs = {}, env = {} }) {
  const dir = mkdtempSync(join(tmpdir(), "ci-fast-route-"));
  try {
    writeFileSync(
      join(dir, "runs.json"),
      JSON.stringify({ workflow_runs: runs }),
    );
    for (const [id, list] of Object.entries(jobs))
      writeFileSync(
        join(dir, `jobs-${id}.json`),
        JSON.stringify({ jobs: list }),
      );
    writeFileSync(
      join(dir, "gh"),
      `#!/usr/bin/env bash
echo "$*" >> "${dir}/calls"
url="$2"; filter="$4"
case "$url" in
  */workflows/ci-fast.yml/runs*) file="${dir}/runs.json" ;;
  */actions/runs/*/jobs*) id="\${url#*/actions/runs/}"; file="${dir}/jobs-\${id%%/*}.json" ;;
  *) exit 1 ;;
esac
[ -f "$file" ] || exit 1
jq -r "$filter" "$file"
`,
      { mode: 0o755 },
    );
    writeFileSync(join(dir, "sleep"), "#!/usr/bin/env bash\n", { mode: 0o755 });
    const output = join(dir, "output");
    writeFileSync(output, "");
    writeFileSync(join(dir, "calls"), "");
    const result = spawnSync("bash", [script], {
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        GH_TOKEN: "fixture",
        REPO: "fixture/repo",
        HEAD_SHA: "abc123",
        AUTHOR: "alice",
        ACTOR: "alice",
        ASSOCIATION: "MEMBER",
        GITHUB_OUTPUT: output,
        ...env,
      },
    });
    assert.equal(result.status, 0, result.stderr);
    return {
      output: Object.fromEntries(
        readFileSync(output, "utf8")
          .trim()
          .split("\n")
          .map((line) => line.split("=")),
      ),
      calls: readFileSync(join(dir, "calls"), "utf8").trim(),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const routeJob = { name: "Route (org members → EX63)", conclusion: "success" };
const memberJobs = [
  routeJob,
  { name: "Lanes / CI Fast (EX63)", conclusion: null },
];
const outsiderJobs = [routeJob, { name: "Lanes", conclusion: "skipped" }];

test("a draft PR runs on GitHub-hosted runners without asking ci-fast.yml", () => {
  const { output, calls } = route({
    runs: [
      {
        id: 1,
        event: "pull_request_target",
        created_at: "2026-10-22T10:00:00Z",
      },
    ],
    jobs: { 1: memberJobs },
    env: { DRAFT: "true" },
  });
  assert.deepEqual(output, { outsider: "true", source: "draft" });
  assert.equal(calls, "");
});

test("a ready member PR follows ci-fast.yml onto the EX63", () => {
  const { output } = route({
    runs: [
      {
        id: 2,
        event: "pull_request_target",
        created_at: "2026-10-22T10:00:05Z",
      },
    ],
    jobs: { 2: memberJobs },
    env: { DRAFT: "false", SINCE: "2026-10-22T10:00:00Z" },
  });
  assert.deepEqual(output, { outsider: "false", source: "ci-fast" });
});

test("an outsider PR follows ci-fast.yml onto GitHub-hosted runners", () => {
  const { output } = route({
    runs: [
      {
        id: 3,
        event: "pull_request_target",
        created_at: "2026-10-22T10:00:05Z",
      },
    ],
    jobs: { 3: outsiderJobs },
    env: { ASSOCIATION: "NONE", SINCE: "2026-10-22T10:00:00Z" },
  });
  assert.deepEqual(output, { outsider: "true", source: "ci-fast" });
});

test("marking a draft ready ignores the draft's earlier ci-fast.yml run on the same commit", () => {
  // Only the draft-era run (EX63 skipped) exists yet; it predates this event,
  // so it must not route the now-ready PR to GitHub-hosted runners.
  const { output } = route({
    runs: [
      {
        id: 4,
        event: "pull_request_target",
        created_at: "2026-10-22T09:00:00Z",
      },
    ],
    jobs: { 4: outsiderJobs },
    env: { DRAFT: "false", SINCE: "2026-10-22T10:00:00Z" },
  });
  assert.equal(output.outsider, "false");
  assert.equal(output.source, "fallback");
});
