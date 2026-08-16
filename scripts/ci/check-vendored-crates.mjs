#!/usr/bin/env node
// Fails when a crate the build describes as vendored has been inlined into
// this tree instead.
//
// See scripts/lib/vendored-crates.mjs for the three assertions, the three
// sources claims are drawn from, and why they are shaped that way (#5559).
// Short version: `3ee5a3cad` deleted the `vendor/tinywallet` submodule and
// inlined ~3,700 lines of crate source, leaving the manifest comments intact.
// Nothing failed. The fork then hid a SLIP-10 key-derivation bug from every
// other host until #5533.
//
// The gitlink assertion reads the git INDEX, not the filesystem, so this lane
// needs no `git submodule update --init` and no Rust toolchain.
//
// Usage: check-vendored-crates.mjs [--repo <dir>]
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  checkVendoredCrates,
  collectDeclaredDependencies,
  collectDocumentedClaims,
  collectModuleIds,
  formatReport,
  parseGitmodulesPaths,
} from '../lib/vendored-crates.mjs';

const DEFAULT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function usage() {
  return 'Usage: check-vendored-crates.mjs [--repo <dir>]';
}

const args = process.argv.slice(2);
if (args[0] === '--help' || args[0] === '-h') {
  console.log(usage());
  process.exit(0);
}
let repoRoot = DEFAULT_ROOT;
if (args[0] === '--repo') {
  if (!args[1]) {
    console.error(usage());
    process.exit(2);
  }
  repoRoot = resolve(args[1]);
} else if (args.length > 0) {
  console.error(usage());
  process.exit(2);
}

const MANIFESTS = ['Cargo.toml', 'app/src-tauri/Cargo.toml'];
const REGISTRY = 'src/openhuman/modules/registry.rs';
const GITMODULES = '.gitmodules';

function read(relPath, { optional = false } = {}) {
  try {
    return readFileSync(resolve(repoRoot, relPath), 'utf8');
  } catch (err) {
    if (optional) return '';
    console.error(`Could not read ${relPath}: ${err.message}`);
    process.exit(2);
  }
}

/**
 * `vendor/` entries in the git index, split into gitlinks (submodules, mode
 * 160000) and ordinary tracked files. Reading the index rather than the
 * filesystem is what makes an uninitialised submodule indistinguishable from
 * an initialised one — and an inlined crate distinguishable from both.
 */
function readVendorIndex() {
  let out;
  try {
    out = execFileSync('git', ['ls-files', '--stage', '--', 'vendor'], {
      cwd: repoRoot,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch (err) {
    console.error(`Could not read the git index: ${err.message}`);
    process.exit(2);
  }
  const gitlinkPaths = new Set();
  const trackedVendorDirs = new Set();
  for (const line of out.split('\n')) {
    if (!line.trim()) continue;
    const match = line.match(/^(\d{6})\s+[0-9a-f]+\s+\d\t(.+)$/);
    if (!match) continue;
    const [, mode, path] = match;
    if (mode === '160000') {
      gitlinkPaths.add(path);
    } else {
      const segments = path.split('/');
      if (segments.length >= 2) trackedVendorDirs.add(`${segments[0]}/${segments[1]}`);
    }
  }
  return { gitlinkPaths, trackedVendorDirs };
}

// ── collect claims ─────────────────────────────────────────────────────────

/** @type {Map<string, {sources: string[], evidence: string}>} */
const claims = new Map();
function claim(name, source, evidence) {
  const existing = claims.get(name);
  if (existing) {
    if (!existing.sources.includes(source)) existing.sources.push(source);
    return;
  }
  claims.set(name, { sources: [source], evidence });
}

const declaredDependencies = new Set();
for (const manifest of MANIFESTS) {
  const toml = read(manifest);
  for (const [name, evidence] of collectDocumentedClaims(toml)) {
    claim(name, `a comment in ${manifest}`, evidence);
  }
  for (const name of collectDeclaredDependencies(toml)) {
    declaredDependencies.add(name);
    claim(name, `a path dependency in ${manifest}`, `path = "…/vendor/${name}"`);
  }
}

for (const id of collectModuleIds(read(REGISTRY))) {
  claim(id, `the module registry (${REGISTRY})`, `id: "${id}" in modules::registry::ALL`);
}

// Guard the guard: a scanner that silently found nothing would turn this lane
// into a rubber stamp, which is worse than no lane at all. Treat empty input as
// a failure OF THE CHECK (exit 2), distinct from a real regression (exit 1).
if (claims.size === 0) {
  console.error(
    'FAIL: found zero vendored-crate claims across ' +
      `${MANIFESTS.join(', ')} and ${REGISTRY}.\n` +
      'Either those files changed shape or the scanner is broken — refusing to pass vacuously.',
  );
  process.exit(2);
}

const { gitlinkPaths, trackedVendorDirs } = readVendorIndex();
if (gitlinkPaths.size === 0) {
  console.error(
    'FAIL: the git index reports no submodules under vendor/ at all.\n' +
      'That is either a broken checkout or a broken reader — refusing to pass vacuously.',
  );
  process.exit(2);
}

const result = checkVendoredCrates({
  claims,
  submodulePaths: parseGitmodulesPaths(read(GITMODULES)),
  gitlinkPaths,
  trackedVendorDirs,
  declaredDependencies,
});

console.log(formatReport(result));
process.exit(result.ok ? 0 : 1);
