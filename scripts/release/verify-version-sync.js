#!/usr/bin/env node
// Verify release version consistency across all authoritative files.
//
// Usage:
//   node scripts/release/verify-version-sync.js [expected-version]
//
// If expected-version is provided, every source must match it.

'use strict';

const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '..', '..');
const expectedVersion = process.argv[2] || process.env.EXPECTED_VERSION || null;

function readJsonVersion(filePath, field = 'version') {
  const data = JSON.parse(fs.readFileSync(filePath, 'utf8'));
  const value = data[field];
  if (!value || typeof value !== 'string') {
    throw new Error(`Missing string "${field}" in ${path.relative(root, filePath)}`);
  }
  return value;
}

function readCargoVersion(filePath, section = 'package') {
  const cargo = fs.readFileSync(filePath, 'utf8');
  const escapedSection = section.replace('.', '\\.');
  const match = cargo.match(
    new RegExp(`^\\[${escapedSection}\\][\\s\\S]*?^version\\s*=\\s*"([^"]+)"`, 'm'),
  );
  if (!match) {
    throw new Error(`Failed to read [${section}].version in ${path.relative(root, filePath)}`);
  }
  return match[1];
}

const versions = {
  'app/package.json': readJsonVersion(path.join(root, 'app/package.json')),
  'crates/openhuman-app/tauri.conf.json': readJsonVersion(path.join(root, 'crates/openhuman-app/tauri.conf.json')),
  'app/src-tauri-mobile/tauri.conf.json': readJsonVersion(
    path.join(root, 'app/src-tauri-mobile/tauri.conf.json'),
  ),
  'crates/openhuman-app/Cargo.toml': readCargoVersion(path.join(root, 'crates/openhuman-app/Cargo.toml')),
  'app/src-tauri-mobile/Cargo.toml': readCargoVersion(
    path.join(root, 'app/src-tauri-mobile/Cargo.toml'),
  ),
  'Cargo.toml': readCargoVersion(path.join(root, 'Cargo.toml'), 'workspace.package'),
};

const values = Object.values(versions);
const unique = [...new Set(values)];

if (expectedVersion && values.some((value) => value !== expectedVersion)) {
  console.error('[verify-version-sync] Expected version mismatch.');
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  console.error(`  expected: ${expectedVersion}`);
  process.exit(1);
}

if (unique.length !== 1) {
  console.error('[verify-version-sync] Version mismatch detected.');
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

console.log(`[verify-version-sync] OK ${unique[0]}`);
