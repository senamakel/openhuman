#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const ROOT = "crates/openhuman-core/src";
const CRATES_ROOT = "crates";
const CORE_MANIFEST = "crates/openhuman-core/Cargo.toml";
const LINE_LIMIT = 750;

// These stateful assembly functions still need semantic decomposition. Pinning
// their current size makes the gate monotonic: they cannot grow, no new
// exception can appear, and deleting an entry is the only way to relax it.
const LEGACY_LIMITS = new Map([
  ["crates/openhuman-core/src/agent/harness/session/builder/factory.rs", 1552],
  ["crates/openhuman-core/src/agent/harness/subagent_runner/ops/runner.rs", 1769],
  ["crates/openhuman-core/src/tools/ops.rs", 1502],
  ["crates/openhuman-core/src/web_chat/progress_bridge.rs", 1547],
]);

function rustFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (
      directory === ROOT &&
      ["api", "bin", "core", "lib.rs", "main.rs", "rpc"].includes(entry.name)
    )
      return [];
    if (entry.isDirectory()) return rustFiles(file);
    return entry.isFile() && entry.name.endsWith(".rs") ? [file] : [];
  });
}

// Every Rust file under every crate, no exclusions — used for the naming and
// inline-test-module checks, which apply repository-wide. The line-limit
// check above stays scoped to `ROOT` exactly as before.
function allRustFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "target") return [];
      return allRustFiles(file);
    }
    return entry.isFile() && entry.name.endsWith(".rs") ? [file] : [];
  });
}

const INLINE_TEST_MODULE_RE =
  /^\s*#\[cfg\([^\n]*\btest\b[^\n]*\)\]\s*\n(?:\s*#\[[^\n]+\]\s*\n)*\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/m;

const failures = [];
for (const file of rustFiles(ROOT)) {
  const source = fs.readFileSync(file, "utf8");
  const lineCount = source.split("\n").length - (source.endsWith("\n") ? 1 : 0);
  const portableFile = file.split(path.sep).join("/");
  const legacyLimit = LEGACY_LIMITS.get(portableFile);
  if (lineCount > (legacyLimit ?? LINE_LIMIT)) {
    failures.push(
      `${file}: ${lineCount} lines (limit ${legacyLimit ?? LINE_LIMIT})`,
    );
  }
}

for (const file of allRustFiles(CRATES_ROOT)) {
  const source = fs.readFileSync(file, "utf8");
  if (["tests.rs", "test.rs"].includes(path.basename(file))) {
    failures.push(
      `${file}: test modules must use a descriptive *_tests.rs filename`,
    );
  }
  if (INLINE_TEST_MODULE_RE.test(source)) {
    failures.push(
      `${file}: inline test module; move it to a sibling *_tests.rs file`,
    );
  }
}

for (const file of LEGACY_LIMITS.keys()) {
  if (!fs.existsSync(file))
    failures.push(`${file}: stale legacy exception; remove it from the gate`);
}

// Integration tests and examples remain repository-level for now, so Cargo
// cannot auto-discover them. Keep the explicit target list
// exhaustive: otherwise adding a file can make `cargo test` silently run
// nothing for it while still exiting successfully.
const manifest = fs.readFileSync(CORE_MANIFEST, "utf8");
function declaredTargets(table) {
  const targets = new Set();
  for (const block of manifest.split(`[[${table}]]`).slice(1)) {
    const name = block.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
    if (name) targets.add(name);
  }
  return targets;
}

for (const [directory, table] of [
  ["tests", "test"],
  ["examples", "example"],
]) {
  const files = new Set(
    fs
      .readdirSync(directory)
      .filter((name) => name.endsWith(".rs"))
      .map((name) => name.slice(0, -3)),
  );
  const declared = declaredTargets(table);
  for (const name of files) {
    if (!declared.has(name))
      failures.push(`${directory}/${name}.rs: missing [[${table}]] entry in ${CORE_MANIFEST}`);
  }
  for (const name of declared) {
    if (!files.has(name))
      failures.push(`${CORE_MANIFEST}: stale [[${table}]] target ${name}`);
  }
}

if (failures.length) {
  console.error("OpenHuman Rust layout check failed:");
  failures.forEach((failure) => console.error(`  - ${failure}`));
  process.exit(1);
}

console.log(
  `OpenHuman Rust layout check passed (new files <= ${LINE_LIMIT} lines; tests external).`,
);
