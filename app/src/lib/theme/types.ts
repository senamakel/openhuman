import type { FontRole } from './tokens';

/**
 * A theme is a (partial) set of overrides for the canonical tokens in
 * `styles/tokens.css`. Anything omitted falls through to the tokens.css
 * Light/Dark defaults — so the built-in "Light" and "Dark" presets carry empty
 * override maps and simply lean on `isDark`.
 */
export interface Theme {
  /** Stable id (preset id or generated id for custom themes). */
  id: string;
  /** Display name. */
  name: string;
  /** Whether to apply the `.dark` class (selects the tokens.css dark base and
   *  keeps any remaining `dark:` utilities aligned). */
  isDark: boolean;
  /** Built-in presets cannot be edited in place — editing duplicates them. */
  builtIn: boolean;
  /** Colour token overrides, keyed by var name (no `--`) → `"R G B"` channels. */
  colors: Record<string, string>;
  /** Font role overrides → CSS font-family stack. */
  fonts: Partial<Record<FontRole, string>>;
}
