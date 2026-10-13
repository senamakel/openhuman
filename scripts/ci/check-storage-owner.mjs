#!/usr/bin/env node
// Storage-owner gate (tinyhumansai/openhuman#7642, part of #7522).
//
// tinystoragedrivers is the only crate family allowed to own a storage
// backend. This check reads `cargo metadata` for the root workspace and for
// `crates/openhuman-app` and fails when
//   1. a first-party package (path or git source, i.e. not a crates.io
//      dependency) outside `tinystoragedrivers*` declares a dependency on one
//      of FORBIDDEN_DEPS (any kind, optional or not), or
//   2. a first-party package outside `tinystoragedrivers*` is named
//      `*-sqlite`, `*-mongodb` or `*-file`.
// Third-party registry crates are ignored (they are the backends' own deps,
// e.g. `rusqlite -> libsqlite3-sys`).
//
// Read-only readers of external databases are not storage and are listed in
// ALLOWLIST with a reason. A temporary entry names the open issue that
// removes it.
//
// Usage: check-storage-owner.mjs [repo-root] [--metadata <file.json>]...
// With --metadata the given `cargo metadata` JSON files are checked instead of
// running cargo (used by the self-test).
// Exit 0 when clean, 1 on a violation, 2 when the inputs cannot be read.

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const FORBIDDEN_DEPS = [
  'rusqlite',
  'libsqlite3-sys',
  'mongodb',
  'object_store',
  'sqlx',
  'redb',
  'sled',
  'rocksdb',
];

export const FORBIDDEN_CRATE_NAME = /-(sqlite|mongodb|file)$/;

/** Package name -> why it may keep a forbidden dependency. */
export const ALLOWLIST = {
  tinychannels: 'iMessage chat.db reader: reads an external SQLite database read-only, not storage',
  'openhuman-app': 'imessage scanner: reads chat.db directly and read-only, not storage',
  'tinymemory-integrations': 'legacy-import: reads legacy SQLite files read-only for one-time import',
};

export const MANIFESTS = ['Cargo.toml', 'crates/openhuman-app/Cargo.toml'];

const isOwner = name => name.startsWith('tinystoragedrivers');

/** First-party = built from a path or a git source, not from a registry. */
const isFirstParty = pkg => !pkg.source || pkg.source.startsWith('git+');

/** Returns `[{ package, kind, detail }]` violations for one metadata document. */
export function checkMetadata(metadata, allowlist = ALLOWLIST) {
  const out = [];
  for (const pkg of metadata.packages ?? []) {
    if (!isFirstParty(pkg) || isOwner(pkg.name)) continue;
    if (FORBIDDEN_CRATE_NAME.test(pkg.name)) {
      out.push({ package: pkg.name, kind: 'crate-name', detail: `crate name matches ${FORBIDDEN_CRATE_NAME}` });
    }
    if (Object.hasOwn(allowlist, pkg.name)) continue;
    const seen = new Set();
    for (const dep of pkg.dependencies ?? []) {
      if (FORBIDDEN_DEPS.includes(dep.name) && !seen.has(dep.name)) {
        seen.add(dep.name);
        out.push({ package: pkg.name, kind: 'dependency', detail: `depends on ${dep.name}` });
      }
    }
  }
  return out;
}

export function formatReport(violations) {
  if (violations.length === 0) return 'storage-owner: ok (no storage backend outside tinystoragedrivers*)';
  const lines = violations.map(v => `  - ${v.package}: ${v.detail}`);
  return [
    'storage-owner: FAILED. tinystoragedrivers* owns every storage backend.',
    ...lines,
    'Use the tinystoragedrivers ports instead, or add a read-only external-database reader to ALLOWLIST in scripts/ci/check-storage-owner.mjs with a reason.',
  ].join('\n');
}

function loadMetadata(root, manifest) {
  const r = spawnSync(
    'cargo',
    ['metadata', '--format-version', '1', '--manifest-path', resolve(root, manifest)],
    { cwd: root, encoding: 'utf8', maxBuffer: 1 << 30 }
  );
  if (r.status !== 0) throw new Error(`cargo metadata failed for ${manifest}: ${r.stderr}`);
  return JSON.parse(r.stdout);
}

export function main(argv) {
  const files = [];
  let root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--metadata') files.push(argv[++i]);
    else root = resolve(argv[i]);
  }
  let docs;
  try {
    docs = files.length
      ? files.map(f => JSON.parse(readFileSync(f, 'utf8')))
      : MANIFESTS.map(m => loadMetadata(root, m));
  } catch (err) {
    console.error(`storage-owner: cannot read inputs: ${err.message}`);
    return 2;
  }
  const seen = new Set();
  const violations = docs
    .flatMap(d => checkMetadata(d))
    .filter(v => !seen.has(`${v.package}|${v.detail}`) && seen.add(`${v.package}|${v.detail}`));
  console.log(formatReport(violations));
  return violations.length ? 1 : 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  process.exit(main(process.argv.slice(2)));
}
