import type { FlowRunStatus as FlowRunStatusValue } from '../../services/api/flowsApi';

export const FLOW_RUN_STATUS_ACCENT: Record<FlowRunStatusValue, string> = {
  running:
    'border-ocean-200 bg-ocean-50 text-ocean-700 dark:border-ocean-500/30 dark:bg-ocean-500/10 dark:text-ocean-300',
  completed:
    'border-sage-200 bg-sage-50 text-sage-700 dark:border-sage-500/30 dark:bg-sage-500/10 dark:text-sage-300',
  completed_with_warnings:
    'border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-300',
  pending_approval:
    'border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-300',
  failed:
    'border-coral-200 bg-coral-50 text-coral-700 dark:border-coral-500/30 dark:bg-coral-500/10 dark:text-coral-300',
  cancelled: 'border-line bg-surface-muted text-content-secondary',
  interrupted:
    'border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-300',
};

export const FLOW_RUN_STATUS_DOT: Record<FlowRunStatusValue, string> = {
  running: 'bg-ocean-500 animate-pulse',
  completed: 'bg-sage-500',
  completed_with_warnings: 'bg-amber-500',
  pending_approval: 'bg-amber-500 animate-pulse',
  failed: 'bg-coral-500',
  cancelled: 'bg-surface-strong',
  interrupted: 'bg-amber-500',
};

export const FLOW_RUN_STATUS_KEY: Record<FlowRunStatusValue, string> = {
  running: 'flowRuns.status.running',
  completed: 'flowRuns.status.completed',
  completed_with_warnings: 'flowRuns.status.completed_with_warnings',
  pending_approval: 'flowRuns.status.pending_approval',
  failed: 'flowRuns.status.failed',
  cancelled: 'flowRuns.status.cancelled',
  interrupted: 'flowRuns.status.interrupted',
};

export type FlowRunStatusPresentation = 'badge' | 'dot';

export interface FlowRunStatusProps {
  status: FlowRunStatusValue;
  label: string;
  presentation?: FlowRunStatusPresentation;
  className?: string;
  testId?: string;
}

export function FlowRunStatus({
  status,
  label,
  presentation = 'badge',
  className = '',
  testId,
}: FlowRunStatusProps) {
  if (presentation === 'dot') {
    return (
      <span
        data-testid={testId}
        className={`h-2 w-2 shrink-0 rounded-full ${FLOW_RUN_STATUS_DOT[status]} ${className}`.trim()}
        aria-hidden
      />
    );
  }

  return (
    <span
      data-testid={testId}
      className={`inline-flex items-center rounded-full border px-2 py-0.5 font-medium ${FLOW_RUN_STATUS_ACCENT[status]} ${className}`.trim()}>
      {label}
    </span>
  );
}
