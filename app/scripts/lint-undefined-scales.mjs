#!/usr/bin/env node
/**
 * lint:ui-tokens companion — fail on Tailwind colour utilities that name a
 * scale the palette never defines.
 *
 * Motivation: `ocean-*` shipped across ~26 component files and emitted ZERO
 * CSS, because `ocean` is not a colour in `tailwind.config.js` (it is only a
 * *theme preset id* in `src/lib/theme/presets.ts`). Those elements rendered
 * with no background, no text colour and no border. The old rg-based
 * `lint:ui-tokens` regex only banned scales that DO exist but are off-palette
 * (`neutral|stone|slate|canvas|white|black`), so an undefined scale — the more
 * damaging case, since it is silently invisible — slipped straight through.
 *
 * This script inverts the check: instead of a hand-maintained deny-list it
 * derives the ALLOWED scale names from the Tailwind config itself
 * (`theme.extend.colors`) plus Tailwind's own default palette, and fails on
 * any `<utility>-<name>-<shade>` whose `<name>` is in neither.
 *
 * Deliberately NOT flagged:
 *  - bare words (`ocean` as a preset id, a CSS custom property `--ocean`, a
 *    comment) — only utility-shaped matches count;
 *  - arbitrary values (`bg-[#D97757]`), which need no scale;
 *  - non-shade numeric suffixes (`border-l-2`, `w-12`), filtered by requiring
 *    a real Tailwind shade step.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import tailwindColorsModule from 'tailwindcss/colors';

const here = path.dirname(fileURLToPath(import.meta.url));
const appRoot = path.resolve(here, '..');

const SHADES = new Set([
  '50',
  '100',
  '150',
  '200',
  '300',
  '400',
  '500',
  '600',
  '700',
  '800',
  '900',
  '950',
]);

/** Colour-bearing utility prefixes, including directional border/divide forms. */
const UTILITY_PREFIXES = [
  'bg',
  'text',
  'border',
  'border-x',
  'border-y',
  'border-t',
  'border-r',
  'border-b',
  'border-l',
  'ring',
  'ring-offset',
  'divide',
  'divide-x',
  'divide-y',
  'outline',
  'shadow',
  'fill',
  'stroke',
  'accent',
  'caret',
  'decoration',
  'placeholder',
  'from',
  'to',
  'via',
];

/**
 * Tailwind v4 removed `resolveConfig` and this app now defines its custom
 * palette in `src/index.css`'s `@theme` block. Build the effective shade set
 * from Tailwind's exported default palette plus every numeric
 * `--color-<scale>-<shade>` variable declared by the app.
 */
function shadeResolver() {
  const colors = tailwindColorsModule.default ?? tailwindColorsModule;
  const shades = new Set();
  const scaleNames = new Set();

  for (const [scale, values] of Object.entries(colors)) {
    if (!values || typeof values !== 'object') continue;
    for (const shade of Object.keys(values)) {
      if (!/^\d+$/.test(shade)) continue;
      shades.add(`${scale}-${shade}`);
      scaleNames.add(scale);
    }
  }

  const themeCss = readFileSync(path.join(appRoot, 'src/index.css'), 'utf8');
  for (const match of themeCss.matchAll(/--color-([a-z][a-z0-9-]*)-(\d{1,3})\s*:/g)) {
    const [, scale, shade] = match;
    shades.add(`${scale}-${shade}`);
    scaleNames.add(scale);
  }

  if (!shades.has('primary-500')) {
    throw new Error(
      'lint:ui-tokens: Tailwind v4 theme has no primary-500 — refusing to run, ' +
        'because the app palette could not be loaded.'
    );
  }

  return {
    shadeExists: (scale, shade) => shades.has(`${scale}-${shade}`),
    scaleNames: [...scaleNames],
  };
}

const PATTERN = new RegExp(
  `\\b(${UTILITY_PREFIXES.join('|')})-([a-z][a-z0-9]+)-(\\d{2,3})\\b`,
  'g'
);

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    if (entry === 'node_modules' || entry === 'dist' || entry.startsWith('.')) continue;
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      yield* walk(full);
    } else if (/\.(ts|tsx|js|jsx)$/.test(entry)) {
      yield full;
    }
  }
}

const { shadeExists, scaleNames } = shadeResolver();
const violations = [];

for (const file of walk(path.join(appRoot, 'src'))) {
  const lines = readFileSync(file, 'utf8').split('\n');
  lines.forEach((line, i) => {
    // Skip comment-only lines — prose is allowed to name a retired scale.
    if (/^\s*(\/\/|\*|\/\*)/.test(line)) return;
    for (const m of line.matchAll(PATTERN)) {
      const [match, , scale, shade] = m;
      if (!SHADES.has(shade)) continue;
      if (shadeExists(scale, shade)) continue;
      violations.push(`${path.relative(appRoot, file)}:${i + 1}: ${match}`);
    }
  });
}

if (violations.length > 0) {
  console.error(
    `lint:ui-tokens: ${violations.length} Tailwind utility/utilities name a colour scale that ` +
      `the Tailwind v4 theme does not define. These emit NO CSS and render uncoloured:\n`
  );
  for (const v of violations) console.error(`  ${v}`);
  console.error(
    `\nScales that define numeric shades: ${[...scaleNames].sort().join(', ')}\n` +
      `Fix by choosing a defined semantic token — do NOT add a scale only to silence this lint.`
  );
  process.exit(1);
}

console.log(
  `lint:ui-tokens: no undefined colour scales (${scaleNames.length} shade-bearing scales).`
);
