/**
 * MedullaDemoTasks — the scale showcase for the "Tasks" tab shown to users
 * without Medulla access. A fake four-column orchestrator board populated with
 * ~40 illustrative tasks, behind a preview banner.
 */
import { useMemo } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import DemoScaleBanner from './DemoScaleBanner';
import { buildDemoTasks, type DemoTaskStatus } from './medullaDemoData';

const COLUMNS: Array<{ status: DemoTaskStatus; labelKey: string; dot: string }> = [
  { status: 'pending', labelKey: 'orchPage.tasks.colPending', dot: 'bg-content-faint' },
  { status: 'active', labelKey: 'orchPage.tasks.colActive', dot: 'bg-primary-500' },
  { status: 'blocked', labelKey: 'orchPage.tasks.colBlocked', dot: 'bg-coral-500' },
  { status: 'completed', labelKey: 'orchPage.tasks.colCompleted', dot: 'bg-sage-500' },
];

export default function MedullaDemoTasks() {
  const { t } = useT();
  const tasks = useMemo(() => buildDemoTasks(), []);

  return (
    <div
      className="mx-auto h-full w-full max-w-5xl overflow-y-auto p-4"
      data-testid="orch-demo-tasks">
      <div className="animate-fade-up space-y-4">
        <DemoScaleBanner />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {COLUMNS.map(col => {
            const colTasks = tasks.filter(task => task.status === col.status);
            return (
              <div
                key={col.status}
                className="rounded-2xl border border-line bg-surface p-3 shadow-soft"
                data-testid={`orch-demo-col-${col.status}`}>
                <div className="mb-2 flex items-center gap-2 px-1">
                  <span className={`h-2 w-2 rounded-full ${col.dot}`} aria-hidden="true" />
                  <h3 className="text-sm font-semibold text-content">{t(col.labelKey)}</h3>
                  <span className="ml-auto text-xs font-medium text-content-faint">
                    {colTasks.length}
                  </span>
                </div>
                <div className="space-y-2">
                  {colTasks.map(task => (
                    <div
                      key={task.id}
                      className="rounded-xl border border-line bg-surface-subtle px-3 py-2">
                      <p className="text-xs font-medium leading-snug text-content">
                        {t(task.titleKey)}
                      </p>
                      <p className="mt-1 font-mono text-[10px] text-content-faint">{task.agent}</p>
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
