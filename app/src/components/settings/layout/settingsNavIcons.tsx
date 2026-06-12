import type { ReactNode } from 'react';

// ---------------------------------------------------------------------------
// Sidebar icons, keyed by settings registry entry id. Consolidates the SVGs
// previously duplicated across SettingsHome.tsx and Settings.tsx.
// ---------------------------------------------------------------------------

const icon = (path: ReactNode) => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    {path}
  </svg>
);

const stroke = (d: string) => (
  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d={d} />
);

export const SETTINGS_NAV_ICONS: Record<string, ReactNode> = {
  account: icon(stroke('M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z')),
  appearance: icon(stroke('M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z')),
  notifications: icon(
    stroke(
      'M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9'
    )
  ),
  llm: icon(
    stroke(
      'M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z'
    )
  ),
  voice: icon(
    stroke(
      'M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m0 0H8m4 0h4m-4-8a3 3 0 01-3-3V5a3 3 0 116 0v6a3 3 0 01-3 3z'
    )
  ),
  personality: icon(
    stroke(
      'M12 21a9 9 0 100-18 9 9 0 000 18zM9 10h.01M15 10h.01M9.5 15c.83.67 1.67 1 2.5 1s1.67-.33 2.5-1'
    )
  ),
  agents: icon(
    stroke(
      'M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2z'
    )
  ),
  devices: icon(
    stroke('M12 18h.01M8 21h8a2 2 0 002-2V5a2 2 0 00-2-2H8a2 2 0 00-2 2v14a2 2 0 002 2z')
  ),
  'memory-sync': icon(
    stroke(
      'M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15'
    )
  ),
  'wallet-balances': icon(
    stroke('M3 10h18M7 15h1m4 0h1m-7 4h12a3 3 0 003-3V8a3 3 0 00-3-3H6a3 3 0 00-3 3v8a3 3 0 003 3z')
  ),
  integrations: icon(stroke('M13 10V3L4 14h7v7l9-11h-7z')),
  'screen-intelligence': icon(stroke('M3 5h18v12H3zM8 21h8m-4-4v4')),
  tools: icon(
    stroke(
      'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065zM15 12a3 3 0 11-6 0 3 3 0 016 0z'
    )
  ),
  companion: icon(
    stroke(
      'M8 10h.01M12 10h.01M16 10h.01M9 16H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-5l-5 5v-5z'
    )
  ),
  'developer-options': icon(stroke('M10 20l4-16m4 4l4 4-4 4M6 16l-4-4 4-4')),
  about: icon(stroke('M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z')),
};
