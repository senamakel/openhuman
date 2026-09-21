import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ProcessingTranscriptItem } from '../../../../store/chatRuntimeSlice';
import { AgentInsightsSlot } from './AgentInsightsSlot';

vi.mock('../../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (key: string, fallback?: string) => fallback ?? key, locale: 'en' }),
}));

const thinking: ProcessingTranscriptItem[] = [
  { kind: 'thinking', round: 1, seq: 0, text: 'Planning a Kashmir itinerary.' },
];

function renderSlot(props: Partial<Parameters<typeof AgentInsightsSlot>[0]> = {}) {
  return render(
    <AgentInsightsSlot
      entries={[]}
      transcript={thinking}
      turnActive={false}
      timelineBeforeLatestAnswer={false}
      hideAgentInsights={false}
      onViewDetails={() => {}}
      onViewWholeRun={() => {}}
      {...props}
    />
  );
}

describe('AgentInsightsSlot before the first tool call', () => {
  it('streams the live thought inline while the turn is in flight', () => {
    renderSlot({ turnActive: true });
    expect(screen.getByTestId('agent-insights-live')).toBeTruthy();
    expect(screen.getByTestId('processing-thinking-live').textContent).toContain(
      'Planning a Kashmir itinerary.'
    );
    // The whole-run opener stays reachable.
    expect(screen.getByTestId('view-process-source')).toBeTruthy();
  });

  it('falls back to the static opener once the turn has settled', () => {
    renderSlot({ turnActive: false });
    expect(screen.queryByTestId('agent-insights-live')).toBeNull();
    expect(screen.queryByTestId('processing-thinking-live')).toBeNull();
    expect(screen.getByTestId('view-process-source')).toBeTruthy();
  });

  it('respects "hide agent insights" and shows only the processing link', () => {
    renderSlot({ turnActive: true, hideAgentInsights: true });
    expect(screen.queryByTestId('agent-insights-live')).toBeNull();
    expect(screen.getByTestId('agent-processing-link')).toBeTruthy();
  });

  it('renders nothing with neither steps nor transcript', () => {
    const { container } = renderSlot({ turnActive: true, transcript: [] });
    expect(container.innerHTML).toBe('');
  });
});
