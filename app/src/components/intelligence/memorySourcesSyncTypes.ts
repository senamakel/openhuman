/**
 * Sync-progress types shared between `MemorySourcesRegistry` (which owns the
 * in-flight state) and `MemorySourceRow` (which renders it). Split out so
 * neither file needs to import the other for a type.
 */

/** Live progress for a sync in flight, driven by MemorySyncStageChanged events. */
export interface SyncProgress {
  stage: string;
  detail: string | null;
  percent: number | null;
}

/**
 * Terminal outcome of a sync run, shown on the row after the `completed` or
 * `failed` stage event arrives (#3295). Persists until the next sync starts,
 * so a no-op ("0 new items") or failed sync leaves visible confirmation
 * instead of the indicator silently vanishing.
 */
/**
 * Why a completed run stopped short, parsed from the remainder the core writes
 * after the item count. `more_pending`: the per-run cap or the day's budget
 * left more to read — click Sync again. `budget_spent`: the day's provider
 * request budget is gone, so nothing more arrives until tomorrow; with a zero
 * count that is the opposite of "Up to date", which is what the row used to
 * say.
 */
export type SyncNote = 'more_pending' | 'budget_spent';

export interface SyncResult {
  kind: 'success' | 'failed';
  /** New items ingested (success only); null when the count is unknown. */
  items: number | null;
  /** Human-readable failure reason (failed only). */
  reason: string | null;
  /** Why the run stopped short (success only); null when it did not. */
  note: SyncNote | null;
}

/**
 * Per-stage fallback percentages so the progress bar always advances even
 * when no numeric "N/M" ratio is present in the detail string (RC#4, #3295).
 */
export const STAGE_FALLBACK_PERCENT: Record<string, number> = {
  requested: 2,
  fetching: 5,
  stored: 15,
  queued: 25,
  ingesting: 40,
  completed: 100,
};

/**
 * The stages that describe one item inside a run rather than the run itself.
 * The core's sync-stage bridge emits them per stored document, and their
 * details name internal ids (`mem_src:…`, `queue_depth=…`).
 */
const ITEM_STAGES = new Set(['stored', 'queued', 'ingesting']);

/** Whether `stage` describes one item inside a run (see `ITEM_STAGES`). */
export function isItemStage(stage: string): boolean {
  return ITEM_STAGES.has(stage);
}

/**
 * The i18n key for each stage's plain-language label (openhuman#6257). The
 * rows used to print the raw stage name, so the reader path's per-item stage
 * surfaced as "Queued" beside "queued chunk extraction for mem_src:…" —
 * pipeline vocabulary with no queue anywhere to look at. Spelled out literally
 * so each key is visible to the i18n scanner.
 */
const STAGE_LABEL_KEYS: Record<string, string> = {
  requested: 'memorySources.stage.requested',
  running: 'memorySources.stage.running',
  fetching: 'memorySources.stage.fetching',
  stored: 'memorySources.stage.stored',
  queued: 'memorySources.stage.queued',
  ingesting: 'memorySources.stage.ingesting',
};

/** The label key for `stage`, with a generic "Syncing" for one it does not know. */
export function stageLabelKey(stage: string): string {
  return STAGE_LABEL_KEYS[stage] ?? 'memorySources.stage.unknown';
}

/**
 * The stages whose detail a person can read: the connector's `running` detail
 * ("pass 2 done, 400 item(s) so far"), a reader's `fetching` ratio, and the
 * request itself. Listed rather than inferred, so a per-item stage's detail
 * (`mem_src:…`, `queue_depth=…`) and a stage this app does not know yet stay
 * hidden.
 */
const DETAIL_STAGES = new Set(['requested', 'running', 'fetching']);

/** Whether a stage's detail is worth showing (see `DETAIL_STAGES`). */
export function showsStageDetail(stage: string): boolean {
  return DETAIL_STAGES.has(stage);
}

/**
 * Display labels keyed by source id, from the rows of
 * `memory_sources.status_list` that carry one — a core that predates the
 * `label` field sends none, and callers fall back to the id or the scope.
 */
export function sourceLabelsById(
  statuses: ReadonlyArray<{ source_id: string; label?: string }>
): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const status of statuses) {
    if (status.label) labels[status.source_id] = status.label;
  }
  return labels;
}

/**
 * The syncing rows that name a registry source, sorted (openhuman#6257).
 *
 * The store lights a row for any stage event it can key, and the core's bridge
 * keys its per-document `ingesting` stage for a document outside the registry
 * (a Composio message, a note) by that document's id. Such a row never ends and
 * names no source, so a screen that lists or counts runs keeps to the ids the
 * status list reports.
 */
export function registrySyncingIds(
  syncingIds: ReadonlySet<string>,
  sourceIds: ReadonlySet<string>
): string[] {
  return [...syncingIds].filter(id => sourceIds.has(id)).sort();
}
