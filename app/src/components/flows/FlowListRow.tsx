/**
 * FlowListRow — one saved-flow row on the Workflows list page (issue B5a).
 *
 * Mirrors the row layout of `CoreJobList`
 * (`app/src/components/settings/panels/cron/CoreJobList.tsx`): name + status
 * badge header, a line of run metadata, then a row of `Button` actions. Swaps
 * the cron "pause/resume" text button for a `SettingsSwitch` toggle (the
 * canonical boolean control — see `components/settings/controls`) since
 * enable/disable here is a persistent setting, not a one-off action.
 *
 * "View runs" (issue B5a.1) opens `FlowRunsDrawer` (mounted by `FlowsPage`)
 * for this flow's run history — re-added now that B3b's run inspector has
 * landed and the drawer has somewhere to send the user.
 *
 * The flow name (issue B5b.1) is itself the "View" affordance for the new
 * read-only Workflow Canvas: it's rendered as a button that calls `onView`,
 * which `FlowsPage` wires to `navigate('/flows/' + flow.id)`. Kept as the
 * name itself (not a separate icon button) since it's the row's most
 * prominent, discoverable element and "view this flow's graph" is the most
 * natural action to hang off it — "View runs" and "Run" stay distinct
 * actions in the button row below.
 */
import { useT } from '../../lib/i18n/I18nContext';
import type { Flow } from '../../services/api/flowsApi';
import Button from '../ui/Button';
import FlowRowMenu from './FlowRowMenu';

function PlayIcon() {
  return (
    <svg className="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M7 5l12 7-12 7V5z" />
    </svg>
  );
}

function PowerIcon() {
  // On/off — enabled vs. paused (distinct from Run's play triangle).
  return (
    <svg
      className="h-4 w-4"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true">
      <path d="M12 2v10" />
      <path d="M18.36 6.64a9 9 0 11-12.73 0" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg
      className="h-4 w-4"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true">
      <path d="M3 6h18M8 6V4a1 1 0 011-1h6a1 1 0 011 1v2m2 0v14a1 1 0 01-1 1H7a1 1 0 01-1-1V6" />
      <path d="M10 11v6M14 11v6" />
    </svg>
  );
}

/** Which of this row's actions currently has a request in flight, if any. */
export type FlowListRowBusy = 'toggle' | 'run' | null;

/** Matches `useT()`'s `t` signature (`I18nContextValue['t']` isn't exported). */
type TFn = (key: string, fallback?: string) => string;

export interface FlowListRowProps {
  flow: Flow;
  onToggle: (flow: Flow) => void;
  onRun: (flow: Flow) => void;
  onViewRuns: (flow: Flow) => void;
  /** Opens the read-only Workflow Canvas for this flow (issue B5b.1). */
  onView: (flow: Flow) => void;
  /** Downloads this flow's `WorkflowGraph` as a JSON file (Phase 4d export). */
  onExport: (flow: Flow) => void;
  /** Creates a disabled copy of this flow (`flows_duplicate`). */
  onDuplicate: (flow: Flow) => void;
  /** Permanently deletes this flow after a confirm (`flows_delete`). */
  onDelete: (flow: Flow) => void;
  busy?: FlowListRowBusy;
}

/**
 * Formats the "last run" line. `t()` doesn't interpolate, so counts are
 * spliced into the translated template in code (`{count}` placeholder) rather
 * than templated through raw string concatenation.
 */
function relativeTime(iso: string, t: TFn): string {
  const ms = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return t('flows.list.justNow');
  if (mins < 60) return t('flows.list.minutesAgo').replace('{count}', String(mins));
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return t('flows.list.hoursAgo').replace('{count}', String(hrs));
  const days = Math.floor(hrs / 24);
  return t('flows.list.daysAgo').replace('{count}', String(days));
}

/**
 * `last_status` is rendered as-is (capitalized) rather than mapped through
 * i18n — the same precedent `CoreJobList` follows for `job.last_status` —
 * since it's a raw engine-status word, not prose.
 */
function capitalize(value: string): string {
  return value.length > 0 ? value.charAt(0).toUpperCase() + value.slice(1) : value;
}

const FlowListRow = ({
  flow,
  onToggle,
  onRun,
  onViewRuns,
  onView,
  onExport,
  onDuplicate,
  onDelete,
  busy = null,
}: FlowListRowProps) => {
  const { t } = useT();
  const toggleBusy = busy === 'toggle';
  const runBusy = busy === 'run';

  const lastRunLabel =
    flow.last_run_at && flow.last_status
      ? `${capitalize(flow.last_status)} · ${relativeTime(flow.last_run_at, t)}`
      : t('flows.list.neverRun');

  return (
    <div
      data-testid={`flow-row-${flow.id}`}
      className="flex items-center gap-3 border-t border-line p-4 first:border-t-0">
      <div className="min-w-0 flex-1">
        <button
          type="button"
          data-testid={`flow-view-${flow.id}`}
          title={t('flows.list.view')}
          aria-label={`${t('flows.list.view')}: ${flow.name}`}
          onClick={() => onView(flow)}
          className="max-w-full truncate text-left text-sm font-semibold text-content hover:text-primary-600 hover:underline dark:hover:text-primary-400">
          {flow.name}
        </button>
        <div className="mt-0.5 text-[11px] text-content-faint">{lastRunLabel}</div>
      </div>

      {/* All controls sit together on the right: the toggle (enabled/paused —
          the switch alone communicates state), then Run, Delete, and an overflow
          menu with the secondary actions (view runs / export / duplicate). */}
      <div className="flex flex-shrink-0 items-center gap-1.5">
        <Button
          type="button"
          variant="tertiary"
          size="sm"
          iconOnly
          data-testid={`flow-toggle-${flow.id}`}
          aria-label={t('flows.list.toggleEnabled')}
          aria-pressed={flow.enabled}
          title={flow.enabled ? t('flows.list.enabled') : t('flows.list.paused')}
          disabled={toggleBusy}
          className={
            flow.enabled
              ? 'text-sage-600 dark:text-sage-300'
              : 'text-content-faint hover:text-content-secondary'
          }
          onClick={() => onToggle(flow)}>
          <PowerIcon />
        </Button>
        <Button
          type="button"
          variant="primary"
          size="sm"
          analyticsId="flows-list-run"
          iconOnly
          data-testid={`flow-run-${flow.id}`}
          aria-label={runBusy ? t('flows.list.running') : t('flows.list.runNow')}
          title={runBusy ? t('flows.list.running') : t('flows.list.runNow')}
          disabled={runBusy}
          onClick={() => onRun(flow)}>
          <PlayIcon />
        </Button>
        <Button
          type="button"
          variant="tertiary"
          tone="danger"
          size="sm"
          iconOnly
          data-testid={`flow-delete-${flow.id}`}
          aria-label={t('flows.list.delete')}
          title={t('flows.list.delete')}
          onClick={() => onDelete(flow)}>
          <TrashIcon />
        </Button>
        <FlowRowMenu
          rowId={flow.id}
          items={[
            {
              key: 'view-runs',
              label: t('flows.list.viewRuns'),
              onSelect: () => onViewRuns(flow),
              testId: `flow-view-runs-${flow.id}`,
            },
            {
              key: 'export',
              label: t('flows.list.export'),
              onSelect: () => onExport(flow),
              testId: `flow-export-${flow.id}`,
            },
            {
              key: 'duplicate',
              label: t('flows.list.duplicate'),
              onSelect: () => onDuplicate(flow),
              testId: `flow-duplicate-${flow.id}`,
            },
          ]}
        />
      </div>
    </div>
  );
};

export default FlowListRow;
