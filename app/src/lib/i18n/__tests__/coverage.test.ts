import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { beforeAll, describe, expect, it } from 'vitest';

const REPO_ROOT = path.resolve(__dirname, '../../../../..');
const SCRIPT = path.join(REPO_ROOT, 'scripts/i18n-coverage.ts');

interface LocaleReport {
  locale: string;
  missingChunks: number[];
  missingKeys: string[];
  extraKeys: string[];
  driftedKeys: Array<{ key: string; expectedChunk: number; actualChunk: number }>;
}

let locales: LocaleReport[] = [];

describe('i18n coverage', () => {
  beforeAll(() => {
    // Pipe stdout to a temp file via shell redirection — under vitest's worker pool,
    // direct `stdio: 'pipe'` truncates at 64KB regardless of maxBuffer.
    const dir = mkdtempSync(path.join(tmpdir(), 'i18n-cov-'));
    const out = path.join(dir, 'report.json');
    try {
      const tsx = path.join(REPO_ROOT, 'node_modules/.bin/tsx');
      execFileSync('sh', ['-c', `"${tsx}" "${SCRIPT}" --json --no-unused > "${out}"`], {
        cwd: REPO_ROOT,
        stdio: ['ignore', 'inherit', 'inherit'],
      });
      const parsed = JSON.parse(readFileSync(out, 'utf8')) as { locales: LocaleReport[] };
      locales = parsed.locales;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);

  it('runs against every non-English locale', () => {
    expect(locales.length).toBeGreaterThan(0);
  });

  it('no locale has missing chunk files', () => {
    const offenders = locales.filter(l => l.missingChunks.length).map(l => l.locale);
    expect(offenders).toEqual([]);
  });

  it('no locale is missing keys vs English', () => {
    const offenders = locales
      .filter(l => l.missingKeys.length)
      .map(l => `${l.locale}:${l.missingKeys.length}`);
    expect(offenders).toEqual([]);
  });

  it('no locale defines keys absent from English', () => {
    const offenders = locales
      .filter(l => l.extraKeys.length)
      .map(l => `${l.locale}:${l.extraKeys.join(',')}`);
    expect(offenders).toEqual([]);
  });

  it('every key lives in the same chunk as English', () => {
    const offenders = locales
      .filter(l => l.driftedKeys.length)
      .map(l => `${l.locale}:${l.driftedKeys.length}`);
    expect(offenders).toEqual([]);
  });
});
