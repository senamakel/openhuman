import type { Theme } from './types';

/**
 * Built-in theme presets.
 *
 * `light` and `dark` carry no colour overrides — they rely entirely on the
 * tokens.css `:root` / `:root.dark` defaults, so they always match the
 * historical palette. The remaining presets layer a small set of overrides on
 * top of a light or dark base; tokens they don't mention inherit the base.
 */

export const LIGHT_THEME_ID = 'light';
export const DARK_THEME_ID = 'dark';

export const PRESET_THEMES: Theme[] = [
  {
    id: LIGHT_THEME_ID,
    name: 'Light',
    isDark: false,
    builtIn: true,
    colors: {},
    fonts: {},
  },
  {
    id: DARK_THEME_ID,
    name: 'Dark',
    isDark: true,
    builtIn: true,
    colors: {},
    fonts: {},
  },
  {
    id: 'ocean',
    name: 'Ocean',
    isDark: false,
    builtIn: true,
    colors: {
      'surface-canvas': '233 242 252',
      'surface': '255 255 255',
      'surface-muted': '224 236 248',
      'surface-subtle': '230 240 250',
      'surface-hover': '224 236 248',
      'line': '203 222 242',
      'line-subtle': '224 236 248',
      'content': '15 36 56',
      'content-secondary': '45 70 96',
      'primary-500': '74 131 221',
      'primary-600': '53 110 200',
      'primary-700': '40 92 176',
    },
    fonts: {},
  },
  {
    id: 'midnight',
    name: 'Midnight',
    isDark: true,
    builtIn: true,
    colors: {
      'surface-canvas': '10 14 26',
      'surface': '17 23 41',
      'surface-muted': '26 33 54',
      'surface-subtle': '26 33 54',
      'surface-strong': '33 42 66',
      'surface-hover': '33 42 66',
      'line': '38 48 74',
      'line-strong': '54 66 98',
      'line-subtle': '30 39 62',
      'content': '226 232 240',
      'content-secondary': '180 190 210',
      'content-muted': '140 152 178',
      'content-faint': '104 116 142',
      'primary-500': '96 165 250',
      'primary-600': '59 130 246',
    },
    fonts: {},
  },
  {
    id: 'sepia',
    name: 'Sepia',
    isDark: false,
    builtIn: true,
    colors: {
      'surface-canvas': '244 236 222',
      'surface': '250 244 233',
      'surface-muted': '238 228 210',
      'surface-subtle': '240 231 215',
      'surface-strong': '232 220 198',
      'surface-hover': '238 228 210',
      'line': '222 209 186',
      'line-strong': '206 190 162',
      'line-subtle': '234 224 206',
      'content': '60 50 38',
      'content-secondary': '90 76 58',
      'content-muted': '120 104 82',
      'content-faint': '160 144 120',
      'primary-500': '180 120 60',
      'primary-600': '156 100 46',
    },
    fonts: {
      body: `'Newsreader', Georgia, Cambria, 'Times New Roman', Times, serif`,
      heading: `'Newsreader', Georgia, Cambria, 'Times New Roman', Times, serif`,
    },
  },
];

export function findPreset(id: string): Theme | undefined {
  return PRESET_THEMES.find((t) => t.id === id);
}
