import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { AssistantUiToolCallCard, formatElapsed } from './AssistantUiToolCall';
import { ChatToolGroup } from './ChatToolParts';

const SEARCH_TEXT = [
  'Search results for: rust async traits (via Exa)',
  '1. Async fn in traits',
  '   https://blog.rust-lang.org/async-fn',
  '   Stable in 1.75.',
  '2. Bad link',
  '   javascript:alert(1)',
].join('\n');

describe('AssistantUiToolCallCard', () => {
  it('renders a web search through the web-search element, visible without expanding', () => {
    render(
      <AssistantUiToolCallCard
        toolName="web_search_tool"
        args={{ query: 'rust async traits' }}
        result={SEARCH_TEXT}
        status="success"
        elapsedMs={1840}
      />
    );
    const results = screen.getByTestId('web-search-results');
    expect(results).toHaveTextContent('rust async traits');
    expect(results).toHaveTextContent('Found 1 result · via Exa');
    const hits = within(results).getAllByTestId('web-search-hit');
    expect(hits).toHaveLength(1);
    expect(hits[0]).toHaveAttribute('href', 'https://blog.rust-lang.org/async-fn');
    // The javascript: hit never becomes a link or a row.
    expect(results).not.toHaveTextContent('Bad link');
    expect(screen.getByTestId('tool-call-elapsed')).toHaveTextContent('1.8s');
  });

  it('prefers the structured search payload the core attaches', () => {
    render(
      <AssistantUiToolCallCard
        toolName="web_search_tool"
        args={{ query: 'q' }}
        result="unparseable"
        structured={{
          kind: 'web_search',
          query: 'q',
          provider: 'Parallel',
          results: [{ title: 'From payload', url: 'https://a.dev/x' }],
        }}
        status="success"
      />
    );
    expect(screen.getByTestId('web-search-results')).toHaveTextContent('From payload');
    expect(screen.getByTestId('web-search-results')).toHaveTextContent('via Parallel');
  });

  it('swaps the label tense as the call settles', () => {
    const { rerender } = render(
      <AssistantUiToolCallCard toolName="file_read" args={{ path: 'a.ts' }} status="running" />
    );
    const card = screen.getByTestId('assistant-ui-tool-call');
    expect(card).toHaveAttribute('data-outcome', 'running');
    expect(within(card).getByRole('button', { name: /Reading file/ })).toBeInTheDocument();
    rerender(
      <AssistantUiToolCallCard
        toolName="file_read"
        args={{ path: 'a.ts' }}
        status="success"
        result="x"
      />
    );
    expect(card).toHaveAttribute('data-outcome', 'success');
    expect(within(card).getByRole('button', { name: /Read file/ })).toBeInTheDocument();
  });

  it('spells out states that need attention and keeps the rest for screen readers', () => {
    render(
      <>
        <AssistantUiToolCallCard toolName="grep" status="error" result="bad regex" />
        <AssistantUiToolCallCard toolName="shell" status="success" result="ok" />
      </>
    );
    const [failed, done] = screen.getAllByTestId('tool-call-status');
    expect(failed).toHaveTextContent('failed');
    expect(failed).not.toHaveClass('sr-only');
    expect(done).toHaveTextContent('done');
    expect(done).toHaveClass('sr-only');
  });

  it('expands an edit into the code-diff element and a read into a code block', async () => {
    render(
      <>
        <AssistantUiToolCallCard
          toolName="edit"
          args={{ path: 'a.ts', old_string: 'old', new_string: 'new' }}
          status="success"
          result="ok"
        />
        <AssistantUiToolCallCard
          toolName="file_read"
          args={{ path: 'src/b.rs' }}
          status="success"
          result="fn main() {}"
        />
      </>
    );
    const [edit, read] = screen.getAllByTestId('assistant-ui-tool-call');
    await userEvent.click(within(edit).getByRole('button', { name: /Edited file/ }));
    const diff = screen.getByTestId('tool-body-file-diff');
    expect(diff).toHaveTextContent('+1');
    expect(diff).toHaveTextContent('old');
    await userEvent.click(within(read).getByRole('button', { name: /Read file/ }));
    expect(screen.getByTestId('tool-body-file')).toHaveTextContent('fn main() {}');
  });

  it('names a connected-app action by its app, never the slug', () => {
    render(<AssistantUiToolCallCard toolName="GMAIL_SEND_EMAIL" status="success" result="ok" />);
    const card = screen.getByTestId('assistant-ui-tool-call');
    expect(card).toHaveTextContent('Used Gmail');
    expect(card).toHaveTextContent('Send email');
    expect(card).not.toHaveTextContent('GMAIL_SEND_EMAIL');
  });
});

describe('ChatToolGroup', () => {
  it('renders a lone call without a timeline header', () => {
    render(
      <ChatToolGroup group={{ type: 'group-tool', status: { type: 'complete' }, indices: [0] }}>
        <span>only step</span>
      </ChatToolGroup>
    );
    expect(screen.getByText('only step')).toBeVisible();
    expect(screen.queryByTestId('tool-timeline')).toBeNull();
  });

  it('wraps several calls in the assistant-ui tool timeline', () => {
    render(
      <ChatToolGroup group={{ type: 'group-tool', status: { type: 'running' }, indices: [0, 1] }}>
        <span>step one</span>
        <span>step two</span>
      </ChatToolGroup>
    );
    const timeline = screen.getByTestId('tool-timeline');
    expect(timeline).toHaveAttribute('data-slot', 'tool-timeline');
    expect(screen.getByText('step two')).toBeVisible();
  });
});

describe('formatElapsed', () => {
  it('formats milliseconds, seconds and minutes', () => {
    expect(formatElapsed(850)).toBe('850ms');
    expect(formatElapsed(1840)).toBe('1.8s');
    expect(formatElapsed(75_000)).toBe('1m 15s');
    expect(formatElapsed(59_999)).toBe('1m 0s');
    expect(formatElapsed(-1)).toBe('0ms');
  });
});
