import { createSelector, createSlice, type PayloadAction } from '@reduxjs/toolkit';

import type { FontRole } from '../lib/theme/tokens';
import type { Theme } from '../lib/theme/types';
import { PRESET_THEMES, findPreset, LIGHT_THEME_ID, DARK_THEME_ID } from '../lib/theme/presets';

export type ThemeMode = 'light' | 'dark' | 'system';

/** Sentinel active-theme id meaning "follow OS light/dark preference". */
export const SYSTEM_THEME_ID = 'system';
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
   * Active theme id. Drives the runtime CSS-variable theme applied by
   * ThemeProvider. May be {@link SYSTEM_THEME_ID} (follow OS light/dark), a
   * built-in preset id (`light`, `dark`, `ocean`, …), or a custom theme id.
   * Kept in sync with {@link ThemeState.mode} for the simple Appearance toggle.
   */
  activeThemeId: string;
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
  activeThemeId: SYSTEM_THEME_ID,
  customThemes: [],
};

const themeSlice = createSlice({
  name: 'theme',
  initialState,
  reducers: {
    setThemeMode(state, action: PayloadAction<ThemeMode>) {
      state.mode = action.payload;
      // Keep the runtime theme in sync with the simple light/dark/system toggle.
      state.activeThemeId =
        action.payload === 'system'
          ? SYSTEM_THEME_ID
          : action.payload === 'dark'
            ? DARK_THEME_ID
            : LIGHT_THEME_ID;
    },
    /** Select any theme (preset, custom, or the `system` sentinel). */
    setActiveTheme(state, action: PayloadAction<string>) {
      state.activeThemeId = action.payload;
      // Mirror into `mode` so the Appearance radios stay coherent for the
      // three values they represent; custom/extra presets leave `mode` as-is.
      if (action.payload === SYSTEM_THEME_ID) state.mode = 'system';
      else if (action.payload === LIGHT_THEME_ID) state.mode = 'light';
      else if (action.payload === DARK_THEME_ID) state.mode = 'dark';
    },
    /** Insert or replace a custom theme (by id) and make it active. */
    upsertCustomTheme(state, action: PayloadAction<Theme>) {
      const theme = action.payload;
      const idx = state.customThemes.findIndex((t) => t.id === theme.id);
      if (idx >= 0) state.customThemes[idx] = theme;
      else state.customThemes.push(theme);
      state.activeThemeId = theme.id;
    },
    /** Remove a custom theme; fall back to `system` if it was active. */
    deleteCustomTheme(state, action: PayloadAction<string>) {
      state.customThemes = state.customThemes.filter((t) => t.id !== action.payload);
      if (state.activeThemeId === action.payload) {
        state.activeThemeId = SYSTEM_THEME_ID;
        state.mode = 'system';
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
  resetActiveTheme,
} = themeSlice.actions;
export default themeSlice.reducer;

/**
 * All selectable themes: built-in presets followed by user-authored ones.
 * Memoized so it returns a stable array reference while `customThemes` is
 * unchanged (a fresh array each call would defeat React-Redux render bailout).
 */
export const selectAllThemes = createSelector(
  (state: { theme: ThemeState }) => state.theme.customThemes,
  (customThemes): Theme[] => [...PRESET_THEMES, ...(customThemes ?? [])],
);

export const selectActiveThemeId = (state: { theme: ThemeState }): string =>
  state.theme.activeThemeId ?? SYSTEM_THEME_ID;

export const selectCustomThemes = (state: { theme: ThemeState }): Theme[] =>
  state.theme.customThemes ?? [];

/**
 * Resolve the effective {@link Theme} to apply right now. The `system` sentinel
 * resolves to the Light or Dark preset via `prefers-color-scheme`; a missing /
 * unknown id falls back to Light so the UI is never left unthemed.
 */
export function selectEffectiveTheme(state: { theme: ThemeState }): Theme {
  const id = state.theme.activeThemeId ?? SYSTEM_THEME_ID;
  if (id === SYSTEM_THEME_ID) {
    return findPreset(resolveTheme('system') === 'dark' ? DARK_THEME_ID : LIGHT_THEME_ID)!;
  }
  const custom = state.theme.customThemes?.find((t) => t.id === id);
  if (custom) return custom;
  return findPreset(id) ?? findPreset(LIGHT_THEME_ID)!;
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
