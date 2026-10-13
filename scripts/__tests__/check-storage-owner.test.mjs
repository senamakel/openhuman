import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { ALLOWLIST, checkMetadata, formatReport } from '../ci/check-storage-owner.mjs';

const CHECKER = resolve(dirname(fileURLToPath(import.meta.url)), '../ci/check-storage-owner.mjs');
const pkg = (name, deps = [], source = null) => ({
  name,
  source,
  dependencies: deps.map(d => ({ name: d, kind: null, optional: false })),
});
const registry = 'registry+https://github.com/rust-lang/crates.io-index';

test('a clean workspace passes', () => {
  assert.deepEqual(checkMetadata({ packages: [pkg('openhuman', ['serde']), pkg('tinyflows', ['tinystoragedrivers-core'])] }), []);
});

test('every forbidden backend dependency is rejected', () => {
  for (const dep of ['rusqlite', 'libsqlite3-sys', 'mongodb', 'object_store', 'sqlx', 'redb', 'sled', 'rocksdb']) {
    const v = checkMetadata({ packages: [pkg('tinymcp', [dep])] });
    assert.equal(v.length, 1, dep);
    assert.equal(v[0].package, 'tinymcp');
  }
});

test('tinystoragedrivers crates own the backends', () => {
  const v = checkMetadata({
    packages: [pkg('tinystoragedrivers-sqlite', ['rusqlite']), pkg('tinystoragedrivers-file')],
  });
  assert.deepEqual(v, []);
});

test('backend-named crates outside tinystoragedrivers are rejected', () => {
  for (const name of ['tinyflows-sqlite', 'tinyflows-mongodb', 'thing-file']) {
    assert.equal(checkMetadata({ packages: [pkg(name)] })[0].kind, 'crate-name', name);
  }
});

test('registry crates are ignored', () => {
  assert.deepEqual(
    checkMetadata({ packages: [pkg('rusqlite', ['libsqlite3-sys'], registry), pkg('same-file', [], registry)] }),
    []
  );
});

test('allowlisted read-only readers may depend on rusqlite, with a reason', () => {
  for (const [name, reason] of Object.entries(ALLOWLIST)) {
    assert.ok(reason.length > 10, `${name} needs a reason`);
    assert.deepEqual(checkMetadata({ packages: [pkg(name, ['rusqlite'])] }), []);
  }
  assert.equal(checkMetadata({ packages: [pkg('tinyskills', ['rusqlite'])] }).length, 1);
});

test('the report names the offender', () => {
  assert.match(formatReport(checkMetadata({ packages: [pkg('tinymcp', ['rusqlite'])] })), /tinymcp: depends on rusqlite/);
  assert.match(formatReport([]), /ok/);
});

test('CLI exits 1 on a violation, 0 when clean, 2 on unreadable input', () => {
  const dir = mkdtempSync(join(tmpdir(), 'storage-owner-'));
  try {
    const bad = join(dir, 'bad.json');
    const good = join(dir, 'good.json');
    writeFileSync(bad, JSON.stringify({ packages: [pkg('tinymcp', ['rusqlite'])] }));
    writeFileSync(good, JSON.stringify({ packages: [pkg('tinymcp')] }));
    assert.equal(spawnSync('node', [CHECKER, '--metadata', bad]).status, 1);
    assert.equal(spawnSync('node', [CHECKER, '--metadata', good]).status, 0);
    assert.equal(spawnSync('node', [CHECKER, '--metadata', join(dir, 'missing.json')]).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
