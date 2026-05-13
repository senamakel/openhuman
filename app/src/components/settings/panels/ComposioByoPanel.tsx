import { useEffect, useRef, useState } from 'react';

import {
  openhumanGetComposioByoStatus,
  openhumanUpdateComposioByoSettings,
} from '../../../utils/tauriCommands';
import type { ComposioByoStatus } from '../../../utils/tauriCommands/config';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

/**
 * BYO Composio Settings.
 *
 * Lets the user paste their own Composio API key. When set, the Rust
 * core bypasses the openhuman backend proxy and talks to
 * `api.composio.dev` directly. Triggers (webhooks) are unsupported in
 * BYO mode — the panel surfaces that limitation explicitly so the user
 * isn't surprised.
 *
 * Implementation note: the masked key (`sk_••••••abcd`) is the only
 * preview the core sends back. The full key is never echoed to the
 * renderer once saved — re-entering replaces it.
 */
const ComposioByoPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();

  const [status, setStatus] = useState<ComposioByoStatus | null>(null);
  const [keyInput, setKeyInput] = useState('');
  const [revealInput, setRevealInput] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<'idle' | 'saved' | 'cleared' | 'error'>('idle');
  const saveStatusTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refresh = async () => {
    try {
      const res = await openhumanGetComposioByoStatus();
      if (res.result) {
        setStatus(res.result);
      }
    } catch (err) {
      console.warn('[ComposioByoPanel] failed to load status:', err);
    }
  };

  useEffect(() => {
    let isMounted = true;
    refresh()
      .catch(err => {
        if (!isMounted) return;
        console.warn('[ComposioByoPanel] initial load failed:', err);
      })
      .finally(() => {
        if (isMounted) setLoading(false);
      });

    return () => {
      isMounted = false;
      if (saveStatusTimer.current !== null) {
        clearTimeout(saveStatusTimer.current);
      }
    };
  }, []);

  const flashStatus = (next: 'saved' | 'cleared' | 'error') => {
    setSaveStatus(next);
    if (saveStatusTimer.current !== null) clearTimeout(saveStatusTimer.current);
    saveStatusTimer.current = setTimeout(() => setSaveStatus('idle'), 3000);
  };

  const handleSave = async () => {
    const trimmed = keyInput.trim();
    if (!trimmed) return;
    setSaving(true);
    try {
      await openhumanUpdateComposioByoSettings({ byo_api_key: trimmed });
      setKeyInput('');
      await refresh();
      flashStatus('saved');
    } catch (err) {
      console.warn('[ComposioByoPanel] failed to save BYO key:', err);
      flashStatus('error');
    } finally {
      setSaving(false);
    }
  };

  const handleClear = async () => {
    setSaving(true);
    try {
      await openhumanUpdateComposioByoSettings({ byo_api_key: null });
      setKeyInput('');
      await refresh();
      flashStatus('cleared');
    } catch (err) {
      console.warn('[ComposioByoPanel] failed to clear BYO key:', err);
      flashStatus('error');
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div>
        <SettingsHeader
          title="Composio API Key"
          showBackButton
          onBack={navigateBack}
          breadcrumbs={breadcrumbs}
        />
        <div className="p-4">
          <p className="text-sm text-stone-500">Loading…</p>
        </div>
      </div>
    );
  }

  const isByoActive = status?.active === true;

  return (
    <div>
      <SettingsHeader
        title="Composio API Key"
        showBackButton
        onBack={navigateBack}
        breadcrumbs={breadcrumbs}
      />

      <div className="p-4 space-y-5">
        <p className="text-sm text-stone-500">
          By default OpenHuman talks to Composio through the hosted openhuman backend. If you want
          to manage your own toolkit allowlist, billing, and security boundaries, you can supply
          your own Composio API key here — the core will then call{' '}
          <span className="font-mono">api.composio.dev</span> directly with your key, and the hosted
          backend never sees your tool calls.
        </p>

        {/* Status card */}
        <div
          className={`rounded-2xl border p-4 ${
            isByoActive ? 'border-primary-200 bg-primary-50/60' : 'border-stone-200 bg-stone-50/60'
          }`}>
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium text-stone-900">
              {isByoActive ? 'Using your Composio key' : 'Using hosted openhuman backend'}
            </span>
            <span
              className={`text-xs font-mono px-2 py-0.5 rounded ${
                isByoActive ? 'bg-primary-100 text-primary-800' : 'bg-stone-200 text-stone-700'
              }`}
              aria-label={`Current backend: ${status?.backend ?? 'proxy'}`}>
              {status?.backend ?? 'proxy'}
            </span>
          </div>
          {isByoActive && status?.masked_key ? (
            <p className="mt-1 text-xs text-stone-500">
              Active key: <span className="font-mono">{status.masked_key}</span>
            </p>
          ) : (
            <p className="mt-1 text-xs text-stone-500">
              No BYO key configured. Paste a key below to switch.
            </p>
          )}
        </div>

        {/* BYO unsupported features */}
        <div className="rounded-2xl border border-amber-200 bg-amber-50/60 p-4">
          <p className="text-sm font-medium text-amber-900">
            What BYO mode supports — and what it doesn’t
          </p>
          <ul className="mt-2 space-y-1 text-xs text-amber-900/90 list-disc pl-5">
            <li>Listing toolkits, connections, tools, and executing tool calls — supported.</li>
            <li>
              Composio <em>triggers</em> (webhook-driven events) — <strong>not supported</strong>.
              Triggers need an internet-reachable webhook endpoint; the hosted backend provides
              that. To use triggers, clear your BYO key and switch back to the hosted backend.
            </li>
            <li>
              New OAuth handoff (<span className="font-mono">authorize</span>) — connect accounts in
              your Composio dashboard, then they’ll appear here.
            </li>
          </ul>
        </div>

        {/* Key input */}
        <div className="space-y-2">
          <label className="block text-sm font-medium text-stone-800" htmlFor="composio-byo-key">
            {isByoActive ? 'Replace your Composio API key' : 'Composio API key'}
          </label>
          <div className="flex gap-2">
            <input
              id="composio-byo-key"
              type={revealInput ? 'text' : 'password'}
              value={keyInput}
              onChange={e => setKeyInput(e.target.value)}
              placeholder="sk_live_…"
              autoComplete="off"
              spellCheck={false}
              className="flex-1 rounded-xl border border-stone-200 bg-white px-3 py-2 text-sm font-mono text-stone-900 placeholder-stone-400 focus:border-primary-400 focus:outline-none focus:ring-1 focus:ring-primary-400"
            />
            <button
              type="button"
              onClick={() => setRevealInput(v => !v)}
              className="px-3 rounded-xl border border-stone-200 bg-white text-xs text-stone-700 hover:bg-stone-50"
              aria-pressed={revealInput}>
              {revealInput ? 'Hide' : 'Show'}
            </button>
          </div>
          <p className="text-xs text-stone-500">
            Stored locally in your config.toml. Never sent to the openhuman backend.
          </p>
        </div>

        <div className="flex gap-2">
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || keyInput.trim().length === 0}
            className="flex-1 py-2 rounded-xl bg-primary-600 text-white text-sm font-medium hover:bg-primary-500 transition-colors disabled:opacity-50">
            {saving ? 'Saving…' : isByoActive ? 'Replace key' : 'Save key'}
          </button>
          {isByoActive && (
            <button
              type="button"
              onClick={handleClear}
              disabled={saving}
              className="py-2 px-4 rounded-xl border border-coral-200 bg-coral-50 text-coral-800 text-sm font-medium hover:bg-coral-100 transition-colors disabled:opacity-50">
              Clear & use hosted backend
            </button>
          )}
        </div>

        {saveStatus === 'saved' && (
          <p className="text-xs text-center text-green-600">
            BYO key saved. Core is now in direct mode.
          </p>
        )}
        {saveStatus === 'cleared' && (
          <p className="text-xs text-center text-stone-600">
            BYO key cleared. Reverted to hosted backend.
          </p>
        )}
        {saveStatus === 'error' && (
          <p className="text-xs text-center text-red-500">Failed to update. Try again.</p>
        )}
      </div>
    </div>
  );
};

export default ComposioByoPanel;
