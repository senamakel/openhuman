import debugFactory from 'debug';
import { useCallback, useEffect, useRef, useState } from 'react';

import {
  fetchHarnessInitStatus,
  type HarnessInitSnapshot,
  runHarnessInit,
} from '../../services/harnessInitService';
import InitProgressScreen from './InitProgressScreen';

const log = debugFactory('harness-init');

const POLL_MS = 2000;

// Persist the "Run in background" dismissal for the *current* provisioning run
// so a remount or reload does not reopen the overlay (GH-5047). A run is keyed
// by its `startedAt` timestamp — a genuinely new provisioning run gets a fresh
// timestamp and is allowed to surface again. `sessionStorage` survives a
// renderer reload within the same window; a module-level mirror covers plain
// React remounts even if storage is unavailable.
const DISMISS_KEY = 'harness-init-dismissed-run';
// Runs before `startedAt` is stamped (or when it is absent) still need a stable
// key so an early dismissal sticks.
const UNKEYED_RUN = 'pending';

let dismissedRunMirror: string | null = null;

// Coalesce overlapping status polls onto a single in-flight request. React
// StrictMode double-mounts this overlay in dev (effect → cleanup → effect),
// and each setup fires an immediate poll — without this that boots two
// `harness_init_status` RPCs at the same instant. Concurrent callers share the
// in-flight promise; it clears once settled, so the ongoing (sequential) poll
// loop is unaffected. Also guards any genuine remount during the boot window.
let inflightStatusFetch: Promise<HarnessInitSnapshot | null> | null = null;

function fetchHarnessInitStatusCoalesced(): Promise<HarnessInitSnapshot | null> {
  if (inflightStatusFetch) {
    log('status poll: joining in-flight request (coalesced)');
    return inflightStatusFetch;
  }
  log('status poll: dispatching harness_init_status');
  const pending = fetchHarnessInitStatus().finally(() => {
    if (inflightStatusFetch === pending) {
      inflightStatusFetch = null;
      log('status poll: in-flight request settled, cache cleared');
    }
  });
  inflightStatusFetch = pending;
  return pending;
}

function runKey(snapshot: HarnessInitSnapshot | null): string {
  return snapshot?.startedAt ?? UNKEYED_RUN;
}

function readDismissedRun(): string | null {
  if (dismissedRunMirror !== null) {
    return dismissedRunMirror;
  }
  try {
    return window.sessionStorage.getItem(DISMISS_KEY);
  } catch {
    log('sessionStorage read failed; treating run as not dismissed');
    return null;
  }
}

function writeDismissedRun(key: string): void {
  dismissedRunMirror = key;
  try {
    window.sessionStorage.setItem(DISMISS_KEY, key);
    log('dismissed run persisted to sessionStorage: %s', key);
  } catch {
    // Non-fatal: the module-level mirror still guards remounts this session.
    log('sessionStorage unavailable; dismissed run %s held in module mirror only', key);
  }
}

function isRunDismissed(snapshot: HarnessInitSnapshot | null): boolean {
  return readDismissedRun() === runKey(snapshot);
}

/**
 * Blocking first-run initialization gate.
 *
 * Polls `openhuman.harness_init_status` and, while the run is in progress,
 * covers the app with a full-screen overlay showing per-step progress. The
 * overlay offers a "Run in background" action so the user can dismiss it and
 * keep working while setup continues — the core runs init as a background task
 * regardless of whether the overlay is shown. On a warm host every step is
 * already provisioned, so the snapshot reports `done` on the first poll and
 * this renders nothing. On a terminal `failed` it offers Retry / Continue —
 * failures are non-fatal (the core degrades to a fallback).
 *
 * Polling-based (not socket) to sidestep the cold-start race where the socket
 * is not yet connected when init begins.
 */
export default function HarnessInitOverlay() {
  const [snapshot, setSnapshot] = useState<HarnessInitSnapshot | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const cancelledRef = useRef(false);
  // Mirrors `dismissed` so the poll loop can stop without re-running the effect.
  const dismissedRef = useRef(false);

  useEffect(() => {
    cancelledRef.current = false;
    let timeoutId: number | null = null;

    const poll = async () => {
      try {
        const next = await fetchHarnessInitStatusCoalesced();
        if (cancelledRef.current || dismissedRef.current) {
          return;
        }
        if (next) {
          setSnapshot(next);
          // If this run was already dismissed to the background (possibly in a
          // prior mount / before a reload), stay hidden and stop polling —
          // don't let a remount reopen the overlay (GH-5047).
          if (isRunDismissed(next)) {
            log(
              'warm poll: run %s already dismissed — staying hidden, stopping poll',
              runKey(next)
            );
            dismissedRef.current = true;
            setDismissed(true);
            return;
          }
          // Stop polling once the run is terminal; a `failed` snapshot stays
          // on screen (with Retry) but does not need further polling.
          if (next.overall === 'done' || next.overall === 'failed') {
            return;
          }
        }
      } catch (err) {
        // Status can fail while the core is still coming up — keep polling.
        log('status poll failed: %O', err);
      }
      if (!cancelledRef.current && !dismissedRef.current) {
        timeoutId = window.setTimeout(() => void poll(), POLL_MS);
      }
    };

    void poll();

    return () => {
      cancelledRef.current = true;
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
    };
  }, []);

  const handleRetry = useCallback(async () => {
    setRetrying(true);
    try {
      const next = await runHarnessInit(false);
      if (next) {
        setSnapshot(next);
      }
    } catch (err) {
      log('retry failed: %O', err);
    } finally {
      setRetrying(false);
    }
  }, []);

  const handleContinue = useCallback(() => {
    // Hide the overlay and stop polling; the core keeps running init as a
    // background task regardless. Persist the dismissal for this run so a
    // remount/reload does not reopen it (GH-5047).
    log('user dismissed overlay to background for run %s', runKey(snapshot));
    writeDismissedRun(runKey(snapshot));
    dismissedRef.current = true;
    setDismissed(true);
  }, [snapshot]);

  if (dismissed || !snapshot) {
    return null;
  }

  // A run dismissed to the background stays hidden across remounts.
  if (isRunDismissed(snapshot)) {
    return null;
  }

  // Block only while a run is actively in progress, or hold a failed run on
  // screen until the user explicitly continues. `idle` (no run started yet)
  // and `done` never block.
  const shouldShow = snapshot.overall === 'running' || snapshot.overall === 'failed';
  if (!shouldShow) {
    return null;
  }

  return (
    <InitProgressScreen
      snapshot={snapshot}
      onRetry={handleRetry}
      onContinue={handleContinue}
      retrying={retrying}
    />
  );
}
