#!/usr/bin/env node
/**
 * Theme migration codemod.
 *
 * Collapses audited light/dark Tailwind colour pairings into the canonical
 * semantic utilities (bg-surface, text-content, border-line, …) defined in
 * app/src/styles/tokens.css + tailwind.config.js.
 *
 * Usage (from repo root):
 *   node scripts/theme-codemod/migrate.mjs            # dry-run (default) + report
 *   node scripts/theme-codemod/migrate.mjs --write    # apply changes
 *   node scripts/theme-codemod/migrate.mjs --selftest # fixture assertions, no FS scan
 *
 * Safety:
 *   - Only adjacent `light dark:` pairs (either order, single space) are touched.
 *   - Class boundaries exclude a trailing `/`, so opacity-suffixed utilities
 *     (bg-neutral-900/50) are never matched.
 *   - Test/spec files are skipped so fixtures asserting class strings stay intact.
 *   - Idempotent: re-running over migrated code yields zero changes.
 *   - Default is dry-run; nothing is written without --write.
 */

import { readFileSync, writeFileSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, relative } from 'node:path';
import { PAIRS, SINGLES } from './map.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, '..', '..');
const SRC_DIR = join(REPO_ROOT, 'app', 'src');
const OUT_DIR = join(REPO_ROOT, 'target', 'theme-codemod');

// Class-boundary fragments: a token must be flanked by string/JSX-className
// delimiters (start/space/quote/backtick/brace/paren/gt) — never a `/`, `-`,
// `:` or word char that would mean it's part of a larger class or opacity suffix.
const LEFT = `(^|[\\s"'\\\`{(>])`;
const RIGHT = `($|[\\s"'\\\`})<])`;

function esc(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Build the ordered list of {name, re, to} replacement rules. */
function buildRules() {
  const rules = [];
  for (const [light, dark, to] of PAIRS) {
    const a = esc(light);
    const b = esc(dark);
    // Match either ordering of the adjacent pair.
    rules.push({
      name: `${light} ${dark} → ${to}`,
      re: new RegExp(`${LEFT}(?:${a} ${b}|${b} ${a})${RIGHT}`, 'g'),
      to,
      kind: 'pair',
    });
  }
  for (const [from, to] of SINGLES) {
    rules.push({
      name: `${from} → ${to}`,
      re: new RegExp(`${LEFT}${esc(from)}${RIGHT}`, 'g'),
      to,
      kind: 'single',
    });
  }
  return rules;
}

/** Apply all rules to `text`; returns { out, counts }. */
function transform(text, rules) {
  let out = text;
  const counts = {};
  for (const rule of rules) {
    let n = 0;
    out = out.replace(rule.re, (_m, l, r) => {
      n++;
      return `${l}${rule.to}${r}`;
    });
    if (n) counts[rule.name] = (counts[rule.name] || 0) + n;
  }
  return { out, counts };
}

function isSkippable(path) {
  return (
    /\.(test|spec)\.[tj]sx?$/.test(path) ||
    /\.d\.ts$/.test(path) ||
    path.includes(`${join('lib', 'i18n')}`) // locale string maps — never touch
  );
}

function walk(dir, acc = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      if (entry === 'node_modules' || entry === '__snapshots__') continue;
      walk(full, acc);
    } else if (/\.(tsx?|jsx?)$/.test(entry) && !isSkippable(full)) {
      acc.push(full);
    }
  }
  return acc;
}

function runSelfTest() {
  const rules = buildRules();
  const cases = [
    ['<div className="bg-white dark:bg-neutral-900 p-4">', '<div className="bg-surface p-4">'],
    ['className="p-2 dark:bg-neutral-900 bg-white"', 'className="p-2 bg-surface"'], // reversed
    [
      'className="text-stone-500 dark:text-neutral-400"',
      'className="text-content-muted"',
    ],
    [
      'className="border-stone-200 dark:border-neutral-800 rounded"',
      'className="border-line rounded"',
    ],
    [
      'className="hover:bg-stone-50 dark:hover:bg-neutral-800"',
      'className="hover:bg-surface-hover"',
    ],
    ['className="font-display text-xl"', 'className="font-title text-xl"'],
    // Opacity-suffixed must be LEFT ALONE:
    ['className="bg-neutral-900/50"', 'className="bg-neutral-900/50"'],
    ['className="bg-white dark:bg-neutral-900/50"', 'className="bg-white dark:bg-neutral-900/50"'],
    // Unrelated classes untouched:
    ['className="bg-primary-500 text-white"', 'className="bg-primary-500 text-white"'],
    // Idempotent (already migrated):
    ['className="bg-surface text-content"', 'className="bg-surface text-content"'],
  ];
  let failed = 0;
  for (const [input, expected] of cases) {
    const { out } = transform(input, rules);
    if (out !== expected) {
      failed++;
      console.error(`FAIL\n  in:  ${input}\n  got: ${out}\n  exp: ${expected}`);
    }
  }
  if (failed) {
    console.error(`\n${failed}/${cases.length} self-test cases failed`);
    process.exit(1);
  }
  console.log(`self-test: all ${cases.length} cases passed`);
}

function main() {
  const args = new Set(process.argv.slice(2));
  if (args.has('--selftest')) return runSelfTest();

  const write = args.has('--write');
  const rules = buildRules();
  const files = walk(SRC_DIR);

  const totals = {};
  const changedFiles = [];
  let totalReplacements = 0;

  for (const file of files) {
    const src = readFileSync(file, 'utf8');
    const { out, counts } = transform(src, rules);
    const fileCount = Object.values(counts).reduce((a, b) => a + b, 0);
    if (fileCount === 0) continue;
    changedFiles.push({ file: relative(REPO_ROOT, file), count: fileCount });
    totalReplacements += fileCount;
    for (const [k, v] of Object.entries(counts)) totals[k] = (totals[k] || 0) + v;
    if (write) writeFileSync(file, out, 'utf8');
  }

  // Report
  mkdirSync(OUT_DIR, { recursive: true });
  const lines = [];
  lines.push(`Theme codemod ${write ? '(WRITE)' : '(DRY-RUN)'} — ${new Date().toISOString?.() ?? ''}`);
  lines.push(`Scanned ${files.length} files`);
  lines.push(`Changed ${changedFiles.length} files, ${totalReplacements} replacements\n`);
  lines.push('Per-rule counts:');
  for (const [k, v] of Object.entries(totals).sort((a, b) => b[1] - a[1])) {
    lines.push(`  ${String(v).padStart(5)}  ${k}`);
  }
  lines.push('\nTop changed files:');
  for (const { file, count } of changedFiles.sort((a, b) => b.count - a.count).slice(0, 40)) {
    lines.push(`  ${String(count).padStart(4)}  ${file}`);
  }
  const report = lines.join('\n') + '\n';
  writeFileSync(join(OUT_DIR, 'report.txt'), report, 'utf8');

  console.log(report);
  console.log(`Report written to ${relative(REPO_ROOT, join(OUT_DIR, 'report.txt'))}`);
  if (!write) console.log('\nDry-run only. Re-run with --write to apply.');
}

main();
