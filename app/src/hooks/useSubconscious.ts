/**
 * useSubconscious — hook for the subconscious engine UI.
 *
 * Provides status, mode control, and engine actions for the
 * subconscious tab on the Intelligence page.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

import {
  isTauri,
  openhumanHeartbeatSettingsGet,
  openhumanHeartbeatSettingsSet,
  subconsciousStatus,
  subconsciousTrigger,
} from '../utils/tauriCommands';
import type { SubconsciousMode } from '../utils/tauriCommands/heartbeat';
import type { SubconsciousStatus } from '../utils/tauriCommands/subconscious';

export interface UseSubconsciousResult {
  status: SubconsciousStatus | null;
  mode: SubconsciousMode;
  loading: boolean;
  triggering: boolean;
  settingMode: boolean;
  refresh: () => Promise<void>;
  triggerTick: () => Promise<void>;
  setMode: (mode: SubconsciousMode) => Promise<void>;
  error: string | null;
}

export function useSubconscious(): UseSubconsciousResult {
  const [status, setStatus] = useState<SubconsciousStatus | null>(null);
  const [mode, setModeState] = useState<SubconsciousMode>('off');
  const [loading, setLoading] = useState(false);
  const [triggering, setTriggering] = useState(false);
  const [settingMode, setSettingMode] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fetchingRef = useRef(false);

  const refresh = useCallback(async () => {
    if (!isTauri() || fetchingRef.current) return;
    fetchingRef.current = true;
    setLoading(true);
    setError(null);
    try {
      const [statusRes, settingsRes] = await Promise.all([
        withTimeout(subconsciousStatus()),
        withTimeout(openhumanHeartbeatSettingsGet()),
      ]);
      if (statusRes) setStatus(unwrap(statusRes) ?? null);
      const settings = settingsRes ? unwrap<{ settings: { subconscious_mode: SubconsciousMode } }>(settingsRes) : null;
      if (settings?.settings?.subconscious_mode) {
        setModeState(settings.settings.subconscious_mode);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load subconscious data');
    } finally {
      setLoading(false);
      fetchingRef.current = false;
    }
  }, []);

  const triggerTick = useCallback(async () => {
    if (!isTauri() || triggering) return;
    setTriggering(true);
    try {
      await subconsciousTrigger();
    } catch (err) {
      console.warn('[subconscious] trigger failed:', err);
    } finally {
      setTriggering(false);
    }
  }, [triggering]);

  const setMode = useCallback(
    async (newMode: SubconsciousMode) => {
      if (!isTauri()) return;
      setSettingMode(true);
      setModeState(newMode);
      try {
        await openhumanHeartbeatSettingsSet({ subconscious_mode: newMode });
        await refresh();
      } catch (err) {
        console.warn('[subconscious] setMode failed:', err);
        setError(err instanceof Error ? err.message : 'Failed to update mode');
      } finally {
        setSettingMode(false);
      }
    },
    [refresh]
  );

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 5000);
    return () => {
      clearInterval(interval);
      fetchingRef.current = false;
    };
  }, [refresh]);

  return {
    status,
    mode,
    loading,
    triggering,
    settingMode,
    refresh,
    triggerTick,
    setMode,
    error,
  };
}

const RPC_TIMEOUT_MS = 2500;

function withTimeout<T>(promise: Promise<T>, ms: number = RPC_TIMEOUT_MS): Promise<T | null> {
  return Promise.race<T | null>([
    promise.catch(() => null),
    new Promise<null>(resolve => setTimeout(() => resolve(null), ms)),
  ]);
}

function unwrap<T>(response: unknown): T | null {
  if (!response || typeof response !== 'object') return null;
  const r = response as Record<string, unknown>;
  if ('result' in r) {
    return r.result as T;
  }
  return null;
}
