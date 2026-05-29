/**
 * Tests for the Workflows page — list / empty / detail / create / delete /
 * error paths. The i18n translator is mocked to return the fallback string so
 * we can query by the human-readable copy.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';

import Workflows from './Workflows';

const hoisted = vi.hoisted(() => ({
  listWorkflows: vi.fn(),
  readWorkflow: vi.fn(),
  createWorkflow: vi.fn(),
  uninstallWorkflow: vi.fn(),
  resolvePhase: vi.fn(),
}));

vi.mock('../services/api/workflowsApi', () => ({
  workflowsApi: {
    listWorkflows: (...a: unknown[]) => hoisted.listWorkflows(...a),
    readWorkflow: (...a: unknown[]) => hoisted.readWorkflow(...a),
    createWorkflow: (...a: unknown[]) => hoisted.createWorkflow(...a),
    uninstallWorkflow: (...a: unknown[]) => hoisted.uninstallWorkflow(...a),
    resolvePhase: (...a: unknown[]) => hoisted.resolvePhase(...a),
  },
}));

// Identity translator that honours the fallback so assertions read naturally.
vi.mock('../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (_key: string, fallback?: string) => fallback ?? _key }),
}));

const SUMMARY = {
  id: 'bug-triage',
  name: 'bug-triage',
  description: 'Handle a bug',
  when_to_use: 'a user reports a bug',
  tags: ['support'],
  phases: ['on_pick_up_task', 'on_close_task'],
  scope: 'user' as const,
  location: '/x/WORKFLOW.md',
  warnings: [],
};

const DETAIL = {
  ...SUMMARY,
  tools: null,
  phases: {
    on_pick_up_task: {
      description: null,
      rules: ['Reproduce first'],
      scripts: ['git fetch'],
      tools: null,
      context: ['git'],
    },
  },
};

beforeEach(() => {
  Object.values(hoisted).forEach((fn) => fn.mockReset());
});

describe('Workflows page', () => {
  test('renders the empty state when there are no workflows', async () => {
    hoisted.listWorkflows.mockResolvedValue([]);
    render(<Workflows />);
    await waitFor(() => expect(hoisted.listWorkflows).toHaveBeenCalled());
    expect(await screen.findByText(/No workflows yet/i)).toBeInTheDocument();
  });

  test('surfaces a load error', async () => {
    hoisted.listWorkflows.mockRejectedValue(new Error('boom'));
    render(<Workflows />);
    expect(await screen.findByText('boom')).toBeInTheDocument();
  });

  test('lists workflows and expands phase detail on click', async () => {
    hoisted.listWorkflows.mockResolvedValue([SUMMARY]);
    hoisted.readWorkflow.mockResolvedValue(DETAIL);
    render(<Workflows />);

    const card = await screen.findByText('Handle a bug');
    fireEvent.click(card);

    await waitFor(() => expect(hoisted.readWorkflow).toHaveBeenCalledWith('bug-triage'));
    expect(await screen.findByText('Reproduce first')).toBeInTheDocument();
    expect(screen.getByText('git fetch')).toBeInTheDocument();
  });

  test('creates a workflow through the form', async () => {
    hoisted.listWorkflows.mockResolvedValue([]);
    hoisted.createWorkflow.mockResolvedValue(DETAIL);
    render(<Workflows />);
    await waitFor(() => expect(hoisted.listWorkflows).toHaveBeenCalled());

    fireEvent.click(screen.getByText('New workflow'));
    fireEvent.change(screen.getByPlaceholderText('e.g. Bug triage'), {
      target: { value: 'Bug triage' },
    });
    fireEvent.change(screen.getByPlaceholderText('What this workflow is for'), {
      target: { value: 'Handle a bug' },
    });
    // After typing both required fields, the list refresh after create returns the new one.
    hoisted.listWorkflows.mockResolvedValue([SUMMARY]);
    fireEvent.click(screen.getByText('Create'));

    await waitFor(() =>
      expect(hoisted.createWorkflow).toHaveBeenCalledWith({
        name: 'Bug triage',
        description: 'Handle a bug',
        when_to_use: undefined,
      }),
    );
    expect(await screen.findByText('Workflow created')).toBeInTheDocument();
  });

  test('deletes a user-scope workflow after confirmation', async () => {
    hoisted.listWorkflows.mockResolvedValue([SUMMARY]);
    hoisted.uninstallWorkflow.mockResolvedValue({
      name: 'bug-triage',
      removed_path: '/x',
      scope: 'user',
    });
    render(<Workflows />);

    // First Delete click arms the confirm; second confirms.
    const deleteBtn = await screen.findByLabelText('Delete workflow');
    fireEvent.click(deleteBtn);
    hoisted.listWorkflows.mockResolvedValue([]);
    fireEvent.click(screen.getByText('Delete'));

    await waitFor(() => expect(hoisted.uninstallWorkflow).toHaveBeenCalledWith('bug-triage'));
    expect(await screen.findByText('Workflow deleted')).toBeInTheDocument();
  });
});
