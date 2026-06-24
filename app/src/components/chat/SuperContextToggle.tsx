import debugFactory from 'debug';
import { useCallback, useEffect, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { isTauri } from '../../utils/tauriCommands/common';
import {
  openhumanGetSuperContextEnabled,
  openhumanSetSuperContextEnabled,
} from '../../utils/tauriCommands/config';
import SettingsSwitch from '../settings/controls/SettingsSwitch';

const log = debugFactory('chat:super-context-toggle');

/**
 * "Super context" toggle, rendered directly below the chat composer.
 *
 * Flips the persistent `context.super_context_enabled` core config flag. When
 * on, the harness runs a read-only context-collection pass on the **first turn
 * of a new thread** — before the orchestrator LLM runs — and folds the result
 * into the user message. This is harness-driven (deterministic), unlike the
 * `agent_prepare_context` tool the model may call on its own.
 *
 * Because the flag is read at thread construction, toggling it only affects
 * threads started afterwards — surfaced to the user via the helper hint.
 */
const SuperContextToggle = () => {
  const { t } = useT();
  const [enabled, setEnabled] = useState(false);
  // Until the first read resolves we don't know the real value; keep the
  // switch disabled so a stray click can't write a stale default back. Outside
  // Tauri (Storybook/web preview) there's no core to read, so treat the control
  // as loaded immediately in its default-off state without hitting the RPC.
  const [loaded, setLoaded] = useState(() => !isTauri());
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const res = await openhumanGetSuperContextEnabled();
        if (!cancelled) {
          setEnabled(Boolean(res.result));
          log('loaded super_context_enabled=%o', res.result);
        }
      } catch (err) {
        // Best-effort: a read failure leaves the toggle in its default-off
        // state. The user can still flip it; the write path surfaces errors.
        log('failed to load super_context_enabled: %o', err);
      } finally {
        if (!cancelled) setLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleChange = useCallback(
    (next: boolean) => {
      if (busy) return;
      const previous = enabled;
      // Optimistic: reflect the choice immediately, roll back on failure.
      setEnabled(next);
      setBusy(true);
      log('set super_context_enabled -> %o', next);
      void (async () => {
        try {
          const res = await openhumanSetSuperContextEnabled(next);
          setEnabled(Boolean(res.result));
        } catch (err) {
          log('failed to persist super_context_enabled, rolling back: %o', err);
          setEnabled(previous);
        } finally {
          setBusy(false);
        }
      })();
    },
    [busy, enabled]
  );

  return (
    <div className="flex items-center gap-2 px-1 pt-1.5 text-xs text-stone-500 dark:text-neutral-400">
      <SettingsSwitch
        id="super-context-toggle"
        checked={enabled}
        onCheckedChange={handleChange}
        disabled={!loaded || busy}
        aria-label={t('chat.superContext.label')}
        data-testid="super-context-toggle"
      />
      <span className="font-medium text-stone-600 dark:text-neutral-300">
        {t('chat.superContext.label')}
      </span>
      <span className="text-stone-400 dark:text-neutral-500 truncate">
        {t('chat.superContext.hint')}
      </span>
    </div>
  );
};

export default SuperContextToggle;
