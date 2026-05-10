#!/usr/bin/env node
// Post-build guard for #1403: verify that @sentry/vite-plugin actually
// uploaded source maps and injected debug-IDs into the production bundle.
//
// Failure modes this catches:
//   - SENTRY_AUTH_TOKEN missing at build time -> plugin returned null and
//     nothing was uploaded (bundle has no debug-ID comments).
//   - sourcemap.assets glob mismatched cwd -> plugin logged "Didn't find
//     any matching sources for debug ID upload" and exited 0; bundle has
//     no debug-IDs and Sentry can't symbolicate.
//
// Run after `cargo tauri build` (which invokes Vite). Exits non-zero if no
// JS chunk under app/dist/assets/ shows evidence that @sentry/vite-plugin
// ran — either a `// debugId=<uuid>` pragma comment OR an injected
// `_sentryDebugIds` runtime map.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Use `fileURLToPath` rather than `new URL(...).pathname` — on Windows the
// latter returns a leading-slash path like `/D:/a/openhuman/...` which
// `path.resolve` then mangles into `D:\D:\a\...` (duplicate drive letter),
// causing the verifier to ENOENT on `dist/assets`.
const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..', '..');
const ASSETS = join(ROOT, 'app', 'dist', 'assets');
// The pragma comment `//# debugId=<uuid>` is what @sentry/vite-plugin
// appends to chunks, but Vite/esbuild minification strips it from many
// builds. The `globalThis._sentryDebugIds` map is the actual mechanism the
// Sentry SDK uses at runtime to match captured stacks to uploaded source
// maps for bundled apps — its presence alone is sufficient proof that the
// plugin ran end-to-end and uploaded maps. We accept either signal.
const DEBUG_ID_RE = /\/\/[ #]?\s*debugId=[0-9a-f-]{36}/i;
const RUNTIME_MAP_RE = /_sentryDebugIds/;

function listJsFiles(dir) {
  let out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    const st = statSync(p);
    if (st.isDirectory()) out = out.concat(listJsFiles(p));
    else if (entry.endsWith('.js')) out.push(p);
  }
  return out;
}

function main() {
  let files;
  try {
    files = listJsFiles(ASSETS);
  } catch (err) {
    console.error(`[verify-sentry-sourcemaps] ${ASSETS} not found — did Vite build succeed?`);
    console.error(err.message);
    process.exit(2);
  }

  if (files.length === 0) {
    console.error(`[verify-sentry-sourcemaps] no .js files under ${ASSETS}`);
    process.exit(2);
  }

  let withDebugId = 0;
  let withRuntimeMap = 0;
  for (const f of files) {
    const src = readFileSync(f, 'utf8');
    if (DEBUG_ID_RE.test(src)) withDebugId += 1;
    if (RUNTIME_MAP_RE.test(src)) withRuntimeMap += 1;
  }

  console.log(
    `[verify-sentry-sourcemaps] scanned ${files.length} files; ${withDebugId} carry debug-ID pragmas; ${withRuntimeMap} reference _sentryDebugIds.`
  );

  // Either signal proves @sentry/vite-plugin transformed the bundle. The
  // pragma comment is best-effort (minifiers often strip it); the runtime
  // map is what the SDK actually consults to symbolicate captured stacks.
  if (withDebugId < 1 && withRuntimeMap < 1) {
    console.error(
      '[verify-sentry-sourcemaps] FAIL — Sentry source-map upload did not run or did not inject debug-IDs.\n' +
        '  Likely causes:\n' +
        '    - SENTRY_AUTH_TOKEN missing/empty at vite build time\n' +
        '    - sourcemap.assets glob did not match dist/assets\n' +
        '    - SENTRY_RELEASE / VITE_BUILD_SHA mismatch between upload and runtime\n' +
        '  Without debug-IDs, production stack traces cannot be symbolicated. (#1403)'
    );
    process.exit(1);
  }
}

main();
