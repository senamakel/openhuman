/**
 * Subconscious engine commands — engine control and scratchpad.
 *
 * Reflection/thoughts RPCs have been removed — the subconscious now
 * maintains only a scratchpad (via agent tools) and run logs.
 */
import { callCoreRpc } from '../../services/coreRpcClient';
import { type CommandResponse, isTauri } from './common';

// ── Types ────────────────────────────────────────────────────────────────────

export interface SubconsciousStatus {
  enabled: boolean;
  mode: 'off' | 'simple' | 'aggressive' | 'event_driven';
  provider_available: boolean;
  provider_unavailable_reason: string | null;
  interval_minutes: number;
  last_tick_at: number | null;
  total_ticks: number;
  consecutive_failures: number;
}

export interface TickResult {
  tick_at: number;
  duration_ms: number;
  response_chars?: number;
}

/** Status of the event-driven subconscious trigger pipeline. */
export interface SubconsciousTriggersStatus {
  triggers_enabled: boolean;
  mode: string;
  max_promotions_per_hour: number;
  orchestrator_running: boolean;
  queue_depth: number | null;
  orchestrator_thread_id: string;
  user_thread_id: string;
}

// ── Status & Trigger ─────────────────────────────────────────────────────────

export async function subconsciousStatus(): Promise<CommandResponse<SubconsciousStatus>> {
  if (!isTauri()) throw new Error('Not running in Tauri');
  return await callCoreRpc<CommandResponse<SubconsciousStatus>>({
    method: 'openhuman.subconscious_status',
  });
}

export async function subconsciousTrigger(): Promise<CommandResponse<TickResult>> {
  if (!isTauri()) throw new Error('Not running in Tauri');
  return await callCoreRpc<CommandResponse<TickResult>>({
    method: 'openhuman.subconscious_trigger',
  });
}

// The trigger-pipeline status + toggle work over any core transport (Tauri
// invoke or cloud/tunnel HTTP), so they intentionally do NOT gate on
// `isTauri()` — `callCoreRpc` resolves the active transport itself.

export async function subconsciousTriggersStatus(): Promise<
  CommandResponse<SubconsciousTriggersStatus>
> {
  return await callCoreRpc<CommandResponse<SubconsciousTriggersStatus>>({
    method: 'openhuman.subconscious_triggers_status',
  });
}

/**
 * Enable or disable the event-driven trigger pipeline. Enabling also flips the
 * subconscious into `event_driven` mode so the orchestrator bootstraps; the
 * core restarts the heartbeat loop on this change.
 */
export async function setSubconsciousTriggersEnabled(
  enabled: boolean
): Promise<CommandResponse<{ settings: unknown }>> {
  const params: Record<string, unknown> = { triggers_enabled: enabled };
  if (enabled) params.subconscious_mode = 'event_driven';
  return await callCoreRpc<CommandResponse<{ settings: unknown }>>({
    method: 'openhuman.heartbeat_settings_set',
    params,
  });
}
