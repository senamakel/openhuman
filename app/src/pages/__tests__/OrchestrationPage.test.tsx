import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import OrchestrationPage from '../OrchestrationPage';

// The orchestration surface itself has heavy data-source deps; the page wrapper
// is a thin scaffold, so we stub the tab and assert the wrapper mounts it.
vi.mock('../../components/intelligence/TinyPlaceOrchestrationTab', async () => {
  const React = await import('react');
  return { default: () => React.createElement('div', { 'data-testid': 'orchestration-tab' }) };
});

vi.mock('../../components/layout/PanelPage', async () => {
  const React = await import('react');
  return {
    default: ({ children }: { children?: React.ReactNode }) =>
      React.createElement('div', { 'data-testid': 'panel-page' }, children),
  };
});

describe('OrchestrationPage', () => {
  it('renders the TinyPlace orchestration tab inside the panel scaffold', () => {
    render(<OrchestrationPage />);
    expect(screen.getByTestId('panel-page')).toBeInTheDocument();
    expect(screen.getByTestId('orchestration-tab')).toBeInTheDocument();
  });
});
