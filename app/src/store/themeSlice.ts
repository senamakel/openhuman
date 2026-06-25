import { createSelector, createSlice, type PayloadAction } from '@reduxjs/toolkit';

import type { FontRole } from '../lib/theme/tokens';
import type { Theme, ThemeFamily } from '../lib/theme/types';
import {
  PRESET_THEMES,
  THEME_FAMILIES,
  findFamily,
  familyForThemeId,
  resolveFamilyVariant,
} from '../lib/theme/presets';

export type ThemeMode = 'light' | 'dark' | 'system';
/** Theme variant preference: explicit light/dark, or follow the OS. */
export type ThemeVariant = 'light' | 'dark' | 'system';

/** Sentinel active-theme id meaning "follow OS light/dark preference". */
export const SYSTEM_THEME_ID = 'system';
/** Default theme family selected on first run. */
export const DEFAULT_FAMILY_ID = 'classic';
export type TabBarLabels = 'hover' | 'always';
export type AgentMessageViewMode = 'bubbles' | 'text';
/**
 * Global app font size (issue #3120). Drives the root `<html>` font-size, which
 * scales every rem-based Tailwind text utility — including chat messages and the
 * composer — independently of the OS / system font setting.
 */
export type FontSize = 'small' | 'medium' | 'large' | 'xlarge';

/**
 * Single source of truth mapping each {@link FontSize} to the concrete root
 * `font-size` applied to `<html>`. `medium` (16px) matches the historical
 * `:root` size, so existing users see no change after the field defaults in.
 * Consumed by `ThemeProvider`; keep this the only place the px values live.
 */
export const FONT_SIZE_PX: Record<FontSize, string> = {
  small: '14px',
  medium: '16px',
  large: '18px',
  xlarge: '20px',
};

interface ThemeState {
  mode: ThemeMode;
  tabBarLabels: TabBarLabels;
  fontSize: FontSize;
  agentMessageViewMode: AgentMessageViewMode;
  /**
   * Runtime Developer Mode (default OFF).
   * When true, all developer and diagnostic surfaces become visible.
   * Combines with the build-time `IS_DEV` flag — either one enables the gate.
   * Gating is UI-only: the Rust SecurityPolicy / autonomy tier enforcement
   * is authoritative and is never relaxed by this toggle.
   */
  developerMode: boolean;
  /**
   * Hide the live "Agentic task insights" step-by-step timeline in chat
   * (default OFF). When true, the verbose per-agent step rows are collapsed
   * away: the chat shows only the existing message-bubble loading plus a
   * compact blinking "Processing" link while a turn is in flight. The full
   * timeline is still one click away via that link / the "View full agent
   * process Source" affordance, which open the existing side panel.
   */
  hideAgentInsights: boolean;
  /**
   * Active selection: a theme **family** id (`classic`, `ocean`, `matrix`,
   * `hal9000`, `sepia`) or a custom theme id. Combined with
   * {@link ThemeState.themeVariant} to resolve the concrete theme. Legacy values
   * (`light`/`dark`/`system`/`ocean`/`midnight`) from older persisted state are
   * normalized by {@link selectEffectiveTheme}.
   */
  activeThemeId: string;
  /**
   * Which variant of the active family to apply: explicit `light`/`dark` or
   * `system` (follow OS). Mirrors {@link ThemeState.mode} so the simple
   * Appearance toggle and the Theme Studio variant control stay in sync.
   */
  themeVariant: ThemeVariant;
  /** User-authored themes (full or partial token overrides). */
  customThemes: Theme[];
}

const initialState: ThemeState = {
  mode: 'system',
  tabBarLabels: 'hover',
  fontSize: 'medium',
  agentMessageViewMode: 'text',
  developerMode: false,
  hideAgentInsights: false,
  activeThemeId: DEFAULT_FAMILY_ID,
  themeVariant: 'system',
  customThemes: [],
};

const themeSlice = createSlice({
  name: 'theme',
  initialState,
  reducers: {
    setThemeMode(state, action: PayloadAction<ThemeMode>) {
      // The simple Appearance light/dark/system toggle drives the variant of the
      // currently-selected family (it no longer forces the Classic family).
      state.mode = action.payload;
      state.themeVariant = action.payload;
    },
    /** Set the light/dark/system variant of the active family. */
    setThemeVariant(state, action: PayloadAction<ThemeVariant>) {
      state.themeVariant = action.payload;
      state.mode = action.payload;
    },
    /** Select a theme family (or a custom theme id). */
    setActiveFamily(state, action: PayloadAction<string>) {
      state.activeThemeId = action.payload;
    },
    /** Back-compat alias: select any family or custom theme by id. */
    setActiveTheme(state, action: PayloadAction<string>) {
      state.activeThemeId = action.payload;
    },
    /** Insert or replace a custom theme (by id) and make it active. */
    upsertCustomTheme(state, action: PayloadAction<Theme>) {
      const theme = action.payload;
      const idx = state.customThemes.findIndex((t) => t.id === theme.id);
      if (idx >= 0) state.customThemes[idx] = theme;
      else state.customThemes.push(theme);
      state.activeThemeId = theme.id;
    },
    /** Remove a custom theme; fall back to the default family if it was active. */
    deleteCustomTheme(state, action: PayloadAction<string>) {
      state.customThemes = state.customThemes.filter((t) => t.id !== action.payload);
      if (state.activeThemeId === action.payload) {
        state.activeThemeId = DEFAULT_FAMILY_ID;
      }
    },
    /** Set a single colour token (`"R G B"`) on the active custom theme. */
    setThemeToken(state, action: PayloadAction<{ key: string; value: string }>) {
      const theme = state.customThemes.find((t) => t.id === state.activeThemeId);
      if (!theme) return; // built-in/system active — panel duplicates first
      theme.colors[action.payload.key] = action.payload.value;
    },
    /** Set a single font role (CSS stack) on the active custom theme. */
    setFontRole(state, action: PayloadAction<{ role: FontRole; stack: string }>) {
      const theme = state.customThemes.find((t) => t.id === state.activeThemeId);
      if (!theme) return;
      theme.fonts[action.payload.role] = action.payload.stack;
    },
    /** Set the backdrop (mesh/solid/image) on the active custom theme. */
    setThemeBackdrop(
      state,
      action: PayloadAction<{ kind: 'mesh' | 'solid' | 'image'; imageUrl?: string }>,
    ) {
      const theme = state.customThemes.find((t) => t.id === state.activeThemeId);
      if (!theme) return;
      theme.backdrop = { kind: action.payload.kind, imageUrl: action.payload.imageUrl };
    },
    /** Clear all overrides on the active custom theme (back to its base). */
    resetActiveTheme(state) {
      const theme = state.customThemes.find((t) => t.id === state.activeThemeId);
      if (!theme) return;
      theme.colors = {};
      theme.fonts = {};
    },
    setTabBarLabels(state, action: PayloadAction<TabBarLabels>) {
      state.tabBarLabels = action.payload;
    },
    setFontSize(state, action: PayloadAction<FontSize>) {
      state.fontSize = action.payload;
    },
    setAgentMessageViewMode(state, action: PayloadAction<AgentMessageViewMode>) {
      state.agentMessageViewMode = action.payload;
    },
    setDeveloperMode(state, action: PayloadAction<boolean>) {
      state.developerMode = action.payload;
    },
    setHideAgentInsights(state, action: PayloadAction<boolean>) {
      state.hideAgentInsights = action.payload;
    },
  },
});

export const {
  setThemeMode,
  setThemeVariant,
  setActiveFamily,
  setTabBarLabels,
  setFontSize,
  setAgentMessageViewMode,
  setDeveloperMode,
  setHideAgentInsights,
  setActiveTheme,
  upsertCustomTheme,
  deleteCustomTheme,
  setThemeToken,
  setFontRole,
  setThemeBackdrop,
  resetActiveTheme,
} = themeSlice.actions;
export default themeSlice.reducer;

/** Built-in theme families (static). */
export const selectThemeFamilies = (): ThemeFamily[] => THEME_FAMILIES;

/**
 * All selectable concrete themes: built-in variants followed by user themes.
 * Memoized so the reference is stable while `customThemes` is unchanged.
 */
export const selectAllThemes = createSelector(
  (state: { theme: ThemeState }) => state.theme.customThemes,
  (customThemes): Theme[] => [...PRESET_THEMES, ...(customThemes ?? [])],
);

export const selectActiveThemeId = (state: { theme: ThemeState }): string =>
  state.theme.activeThemeId ?? DEFAULT_FAMILY_ID;

export const selectThemeVariant = (state: { theme: ThemeState }): ThemeVariant =>
  state.theme.themeVariant ?? 'system';

export const selectCustomThemes = (state: { theme: ThemeState }): Theme[] =>
  state.theme.customThemes ?? [];

/**
 * Normalize the active selection to a `{ family, variant, custom }` shape,
 * tolerating legacy persisted ids (`light`/`dark`/`system`/`ocean`/`midnight`).
 * Returns `custom` set when a user theme is selected.
 */
function resolveSelection(state: { theme: ThemeState }): {
  family?: ThemeFamily;
  variant: ThemeVariant;
  custom?: Theme;
} {
  const sel = state.theme.activeThemeId ?? DEFAULT_FAMILY_ID;
  const variantPref = state.theme.themeVariant ?? 'system';

  const custom = state.theme.customThemes?.find((t) => t.id === sel);
  if (custom) return { custom, variant: variantPref };

  // Current family-id selection.
  const direct = findFamily(sel);
  if (direct) return { family: direct, variant: variantPref };

  // Legacy concrete-variant / sentinel ids.
  if (sel === SYSTEM_THEME_ID) return { family: findFamily('classic'), variant: 'system' };
  if (sel === 'midnight') return { family: findFamily('ocean'), variant: 'dark' };
  const owner = familyForThemeId(sel);
  if (owner) return { family: owner, variant: owner.dark?.id === sel ? 'dark' : 'light' };
  return { family: findFamily('classic'), variant: variantPref };
}

/** The active family id (`''` when a custom theme is selected). */
export function selectActiveFamilyId(state: { theme: ThemeState }): string {
  const { family, custom } = resolveSelection(state);
  if (custom) return '';
  return family?.id ?? DEFAULT_FAMILY_ID;
}

/**
 * Resolve the effective {@link Theme} to apply right now. A custom theme is
 * returned directly; otherwise the active family's variant is resolved, with
 * `system` consulting `prefers-color-scheme`.
 */
export function selectEffectiveTheme(state: { theme: ThemeState }): Theme {
  const { family, variant, custom } = resolveSelection(state);
  if (custom) return custom;
  const fam = family ?? findFamily('classic')!;
  const resolved = variant === 'system' ? resolveTheme('system') : variant;
  return resolveFamilyVariant(fam, resolved);
}

/**
 * Selector for the persisted `hideAgentInsights` preference. Falls back to
 * `false` so existing persisted state (written before this field existed)
 * keeps the verbose timeline visible until the user opts out.
 */
export const selectHideAgentInsights = (state: { theme: ThemeState }): boolean =>
  state.theme.hideAgentInsights ?? false;

/**
 * Selector for the persisted `developerMode` preference.
 * Use {@link useDeveloperMode} in components — it combines this with `IS_DEV`.
 */
export const selectDeveloperMode = (state: { theme: ThemeState }): boolean =>
  state.theme.developerMode;

/**
 * Resolves a `ThemeMode` to the concrete `light` or `dark` value that should
 * be applied to `<html>`. `system` consults `prefers-color-scheme`; in non-DOM
 * contexts (SSR, tests without matchMedia) it falls back to light.
 */
export function resolveTheme(mode: ThemeMode): 'light' | 'dark' {
  if (mode !== 'system') return mode;
  try {
    if (typeof window !== 'undefined' && window.matchMedia) {
      return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
  } catch {
    // matchMedia unavailable
  }
  return 'light';
}
