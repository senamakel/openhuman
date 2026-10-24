import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const fixture = fileURLToPath(
  new URL("../ci/security-browser-egress-fixture.sh", import.meta.url),
);
const workflow = fileURLToPath(
  new URL("../../.github/workflows/security-browser-egress.yml", import.meta.url),
);

test("browser egress workflow installs TinyComputer native build dependencies", () => {
  const source = readFileSync(workflow, "utf8");
  const install = source.match(/- name: Install native dependencies\n\s+run: ([^\n]+)/)?.[1];

  assert.ok(install, "workflow installs native dependencies");
  assert.match(install, /libasound2-dev/);
  assert.match(install, /libxdo-dev/);
  assert.match(install, /pkg-config/);
});

test("native fixture follows the pinned TinyComputer contract", () => {
  const source = readFileSync(fixture, "utf8");
  assert.doesNotMatch(source, /cargo build -p tinycomputer --features test-connector/);
  assert.doesNotMatch(
    source,
    /loaded_tinycomputer_uses_tinysecurity_to_allow_one_public_socket/,
  );
});

test("native browser egress fixture runs security and opt-in Chromium tests", () => {
  const dir = mkdtempSync(join(tmpdir(), "security-browser-egress-fixture-"));
  const log = join(dir, "invocations.txt");
  const bash = join(dir, "bash");
  const computerModule = join(dir, "libtinycomputer.so");
  writeFileSync(
    bash,
    `#!/bin/sh\nprintf 'ARGS %s\\nDNS %s\\n' "$*" "\${TINYSECURITY_TEST_DNS_SEQUENCE:-}" >> "${log}"\n`,
  );
  chmodSync(bash, 0o755);
  writeFileSync(computerModule, "fixture module artifact");

  const result = spawnSync("/bin/bash", [fixture], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${dir}:${process.env.PATH}`,
      OPENHUMAN_TEST_SECURITY_MODULE: fixture,
      OPENHUMAN_TEST_TINYCOMPUTER_MODULE: computerModule,
    },
  });

  assert.equal(result.status, 0, result.stderr);
  const calls = readFileSync(log, "utf8");
  assert.match(calls, /cargo build -p tinysecurity-module/);
  assert.doesNotMatch(calls, /cargo build -p tinycomputer/);
  assert.match(
    readFileSync(fixture, "utf8"),
    /strip --strip-debug "\$computer_module"/,
  );
  assert.match(calls, /cargo test -p openhuman --lib/);
  assert.match(calls, /--features[^\n]*native-security-tests/);
  const invocations = calls.split("\n");
  const securityTest = invocations.find((line) =>
    line.includes(
      "modules::browser_egress_host::tests::browser_egress_callbacks_use_tinysecurity_one_use_grants_and_leases",
    ),
  );
  assert.ok(securityTest, "TinySecurity callback test is selected");
  assert.match(securityTest, /-- --exact$/);
  assert.doesNotMatch(securityTest, /--ignored/);
  const browserTest = invocations.find((line) =>
    line.includes(
      "modules::browser_egress_host::tests::loaded_tinycomputer_uses_tinysecurity_to_block_loopback_dials",
    ),
  );
  assert.ok(browserTest, "real-module Chromium test is selected");
  assert.match(browserTest, /-- --ignored --exact$/);
});
