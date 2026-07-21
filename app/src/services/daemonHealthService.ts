/**
 * Daemon Health Service
 *
 * Keeps the frontend daemon store in sync with the Rust core's component health.
 *
 * Health is no longer polled on its own timer: the core folds its health
 * snapshot into `app_state_snapshot`, and `CoreStateProvider` feeds each
 * snapshot's `health` payload here via {@link ingestHealthSnapshot}. That
 * collapses the former separate `health_snapshot` poll into the one app-state
 * poll. This service now owns only the parse + store update + the
 * disconnect-timeout watchdog (no data yet after {@link HEALTH_TIMEOUT_MS} →
 * mark the daemon disconnected).
 */
import {
  type ComponentHealth,
  type HealthSnapshot,
  setDaemonStatus,
  updateHealthSnapshot,
} from '../features/daemon/store';
import { getCoreStateSnapshot } from '../lib/coreState/store';

export class DaemonHealthService {
  private healthTimeoutId: ReturnType<typeof setTimeout> | null = null;
  // Health now arrives folded into `app_state_snapshot`, which is allowed to run
  // for up to `SNAPSHOT_TIMEOUT_MS` (90s) — first-launch snapshots legitimately
  // take 30–40s. The disconnect watchdog must therefore tolerate one worst-case
  // slow snapshot (plus the poll cadence) between successful ingests, or a merely
  // slow-but-alive core would be marked `disconnected`. 120s covers the 90s cap
  // with margin; genuine disconnection (snapshots stop succeeding entirely) is
  // still detected, just less aggressively than the old dedicated 2s poll.
  private readonly HEALTH_TIMEOUT_MS = 120000;

  /**
   * Arm the disconnect watchdog once when daemon-health tracking starts, if it
   * isn't already armed. Without this, a core whose `app_state_snapshot`s never
   * succeed (repeated timeouts) — after `useDaemonHealth`'s one-shot agent probe
   * has set the status to `running` — would never arm a watchdog and stick at
   * `running` forever. The baseline watchdog guarantees a fallback to
   * `disconnected` if no snapshot ever arrives, and is re-armed by each ingest.
   */
  ensureWatchdogArmed(): void {
    if (this.healthTimeoutId === null) {
      this.startHealthTimeout();
    }
  }

  /**
   * Ingest a health payload carried by an `app_state_snapshot` refresh.
   *
   * The snapshot arriving at all is proof the core is alive, so the disconnect
   * watchdog is re-armed unconditionally — even when the payload is missing or
   * unparseable (an older core that doesn't fold health, or a partial payload) —
   * otherwise a live-but-health-less core would eventually be marked
   * `disconnected`. The daemon store is only updated when a valid health
   * snapshot is present; otherwise it keeps its last-known state.
   */
  ingestHealthSnapshot(payload: unknown): void {
    // Called by CoreStateProvider only after a successful snapshot → liveness.
    this.startHealthTimeout();
    const healthSnapshot = this.parseHealthSnapshot(payload);
    if (healthSnapshot) {
      this.updateDaemonStoreFromHealth(healthSnapshot);
    }
  }

  cleanup(): void {
    if (this.healthTimeoutId) {
      clearTimeout(this.healthTimeoutId);
      this.healthTimeoutId = null;
    }
  }

  private parseHealthSnapshot(payload: unknown): HealthSnapshot | null {
    try {
      if (!payload || typeof payload !== 'object') {
        return null;
      }

      const data = payload as Record<string, unknown>;
      if (
        typeof data.pid !== 'number' ||
        typeof data.updated_at !== 'string' ||
        typeof data.uptime_seconds !== 'number' ||
        !data.components ||
        typeof data.components !== 'object'
      ) {
        return null;
      }

      const components: Record<string, ComponentHealth> = {};
      const componentsData = data.components as Record<string, unknown>;

      for (const [name, component] of Object.entries(componentsData)) {
        if (!component || typeof component !== 'object') {
          continue;
        }

        const comp = component as Record<string, unknown>;
        if (
          typeof comp.status !== 'string' ||
          typeof comp.updated_at !== 'string' ||
          typeof comp.restart_count !== 'number'
        ) {
          continue;
        }

        if (comp.status !== 'ok' && comp.status !== 'error' && comp.status !== 'starting') {
          continue;
        }

        components[name] = {
          status: comp.status,
          updated_at: comp.updated_at,
          last_ok: typeof comp.last_ok === 'string' ? comp.last_ok : undefined,
          last_error: typeof comp.last_error === 'string' ? comp.last_error : undefined,
          restart_count: comp.restart_count,
        };
      }

      return {
        pid: data.pid,
        updated_at: data.updated_at,
        uptime_seconds: data.uptime_seconds,
        components,
      };
    } catch (error) {
      console.error('[DaemonHealth] Error parsing health snapshot:', error);
      return null;
    }
  }

  private updateDaemonStoreFromHealth(snapshot: HealthSnapshot): void {
    try {
      updateHealthSnapshot(this.getUserId(), snapshot);
    } catch (error) {
      console.error('[DaemonHealth] Error updating daemon store from health:', error);
    }
  }

  private startHealthTimeout(): void {
    if (this.healthTimeoutId) {
      clearTimeout(this.healthTimeoutId);
    }

    const userId = this.getUserId();
    this.healthTimeoutId = setTimeout(() => {
      console.warn('[DaemonHealth] Health timeout reached - setting status to disconnected');
      setDaemonStatus(userId, 'disconnected');
      this.healthTimeoutId = null;
    }, this.HEALTH_TIMEOUT_MS);
  }

  private getUserId(): string {
    const token = getCoreStateSnapshot().snapshot.sessionToken;
    if (!token) {
      return '__pending__';
    }

    try {
      const parts = token.split('.');
      if (parts.length !== 3) {
        return '__pending__';
      }

      const payloadBase64 = parts[1].replace(/-/g, '+').replace(/_/g, '/');
      const payloadJson = atob(payloadBase64);
      const payload = JSON.parse(payloadJson) as {
        sub?: string;
        tgUserId?: string;
        userId?: string;
      };
      return payload.tgUserId || payload.userId || payload.sub || '__pending__';
    } catch {
      return '__pending__';
    }
  }
}

export const daemonHealthService = new DaemonHealthService();
