import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';

import {
  applyAppUpdate,
  type AppUpdateInfo,
  checkAppUpdate,
  isTauri,
} from '../utils/tauriCommands';

type Phase = 'idle' | 'prompt' | 'checking' | 'downloading' | 'installing' | 'restarting' | 'error';

const formatBytes = (n: number): string => {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
};

/**
 * Auto-update prompt for the Tauri shell. On every app load this:
 *
 *  1. Calls `checkAppUpdate()` once.
 *  2. If the configured updater endpoint advertises a newer version, shows a
 *     non-blocking floating prompt with **Install now** and **Later**.
 *  3. *Later* dismisses the prompt for the current session only — no
 *     persistence, so the next app launch shows it again. (Per product:
 *     a missed update should keep nagging; persistence would let users
 *     accidentally disable updates forever.)
 *  4. *Install now* swaps the floating prompt for a centered modal that
 *     reflects the backend `app-update:status` / `app-update:progress`
 *     events (downloading → installing → restarting). The Rust side
 *     finishes by calling `app.restart()`, which terminates this
 *     webview, so the modal effectively lives until process exit.
 */
export default function AppUpdatePrompt() {
  const [info, setInfo] = useState<AppUpdateInfo | null>(null);
  const [phase, setPhase] = useState<Phase>('idle');
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const checkedRef = useRef(false);

  // Probe for an available update once on mount.
  useEffect(() => {
    if (!isTauri() || checkedRef.current) return;
    checkedRef.current = true;
    void (async () => {
      try {
        const result = await checkAppUpdate();
        if (result?.available && result.available_version) {
          setInfo(result);
          setPhase('prompt');
        }
      } catch (err) {
        console.warn('[app-update] check failed:', err);
      }
    })();
  }, []);

  // Subscribe to backend update events.
  useEffect(() => {
    if (!isTauri()) return;
    let unlistenStatus: UnlistenFn | undefined;
    let unlistenProgress: UnlistenFn | undefined;
    let cancelled = false;
    void (async () => {
      const statusUnlisten = await listen<string>('app-update:status', e => {
        const v = e.payload;
        if (v === 'checking' || v === 'downloading' || v === 'installing' || v === 'restarting') {
          setPhase(v);
        } else if (v === 'error') {
          setPhase('error');
          setError(prev => prev ?? 'Update failed.');
        } else if (v === 'up_to_date') {
          // Backend says no work needed; tear the modal down.
          setPhase('idle');
        }
      });
      const progressUnlisten = await listen<{ chunk: number; total: number | null }>(
        'app-update:progress',
        e => {
          const chunk = e.payload?.chunk ?? 0;
          setDownloaded(prev => prev + chunk);
          if (e.payload?.total != null) setTotal(e.payload.total);
        }
      );
      if (cancelled) {
        statusUnlisten();
        progressUnlisten();
        return;
      }
      unlistenStatus = statusUnlisten;
      unlistenProgress = progressUnlisten;
    })();
    return () => {
      cancelled = true;
      unlistenStatus?.();
      unlistenProgress?.();
    };
  }, []);

  const onInstall = async () => {
    setError(null);
    setDownloaded(0);
    setTotal(null);
    setPhase('checking');
    try {
      // The Rust side calls `app.restart()` on success, which terminates
      // this webview before the promise can resolve. A return here means
      // there was nothing to install (e.g. endpoint moved on between the
      // initial probe and this click).
      await applyAppUpdate();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPhase('error');
    }
  };

  if (!isTauri() || phase === 'idle') return null;
  if (phase === 'prompt' && dismissed) return null;

  if (phase === 'prompt' && info) {
    return (
      <div className="fixed bottom-4 right-4 z-[9999] w-[320px] animate-fade-up">
        <div className="bg-stone-900 border border-stone-700/50 rounded-2xl shadow-large overflow-hidden">
          <div className="px-4 pt-3 pb-2 flex items-center gap-2">
            <svg className="w-4 h-4 text-primary-400" viewBox="0 0 16 16" fill="currentColor">
              <path d="M8 1.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13zM8 4a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 018 4zm0 7.5a1 1 0 100-2 1 1 0 000 2z" />
            </svg>
            <span className="text-sm font-medium text-white">Update available</span>
          </div>
          <div className="px-4 pb-3 text-xs text-stone-300">
            v{info.current_version} → v{info.available_version}
            {info.body ? (
              <div className="mt-1.5 text-stone-400 line-clamp-3">{info.body}</div>
            ) : null}
          </div>
          <div className="px-3 pb-3 flex gap-2 justify-end">
            <button
              type="button"
              className="px-3 py-1.5 text-xs font-medium rounded-lg text-stone-300 hover:bg-stone-800 transition-colors"
              onClick={() => setDismissed(true)}>
              Later
            </button>
            <button
              type="button"
              className="px-3 py-1.5 text-xs font-medium rounded-lg bg-primary-500 text-white hover:bg-primary-400 transition-colors"
              onClick={onInstall}>
              Install now
            </button>
          </div>
        </div>
      </div>
    );
  }

  // Installing / downloading / restarting / error → centered modal.
  const pct = total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
  const heading =
    phase === 'error'
      ? 'Update failed'
      : phase === 'restarting'
        ? 'Restarting…'
        : phase === 'installing'
          ? 'Installing update'
          : phase === 'checking'
            ? 'Checking for update…'
            : 'Downloading update';

  return (
    <div className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/40 backdrop-blur-sm animate-fade-in">
      <div className="w-[400px] bg-stone-900 border border-stone-700/50 rounded-2xl shadow-large p-6 flex flex-col gap-4">
        <div className="text-base font-semibold text-white">{heading}</div>
        {info ? (
          <div className="text-xs text-stone-400">
            v{info.current_version} → v{info.available_version}
          </div>
        ) : null}

        {phase === 'downloading' ? (
          <div>
            <div className="h-2 rounded-full bg-stone-800 overflow-hidden">
              <div
                className="h-full bg-primary-500 transition-[width] duration-150"
                style={{ width: pct != null ? `${pct}%` : '0%' }}
              />
            </div>
            <div className="text-xs text-stone-400 mt-2 flex justify-between">
              <span>{pct != null ? `${pct}%` : 'starting…'}</span>
              <span>
                {formatBytes(downloaded)}
                {total ? ` / ${formatBytes(total)}` : ''}
              </span>
            </div>
          </div>
        ) : phase !== 'error' ? (
          // checking / installing / restarting — indeterminate bar
          <div className="h-2 rounded-full bg-stone-800 overflow-hidden">
            <div className="h-full w-1/3 bg-primary-500 animate-pulse" />
          </div>
        ) : null}

        {phase === 'error' ? (
          <>
            <div className="text-sm text-coral-400">{error}</div>
            <div className="flex justify-end">
              <button
                type="button"
                className="px-3 py-1.5 text-xs font-medium rounded-lg bg-stone-800 text-stone-200 hover:bg-stone-700 transition-colors"
                onClick={() => {
                  setPhase('idle');
                  setError(null);
                }}>
                Dismiss
              </button>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
