import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  checkVendoredCrates,
  collectDeclaredDependencies,
  collectDocumentedClaims,
  collectModuleIds,
  parseGitmodulesPaths,
  splitTomlComments,
} from '../lib/vendored-crates.mjs';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const CHECKER = resolve(REPO_ROOT, 'scripts/ci/check-vendored-crates.mjs');

// ── claim collection ───────────────────────────────────────────────────────

test('a "git submodule update --init" comment is a vendoring claim', () => {
  const toml = `
# After cloning: \`git submodule update --init vendor/tinydocs\`.
tinydocs = { path = "vendor/tinydocs" }
`;
  assert.deepEqual([...collectDocumentedClaims(toml).keys()], ['tinydocs']);
});

test('a "Vendored as a git submodule" comment is a vendoring claim', () => {
  const toml = '# Vendored as a git submodule under `vendor/tinywallet`.\n';
  assert.deepEqual([...collectDocumentedClaims(toml).keys()], ['tinywallet']);
});

test('a passing mention of a vendor path is not a claim', () => {
  // The shell manifest points at a file inside a vendored Tauri fork purely to
  // say where the code lives. Treating that as "this build consumes the crate
  // from vendor/tauri-cef" would make the guard fire on prose.
  const toml = "# Tauri's vendored dev-server proxy (see `vendor/tauri-cef/.../tauri.rs`)\n";
  assert.deepEqual([...collectDocumentedClaims(toml).keys()], []);
});

test('claims survive a comment that trails a real dependency line', () => {
  const toml = 'foo = { path = "x" }  # submodule at vendor/tinybus\n';
  assert.deepEqual([...collectDocumentedClaims(toml).keys()], ['tinybus']);
});

test('a "#" inside a quoted value does not become a comment', () => {
  const { code, comments } = splitTomlComments('a = "issue #4901"  # real comment\n');
  assert.match(code, /issue #4901/);
  assert.equal(comments.length, 1);
  assert.match(comments[0], /real comment/);
});

test('sub-crate and relative vendor paths collapse onto the vendor root', () => {
  const toml = `
tinymemory-core = { path = "vendor/tinymemory/core" }
tinyplace = { path = "vendor/tinyplace/sdk/rust" }
tinyagents = { path = "../../vendor/tinyagents" }
`;
  assert.deepEqual([...collectDeclaredDependencies(toml)].sort(), [
    'tinyagents',
    'tinymemory',
    'tinyplace',
  ]);
});

test('a commented-out dependency does not count as declared', () => {
  const toml = '# tinydocs = { path = "vendor/tinydocs" }\n';
  assert.deepEqual([...collectDeclaredDependencies(toml)], []);
});

test('module ids come out of the compiled-in registry', () => {
  const rs = `
const TINYDOCS: ModuleRecord = ModuleRecord {
    id: "tinydocs",
    version: "0.1.13",
};
const TINYWALLET: ModuleRecord = ModuleRecord {
    id: "tinywallet",
};
`;
  assert.deepEqual([...collectModuleIds(rs)].sort(), ['tinydocs', 'tinywallet']);
});

test('gitmodules paths are parsed', () => {
  const text = '[submodule "vendor/tinybus"]\n\tpath = vendor/tinybus\n\turl = https://x\n';
  assert.deepEqual([...parseGitmodulesPaths(text)], ['vendor/tinybus']);
});

// ── the assertions ─────────────────────────────────────────────────────────

/** The healthy shape: submodule, gitlink, path dependency. */
function vendoredWorld(name = 'tinydocs') {
  return {
    claims: new Map([[name, { sources: ['a comment in Cargo.toml'], evidence: 'x' }]]),
    submodulePaths: new Set([`vendor/${name}`]),
    gitlinkPaths: new Set([`vendor/${name}`]),
    trackedVendorDirs: new Set(),
    declaredDependencies: new Set([name]),
    notVendored: {},
    inTree: {},
  };
}

test('a properly vendored crate passes', () => {
  const result = checkVendoredCrates(vendoredWorld());
  assert.equal(result.ok, true);
  assert.deepEqual(result.checked, ['tinydocs']);
});

test("3ee5a3cad's shape fails: the comment still claims a submodule, the source is inlined", () => {
  // Exactly what `3ee5a3cad` left behind for tinywallet: the manifest comment
  // and AGENTS.md kept describing a vendored crate while the submodule was
  // gone, the dependency line was gone, and ~3,700 lines of crate source had
  // been copied into src/. This is the regression case the guard exists for.
  const result = checkVendoredCrates({
    claims: new Map([
      [
        'tinywallet',
        {
          sources: ['a comment in Cargo.toml'],
          evidence: 'After cloning: `git submodule update --init vendor/tinywallet`.',
        },
      ],
    ]),
    submodulePaths: new Set(),
    gitlinkPaths: new Set(['vendor/tinybus']),
    trackedVendorDirs: new Set(),
    declaredDependencies: new Set(),
    notVendored: {},
    inTree: {},
  });
  assert.equal(result.ok, false);
  assert.equal(result.failures.length, 1);
  assert.match(result.failures[0], /no `path = vendor\/tinywallet` entry in \.gitmodules/);
  assert.match(result.failures[0], /no manifest declares a `path = "…\/vendor\/tinywallet"`/);
});

test('a submodule replaced by real files is reported as INLINED', () => {
  // The other half of the same failure: someone deletes the gitlink and commits
  // the crate source under the same path. `.gitmodules` may even survive.
  const world = vendoredWorld();
  world.gitlinkPaths = new Set();
  world.trackedVendorDirs = new Set(['vendor/tinydocs']);
  const result = checkVendoredCrates(world);
  assert.equal(result.ok, false);
  assert.match(result.failures[0], /INLINED/);
});

test('a submodule nobody depends on fails', () => {
  const world = vendoredWorld();
  world.declaredDependencies = new Set();
  const result = checkVendoredCrates(world);
  assert.equal(result.ok, false);
  assert.match(result.failures[0], /no manifest declares/);
});

test('a waiver with a reason suppresses the failure', () => {
  const world = vendoredWorld('tinyvoice');
  world.submodulePaths = new Set();
  world.gitlinkPaths = new Set();
  world.declaredDependencies = new Set();
  world.notVendored = {
    tinyvoice: 'Module-only; no crate contract is shared.',
  };
  const result = checkVendoredCrates(world);
  assert.equal(result.ok, true);
  assert.equal(result.failures.length, 0);
  assert.match(result.waived[0], /Module-only/);
});

test('a waiver that stopped being true fails as stale', () => {
  // An unratcheted exemption is how a fixed problem quietly comes back: the
  // waiver would keep passing a crate that no longer needs it, and the next
  // regression would land under its cover.
  const world = vendoredWorld();
  world.notVendored = { tinydocs: 'stale reason' };
  const result = checkVendoredCrates(world);
  assert.equal(result.ok, false);
  assert.match(result.staleWaivers[0], /IS vendored now/);
});

test('an in-tree vendor waiver still requires tracked files and a dependency', () => {
  const world = vendoredWorld('motosan-ai-oauth');
  world.submodulePaths = new Set();
  world.gitlinkPaths = new Set();
  world.inTree = {
    'motosan-ai-oauth': 'No upstream repo to point a submodule at.',
  };

  world.trackedVendorDirs = new Set();
  assert.match(checkVendoredCrates(world).failures[0], /no tracked files/);

  world.trackedVendorDirs = new Set(['vendor/motosan-ai-oauth']);
  assert.equal(checkVendoredCrates(world).ok, true);

  world.declaredDependencies = new Set();
  assert.match(checkVendoredCrates(world).failures[0], /no manifest declares/);
});

test('an in-tree waiver for a path that became a real submodule is stale', () => {
  const world = vendoredWorld('motosan-ai-oauth');
  world.inTree = { 'motosan-ai-oauth': 'reason' };
  assert.match(checkVendoredCrates(world).staleWaivers[0], /real submodule now/);
});

// ── the checker end to end ─────────────────────────────────────────────────

/**
 * Build a throwaway git repo with the file layout the checker reads, and a
 * real mode-160000 index entry so the gitlink assertion is exercised for real
 * rather than against a hand-built Set.
 */
function fixtureRepo({ inlined }) {
  const root = mkdtempSync(join(tmpdir(), 'vendored-crates-'));
  execFileSync('git', ['init', '-q', root]);
  mkdirSync(join(root, 'app/src-tauri'), { recursive: true });
  mkdirSync(join(root, 'src/openhuman/modules'), { recursive: true });
  mkdirSync(join(root, 'vendor/tinydocs'), { recursive: true });

  writeFileSync(
    join(root, 'Cargo.toml'),
    '# After cloning: `git submodule update --init vendor/tinydocs`.\n' +
      (inlined ? '' : 'tinydocs = { path = "vendor/tinydocs", default-features = false }\n'),
  );
  writeFileSync(join(root, 'app/src-tauri/Cargo.toml'), '[package]\nname = "shell"\n');
  writeFileSync(
    join(root, 'src/openhuman/modules/registry.rs'),
    'const TINYDOCS: ModuleRecord = ModuleRecord {\n    id: "tinydocs",\n};\n',
  );
  // `3ee5a3cad` removed ONE submodule; the rest of vendor/ stayed. The fixture
  // keeps an unrelated submodule so the inlined case is a real verdict rather
  // than the checker's "no submodules at all" vacuity bail-out.
  writeFileSync(
    join(root, '.gitmodules'),
    '[submodule "vendor/tinybus"]\n\tpath = vendor/tinybus\n\turl = https://x\n' +
      (inlined
        ? ''
        : '[submodule "vendor/tinydocs"]\n\tpath = vendor/tinydocs\n\turl = https://x\n'),
  );

  if (inlined) {
    // The crate source, copied in under the same path.
    writeFileSync(join(root, 'vendor/tinydocs/lib.rs'), '// inlined\n');
  }
  execFileSync('git', ['add', '-A'], { cwd: root });
  execFileSync('git', ['-c', 'user.email=t@t', '-c', 'user.name=t', 'commit', '-q', '-m', 'init'], {
    cwd: root,
  });
  const sha = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: root,
    encoding: 'utf8',
  }).trim();
  const gitlink = path =>
    execFileSync('git', ['update-index', '--add', '--cacheinfo', `160000,${sha},${path}`], {
      cwd: root,
    });
  gitlink('vendor/tinybus');
  if (!inlined) gitlink('vendor/tinydocs');
  return root;
}

test('the checker passes on a repo where the crate is a real submodule', () => {
  const root = fixtureRepo({ inlined: false });
  try {
    const run = spawnSync(process.execPath, [CHECKER, '--repo', root], {
      encoding: 'utf8',
    });
    assert.equal(run.status, 0, run.stdout + run.stderr);
    assert.match(run.stdout, /Vendored crates verified: tinydocs/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the checker exits 1 on a repo where the crate has been inlined', () => {
  const root = fixtureRepo({ inlined: true });
  try {
    const run = spawnSync(process.execPath, [CHECKER, '--repo', root], {
      encoding: 'utf8',
    });
    // exit 2 would mean the guard failed to run; this must be a real verdict.
    assert.equal(run.status, 1, run.stdout + run.stderr);
    assert.match(run.stdout, /tinydocs/);
    assert.match(run.stdout, /INLINED/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the checker refuses to pass vacuously when it parses nothing', () => {
  const root = mkdtempSync(join(tmpdir(), 'vendored-crates-empty-'));
  try {
    execFileSync('git', ['init', '-q', root]);
    mkdirSync(join(root, 'app/src-tauri'), { recursive: true });
    mkdirSync(join(root, 'src/openhuman/modules'), { recursive: true });
    writeFileSync(join(root, 'Cargo.toml'), '[package]\nname = "core"\n');
    writeFileSync(join(root, 'app/src-tauri/Cargo.toml'), '[package]\nname = "shell"\n');
    writeFileSync(join(root, 'src/openhuman/modules/registry.rs'), '// no modules\n');
    writeFileSync(join(root, '.gitmodules'), '');
    const run = spawnSync(process.execPath, [CHECKER, '--repo', root], {
      encoding: 'utf8',
    });
    assert.equal(run.status, 2, run.stdout + run.stderr);
    assert.match(run.stderr, /refusing to pass vacuously/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the real repo satisfies its own guard', () => {
  const run = spawnSync(process.execPath, [CHECKER], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stdout + run.stderr);
});
