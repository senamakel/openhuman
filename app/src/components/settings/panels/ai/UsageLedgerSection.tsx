/*
 * Usage ledger + budget-math section — the right column of
 * BackgroundLoopControls. Pure presentational component: all figures are
 * precomputed by the parent hook.
 */
import type { CreditTransaction, TeamUsage } from '../../../../services/api/creditsApi';
import Button from '../../../ui/Button';
import {
  formatCount,
  formatDateTime,
  formatUsd,
  FormulaRow,
  MetricTile,
} from './backgroundLoopPrimitives';

export const UsageLedgerSection = ({
  t,
  loading,
  onRefresh,
  usage,
  spendRows,
  spendAvgRowUsd,
  spendSampleHours,
  spendPerHour,
  rowsPerHour,
  actionSummary,
  hourSummary,
  latestSpend,
  formatSpendAmount,
  backgroundApiReadsPerWeek,
  backgroundWakeupsPerWeek,
  calendarPlannerCallsPerWeek,
  composioConnectionScansPerWeek,
  memoryPollsPerWeek,
  estimatedRowsLeft,
  estimatedRowsPerBudget,
  projectedExhaustAt,
  projectedHoursLeft,
  scheduledCallsPerRemainingDollar,
  activeConnectionsCount,
}: {
  t: (key: string, fallback?: string) => string;
  loading: boolean;
  onRefresh: () => void;
  usage: TeamUsage | null;
  spendRows: CreditTransaction[];
  spendAvgRowUsd: number;
  spendSampleHours: number;
  spendPerHour: number;
  rowsPerHour: number;
  actionSummary: Array<[string, number, number]>;
  hourSummary: Array<[string, number]>;
  latestSpend: CreditTransaction | null;
  formatSpendAmount: (tx: CreditTransaction) => number;
  backgroundApiReadsPerWeek: number;
  backgroundWakeupsPerWeek: number;
  calendarPlannerCallsPerWeek: number;
  composioConnectionScansPerWeek: number;
  memoryPollsPerWeek: number;
  estimatedRowsLeft: number | null;
  estimatedRowsPerBudget: number | null;
  projectedExhaustAt: string;
  projectedHoursLeft: number | null;
  scheduledCallsPerRemainingDollar: number | null;
  activeConnectionsCount: number;
}) => (
  <div className="rounded-lg border border-line bg-surface p-3">
    <div className="flex items-center justify-between gap-3">
      <div>
        <div className="text-sm font-semibold text-content">
          {t('settings.ai.recentUsageLedger')}
        </div>
        <div className="text-xs text-content-muted">{t('settings.ai.recentUsageLedgerDesc')}</div>
      </div>
      <Button type="button" variant="secondary" size="xs" onClick={onRefresh} disabled={loading}>
        {t('common.reload')}
      </Button>
    </div>

    <div className="mt-3 grid grid-cols-2 gap-2 md:grid-cols-3">
      <MetricTile
        label={t('settings.ai.weekBudget')}
        value={usage ? formatUsd(usage.cycleBudgetUsd) : 'n/a'}
        detail={`resets ${formatDateTime(usage?.cycleEndsAt)}`}
      />
      <MetricTile
        label={t('settings.ai.cycleRemaining')}
        value={usage ? formatUsd(usage.remainingUsd) : 'n/a'}
        detail={usage ? `${formatUsd(usage.cycleSpentUsd)} used` : undefined}
      />
      <MetricTile
        label={t('settings.ai.cycleTotalSpend')}
        value={usage ? formatUsd(usage.insights.totals.totalUsd) : 'n/a'}
        detail={
          usage
            ? `inference ${formatUsd(usage.insights.totals.inferenceUsd)} + integrations ${formatUsd(usage.insights.totals.integrationsUsd)}`
            : undefined
        }
      />
      <MetricTile
        label={t('settings.ai.avgSpendRow')}
        value={spendAvgRowUsd > 0 ? formatUsd(spendAvgRowUsd) : 'n/a'}
        detail={`${spendRows.length} recent spend rows`}
      />
      <MetricTile
        label={t('settings.ai.backgroundApiReads')}
        value={`${formatCount(backgroundApiReadsPerWeek)}/week`}
        detail={`${formatCount(calendarPlannerCallsPerWeek)} planner + ${formatCount(composioConnectionScansPerWeek)} sync`}
      />
      <MetricTile
        label={t('settings.ai.backgroundWakeups')}
        value={`${formatCount(backgroundWakeupsPerWeek)}/week`}
        detail={`${formatCount(memoryPollsPerWeek)} memory polls`}
      />
    </div>

    <div className="mt-3 rounded-lg border border-line bg-surface-muted p-3">
      <div className="text-[10px] font-semibold uppercase tracking-wide text-content-faint">
        {t('settings.ai.budgetMath')}
      </div>
      <div className="mt-2 grid gap-2">
        <FormulaRow
          label={t('settings.ai.rowsLeft')}
          value={estimatedRowsLeft !== null ? formatCount(estimatedRowsLeft) : 'n/a'}
          detail={
            estimatedRowsLeft !== null
              ? `remaining / avg row = ${formatUsd(usage?.remainingUsd ?? 0)} / ${formatUsd(spendAvgRowUsd)}`
              : 'Need recent spend rows to estimate.'
          }
        />
        <FormulaRow
          label={t('settings.ai.rowsPerFullWeekBudget')}
          value={estimatedRowsPerBudget !== null ? formatCount(estimatedRowsPerBudget) : 'n/a'}
          detail={
            estimatedRowsPerBudget !== null
              ? `cycle budget / avg row = ${formatUsd(usage?.cycleBudgetUsd ?? 0)} / ${formatUsd(spendAvgRowUsd)}`
              : 'Need recent spend rows to estimate.'
          }
        />
        <FormulaRow
          label={t('settings.ai.sampleBurnRate')}
          value={spendPerHour > 0 ? `${formatUsd(spendPerHour)}/hr` : 'n/a'}
          detail={
            spendSampleHours > 0
              ? `${formatCount(rowsPerHour)} rows/hr across ${spendSampleHours.toFixed(1)}h sample`
              : 'Need timestamps from at least two spend rows.'
          }
        />
        <FormulaRow
          label={t('settings.ai.projectedEmpty')}
          value={projectedExhaustAt}
          detail={
            projectedHoursLeft !== null
              ? `${projectedHoursLeft.toFixed(1)}h after latest spend at recent burn rate`
              : 'No projection without recent hourly spend.'
          }
        />
        <FormulaRow
          label={t('settings.ai.apiReadsPerDollarRemaining')}
          value={
            scheduledCallsPerRemainingDollar !== null
              ? `${formatCount(scheduledCallsPerRemainingDollar)} reads/$`
              : 'n/a'
          }
          detail={
            usage
              ? `background API reads/week / remaining = ${formatCount(backgroundApiReadsPerWeek)} / ${formatUsd(usage.remainingUsd)}`
              : 'Need usage response to estimate.'
          }
        />
      </div>
    </div>

    <div className="mt-3 rounded-lg border border-line bg-surface-muted p-3">
      <div className="text-[10px] font-semibold uppercase tracking-wide text-content-faint">
        {t('settings.ai.loopCallBudget')}
      </div>
      <div className="mt-2 grid gap-2">
        <FormulaRow
          label={t('settings.ai.composioSyncScans')}
          value={`${formatCount(composioConnectionScansPerWeek)}/week`}
          detail={`${activeConnectionsCount} active integration connection(s) scanned every 20 min`}
        />
        <FormulaRow
          label={t('settings.ai.totalBackgroundApiReadBudget')}
          value={`${formatCount(backgroundApiReadsPerWeek)}/week`}
          detail={`calendar planner reads + periodic integration scans; excludes user-initiated chat tools`}
        />
        <FormulaRow
          label={t('settings.ai.memoryWorkerPolls')}
          value={`${formatCount(memoryPollsPerWeek)}/week max`}
          detail={`4 workers * 5s poll; LLM calls only for queued jobs`}
        />
      </div>
    </div>

    {latestSpend && (
      <div className="mt-3 rounded-md border border-line bg-surface-muted px-3 py-2 text-xs text-content-secondary">
        {t('settings.ai.latestSpend')
          .replace('{amount}', formatUsd(formatSpendAmount(latestSpend)))
          .replace('{time}', new Date(latestSpend.createdAt).toLocaleString())
          .replace('{action}', latestSpend.action)}
      </div>
    )}

    <div className="mt-3 space-y-3">
      <div>
        <div className="text-[10px] font-semibold uppercase tracking-wide text-content-faint">
          {t('settings.ai.topActions')}
        </div>
        <div className="mt-1 space-y-1">
          {actionSummary.length > 0 ? (
            actionSummary.map(([action, count, total]) => (
              <div
                key={action}
                className="flex items-center justify-between gap-2 text-xs text-content-secondary">
                <span className="truncate font-mono">{action}</span>
                <span className="shrink-0 text-content-muted">
                  {count} / {formatUsd(total)}
                </span>
              </div>
            ))
          ) : (
            <div className="text-xs text-content-muted">{t('settings.ai.noSpendRows')}</div>
          )}
        </div>
      </div>

      <div>
        <div className="text-[10px] font-semibold uppercase tracking-wide text-content-faint">
          {t('settings.ai.topHours')}
        </div>
        <div className="mt-1 space-y-1">
          {hourSummary.length > 0 ? (
            hourSummary.map(([hour, total]) => (
              <div
                key={hour}
                className="flex items-center justify-between gap-2 text-xs text-content-secondary">
                <span>{hour}</span>
                <span className="font-mono text-content-muted">{formatUsd(total)}</span>
              </div>
            ))
          ) : (
            <div className="text-xs text-content-muted">{t('settings.ai.noHourlySpend')}</div>
          )}
        </div>
      </div>
    </div>
  </div>
);

export default UsageLedgerSection;
