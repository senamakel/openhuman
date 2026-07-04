#!/usr/bin/env node
// Bump version in package.json, Tauri configs, and Cargo.toml manifests.
//
// Usage:
//   node scripts/release/bump-version.js <patch|minor|major|X.Y.Z>
//
// An explicit `X.Y.Z` sets that exact version (it must be strictly greater
// than the current one — release tags are immutable). Releases are cut with
// a manually chosen version; the increment keywords remain for tooling.
//
// Outputs (to stdout, one per line):
//   version=X.Y.Z
//   tag=vX.Y.Z
//
// When GITHUB_OUTPUT is set (CI), the same key=value pairs are appended there.

'use strict';

const fs = require('fs');
const path = require('path');

const RELEASE_TYPE = process.argv[2] || process.env.RELEASE_TYPE;
const allowed = new Set(['patch', 'minor', 'major']);
const explicitVersion = /^(\d+)\.(\d+)\.(\d+)$/.exec(RELEASE_TYPE || '');
if (!allowed.has(RELEASE_TYPE) && !explicitVersion) {
  console.error(`Usage: bump-version.js <patch|minor|major|X.Y.Z>  (got: "${RELEASE_TYPE}")`);
  process.exit(1);
}

const root = path.resolve(__dirname, '..', '..');
const packagePath = path.join(root, 'app/package.json');
const tauriPath = path.join(root, 'app/src-tauri/tauri.conf.json');
const tauriCargoPath = path.join(root, 'app/src-tauri/Cargo.toml');
const mobileTauriPath = path.join(root, 'app/src-tauri-mobile/tauri.conf.json');
const mobileCargoPath = path.join(root, 'app/src-tauri-mobile/Cargo.toml');
const coreCargoPath = path.join(root, 'Cargo.toml');

// ── Read current version ────────────────────────────────────────────────────
const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
const match = String(pkg.version || '').match(/^(\d+)\.(\d+)\.(\d+)$/);
if (!match) {
  throw new Error(`package.json version must be SemVer X.Y.Z, found: ${pkg.version}`);
}

let major = Number(match[1]);
let minor = Number(match[2]);
let patch = Number(match[3]);

// ── Bump ────────────────────────────────────────────────────────────────────
if (explicitVersion) {
  const next = explicitVersion.slice(1, 4).map(Number);
  const current = [major, minor, patch];
  const cmp =
    next[0] !== current[0] ? next[0] - current[0]
    : next[1] !== current[1] ? next[1] - current[1]
    : next[2] - current[2];
  if (cmp <= 0) {
    console.error(
      `Explicit version ${RELEASE_TYPE} must be strictly greater than current ${major}.${minor}.${patch}`
    );
    process.exit(1);
  }
  [major, minor, patch] = next;
} else if (RELEASE_TYPE === 'major') {
  major += 1; minor = 0; patch = 0;
} else if (RELEASE_TYPE === 'minor') {
  minor += 1; patch = 0;
} else {
  patch += 1;
}
const nextVersion = `${major}.${minor}.${patch}`;

// ── Write package.json ──────────────────────────────────────────────────────
pkg.version = nextVersion;
fs.writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);

// ── Write tauri.conf.json ───────────────────────────────────────────────────
function writeTauriVersion(filePath, nextVersion) {
  const tauri = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  tauri.version = nextVersion;
  fs.writeFileSync(filePath, `${JSON.stringify(tauri, null, 2)}\n`);
}

writeTauriVersion(tauriPath, nextVersion);
writeTauriVersion(mobileTauriPath, nextVersion);

function bumpCargoVersion(filePath, nextVersion) {
  const cargo = fs.readFileSync(filePath, 'utf8');
  const updatedCargo = cargo.replace(
    /(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
    `$1${nextVersion}$3`,
  );
  if (updatedCargo === cargo) {
    throw new Error(`Failed to update [package].version in ${path.relative(root, filePath)}`);
  }
  fs.writeFileSync(filePath, updatedCargo);
}

// ── Write Cargo.toml files ──────────────────────────────────────────────────
bumpCargoVersion(tauriCargoPath, nextVersion);
bumpCargoVersion(mobileCargoPath, nextVersion);
bumpCargoVersion(coreCargoPath, nextVersion);

// ── Output ──────────────────────────────────────────────────────────────────
const lines = `version=${nextVersion}\ntag=v${nextVersion}\n`;
process.stdout.write(lines);

if (process.env.GITHUB_OUTPUT) {
  fs.appendFileSync(process.env.GITHUB_OUTPUT, lines);
}

console.error(`[bump-version] ${pkg.version} → ${nextVersion}`);
