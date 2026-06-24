import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, test, vi } from 'vitest';

import { AgentMessageText, BubbleMarkdown, TableCellMarkdown } from './AgentMessageBubble';

const mocks = vi.hoisted(() => ({ openUrl: vi.fn(), openWorkspacePath: vi.fn() }));

vi.mock('../../../utils/openUrl', () => ({ openUrl: mocks.openUrl }));

vi.mock('../../../utils/tauriCommands/workspacePaths', () => ({
  openWorkspacePath: mocks.openWorkspacePath,
}));

describe('AgentMessageBubble markdown links', () => {
  beforeEach(() => {
    mocks.openUrl.mockReset();
    mocks.openUrl.mockResolvedValue(undefined);
    mocks.openWorkspacePath.mockReset();
    mocks.openWorkspacePath.mockResolvedValue(undefined);
  });

  test('opens allowed external links through the OS URL handler', async () => {
    render(<BubbleMarkdown content="[docs](https://example.com/docs)" />);

    await userEvent.click(screen.getByRole('link', { name: 'docs' }));

    await waitFor(() => expect(mocks.openUrl).toHaveBeenCalledWith('https://example.com/docs'));
    expect(mocks.openWorkspacePath).not.toHaveBeenCalled();
  });

  test('opens workspace links through the Tauri workspace path command', async () => {
    render(<BubbleMarkdown content="[summary](workspace:memory_tree/content/summary.md)" />);

    await userEvent.click(screen.getByRole('link', { name: 'summary' }));

    await waitFor(() =>
      expect(mocks.openWorkspacePath).toHaveBeenCalledWith('memory_tree/content/summary.md')
    );
    expect(mocks.openUrl).not.toHaveBeenCalled();
  });

  test('logs workspace link open failures for diagnostics', async () => {
    const error = new Error('missing file');
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mocks.openWorkspacePath.mockRejectedValueOnce(error);

    try {
      render(<BubbleMarkdown content="[summary](workspace:memory_tree/content/missing.md)" />);

      await userEvent.click(screen.getByRole('link', { name: 'summary' }));

      await waitFor(() =>
        expect(consoleError).toHaveBeenCalledWith('workspace open failed:', error)
      );
    } finally {
      consoleError.mockRestore();
    }
  });

  test('uses the same workspace link handling inside table cells', async () => {
    render(<TableCellMarkdown content="[note](openhuman-workspace:/docs/note.md)" />);

    await userEvent.click(screen.getByRole('link', { name: 'note' }));

    await waitFor(() => expect(mocks.openWorkspacePath).toHaveBeenCalledWith('docs/note.md'));
    expect(mocks.openUrl).not.toHaveBeenCalled();
  });

  test('does not open raw file links from markdown', async () => {
    render(<BubbleMarkdown content="[secret](file:///etc/passwd)" />);

    await userEvent.click(screen.getByText('secret'));

    expect(mocks.openUrl).not.toHaveBeenCalled();
    expect(mocks.openWorkspacePath).not.toHaveBeenCalled();
  });
});

describe('BubbleMarkdown math rendering', () => {
  test('renders GFM markdown tables when they appear inside regular bubble markdown', () => {
    render(
      <BubbleMarkdown
        content={
          'Release notes:\n\n| PR | What it does |\n| --- | --- |\n| #3784 | Render tables |'
        }
      />
    );

    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'PR' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: '#3784' })).toBeInTheDocument();
  });

  test('renders \\[ ... \\] block math (raw delimiters consumed, math visible)', () => {
    const { container } = render(<BubbleMarkdown content={'\\[ x^2 + y^2 = z^2 \\]'} />);
    const text = container.textContent ?? '';
    expect(text).not.toContain('\\[');
    expect(text).not.toContain('\\]');
    expect(text).toContain('x');
    expect(text).toContain('y');
    expect(text).toContain('z');
  });

  test('renders inline \\( ... \\) math (raw delimiters consumed, math visible)', () => {
    const { container } = render(<BubbleMarkdown content={'value \\(a+b\\) here'} />);
    const text = container.textContent ?? '';
    expect(text).not.toContain('\\(');
    expect(text).not.toContain('\\)');
    expect(text).toContain('value');
    expect(text).toContain('here');
    expect(text).toContain('a');
    expect(text).toContain('b');
  });

  test('renders bare bracket vmatrix block (math rendered, not raw text)', () => {
    const { container } = render(
      <BubbleMarkdown content={'[ \\begin{vmatrix} 1 & 2 \\\\ 3 & 4 \\end{vmatrix} = -2 ]'} />
    );
    const text = container.textContent ?? '';
    // KaTeX renders visible glyphs (∣ for vmatrix bars) — confirm rendering happened.
    expect(text).toContain('∣');
    expect(text).toContain('1');
    expect(text).toContain('4');
  });

  test('does NOT treat currency mentions as math', () => {
    const { container } = render(<BubbleMarkdown content={'total is $10 versus $20'} />);
    expect(container.textContent).toContain('$10');
    expect(container.textContent).toContain('$20');
  });
});

describe('AgentMessageText', () => {
  test('renders openhuman link pills without assistant bubble chrome', () => {
    render(
      <AgentMessageText
        content={'<openhuman-link path="settings/appearance">Appearance</openhuman-link>'}
      />
    );

    expect(screen.getByTestId('agent-message-text')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Appearance/ })).toBeInTheDocument();
  });

  test('uses the dedicated table renderer in plain text mode', () => {
    render(<AgentMessageText content={'| Name | Value |\n| --- | --- |\n| OpenHuman | 42 |'} />);

    expect(screen.getByTestId('agent-message-text')).toBeInTheDocument();
    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: 'Name' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'OpenHuman' })).toBeInTheDocument();
  });
});

describe('BubbleMarkdown line breaks', () => {
  // Regression: agent plans/steps separated by SINGLE newlines used to collapse
  // into one run-on paragraph (CommonMark soft break → space). `remark-breaks`
  // now renders each as its own line. Repro is the real "cooking recipes plan"
  // thread that rendered as a wall of text.
  test('preserves single newlines as hard breaks', () => {
    const plan =
      '**1. Project Scaffold** — Vite + React + TS init\n' +
      '**2. Data Model & Mock Data** — TypeScript types\n' +
      '**3. Home Page** — Recipe card grid';
    const { container } = render(<BubbleMarkdown content={plan} />);

    // Each item still reads as a distinct line — a <br> separates them rather
    // than the items running together on one line.
    expect(container.querySelectorAll('br').length).toBeGreaterThanOrEqual(2);
    const text = container.textContent ?? '';
    expect(text).toContain('1. Project Scaffold');
    expect(text).toContain('2. Data Model & Mock Data');
    expect(text).toContain('3. Home Page');
  });

  test('blank-line-separated paragraphs still render as separate blocks', () => {
    const { container } = render(
      <BubbleMarkdown content={'First paragraph.\n\nSecond paragraph.'} />
    );
    expect(container.querySelectorAll('p').length).toBeGreaterThanOrEqual(2);
  });
});
