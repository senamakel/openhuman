/**
 * Rich bodies for a tool call, built from assistant-ui's elements
 * (`components/assistant-ui/elements/`): the web-search element for
 * searches, the terminal block for commands, the web preview for fetched
 * pages and the code diff for file edits.
 *
 * This file only adapts OpenHuman's tool data onto those elements; it adds no
 * styling of its own beyond width. Each adapter returns `null` when the call
 * left nothing to show, so the caller can fall back to the generic
 * Request / Result panel.
 */
import type { ReactNode } from 'react';

import { Source } from '../../../components/ai-elements';
import { CodeDiff, type DiffLine } from '../../../components/assistant-ui/elements/code-diff';
import { TerminalBlock } from '../../../components/assistant-ui/elements/terminal-block';
import { WebPreview } from '../../../components/assistant-ui/elements/web-preview';
import { WebSearch } from '../../../components/assistant-ui/elements/web-search';
import { BubbleMarkdown } from '../components/AgentMessageBubble';
import { parseWebSearchResult } from './parseWebSearchResult';
import { displayUrl, shortenPath, type ToolArgs } from './toolChips';
import { fillPlaceholders } from './toolPhrases';
import type { Translate } from './toolPresentation';

const FULL_WIDTH = 'max-w-none';
const MAX_LINES = 400;

function renderSearchLink({
  href,
  className,
  children,
}: {
  href: string;
  className: string;
  children: ReactNode;
}) {
  // `Source` is the app's one external-link anchor; the global link guard
  // routes it to the OS browser. `href` is an http(s) URL vetted by
  // `parseWebSearchResult`.
  return (
    <Source
      href={href}
      rel="noreferrer noopener"
      className={className}
      data-testid="web-search-hit">
      {children}
    </Source>
  );
}

/** A web search through assistant-ui's web-search element. */
export function WebSearchBody({
  args,
  result,
  structured,
  searching,
  t,
}: {
  args: ToolArgs;
  result: unknown;
  structured?: unknown;
  searching: boolean;
  t: Translate;
}): ReactNode {
  const parsed = searching ? undefined : parseWebSearchResult(result, structured);
  if (!searching && !parsed) return null;
  const argQuery =
    typeof args.query === 'string'
      ? args.query
      : typeof args.objective === 'string'
        ? args.objective
        : '';
  const hits = parsed?.results ?? [];
  const count =
    hits.length === 0
      ? t('conversations.tools.search.none', 'No results')
      : fillPlaceholders(
          hits.length === 1
            ? t('conversations.tools.search.found.one', 'Found {count} result')
            : t('conversations.tools.search.found.other', 'Found {count} results'),
          { count: String(hits.length) }
        );
  const statusLabel = parsed?.provider
    ? `${count} · ${fillPlaceholders(t('conversations.tools.search.via', 'via {provider}'), {
        provider: parsed.provider,
      })}`
    : count;
  return (
    <WebSearch
      data-testid="web-search-results"
      className={FULL_WIDTH}
      query={parsed?.query ?? argQuery}
      results={hits.map(hit => ({ title: hit.title, domain: hit.domain, url: hit.url }))}
      visibleResults={hits.length}
      searching={searching}
      cycle={0}
      searchingLabel={t('conversations.tools.search.searching', 'Searching')}
      statusLabel={statusLabel}
      renderLink={renderSearchLink}
    />
  );
}

function stringOf(value: unknown): string | undefined {
  if (typeof value === 'string') return value;
  if (value === undefined || value === null) return undefined;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function linesOf(text: string): string[] {
  const lines = text.replace(/\s+$/, '').split('\n');
  return lines.length > MAX_LINES ? [...lines.slice(0, MAX_LINES), '…'] : lines;
}

/** A command and its output through assistant-ui's terminal block. */
export function ShellBody({
  args,
  result,
  failed,
  t,
}: {
  args: ToolArgs;
  result: unknown;
  failed: boolean;
  t: Translate;
}): ReactNode {
  const command =
    (typeof args.command === 'string' && args.command) ||
    (typeof args.subcommand === 'string' && `npm ${args.subcommand}`) ||
    (typeof args.script_path === 'string' && args.script_path) ||
    (typeof args.inline_code === 'string' && args.inline_code) ||
    '';
  const output = stringOf(result) ?? '';
  if (!command && !output.trim()) return null;
  const lines = output.trim() ? linesOf(output) : [];
  return (
    <TerminalBlock
      data-testid="tool-body-shell"
      className={FULL_WIDTH}
      command={command ? `$ ${command}` : ''}
      lines={lines}
      visibleCount={lines.length}
      done
      failed={failed}
      exitLabel={
        failed ? t('conversations.tools.status.failed') : t('conversations.tools.status.done')
      }
    />
  );
}

/** `status=200 url=https://… content=markdown` header, then the page. */
function splitFetchOutput(text: string): { status?: string; url?: string; body: string } {
  const newline = text.indexOf('\n');
  const head = newline === -1 ? text : text.slice(0, newline);
  if (!/^status=\d{3}\b/.test(head)) return { body: text };
  return {
    status: head.match(/^status=(\d{3})/)?.[1],
    url: head.match(/\burl=(\S+)/)?.[1],
    body: newline === -1 ? '' : text.slice(newline + 1),
  };
}

function isHttpUrl(value: string): boolean {
  try {
    const { protocol } = new URL(value);
    return protocol === 'http:' || protocol === 'https:';
  } catch {
    return false;
  }
}

/** A fetched page through assistant-ui's web preview. */
export function FetchBody({
  args,
  result,
  t,
  onOpenExternal,
}: {
  args: ToolArgs;
  result: unknown;
  t: Translate;
  onOpenExternal?: (url: string) => void;
}): ReactNode {
  const text = typeof result === 'string' ? result : undefined;
  if (!text) return null;
  const { status, url, body } = splitFetchOutput(text);
  const source = url ?? (typeof args.url === 'string' ? args.url : undefined);
  const externalSource = source && isHttpUrl(source) ? source : undefined;
  const origin = [status, source ? displayUrl(source) : undefined].filter(Boolean).join(' · ');
  return (
    <WebPreview
      data-testid="tool-body-fetch"
      className={FULL_WIDTH}
      origin={origin}
      loading={false}
      onOpenExternal={externalSource && onOpenExternal ? () => onOpenExternal(externalSource) : undefined}
      openExternalLabel={t('conversations.tools.openInBrowser')}>
      <div className="max-h-64 overflow-auto px-3.5 py-2.5 text-xs">
        <BubbleMarkdown content={(body.trim() ? body : text).slice(0, 4000)} />
      </div>
    </WebPreview>
  );
}

/** A file edit, write or read through assistant-ui's code diff. */
export function FileBody({ args, result }: { args: ToolArgs; result: unknown }): ReactNode {
  const path =
    typeof args.path === 'string'
      ? args.path
      : typeof args.file_path === 'string'
        ? args.file_path
        : '';
  const filename = shortenPath(path);
  const oldText = typeof args.old_string === 'string' ? args.old_string : undefined;
  const newText = typeof args.new_string === 'string' ? args.new_string : undefined;
  if (oldText !== undefined || newText !== undefined) {
    const removed = oldText ? linesOf(oldText) : [];
    const added = newText ? linesOf(newText) : [];
    const lines: DiffLine[] = [
      ...removed.map(text => ({ kind: 'removed' as const, text })),
      ...added.map(text => ({ kind: 'added' as const, text })),
    ];
    return (
      <CodeDiff
        data-testid="tool-body-file-diff"
        className={FULL_WIDTH}
        filename={filename}
        additions={added.length}
        deletions={removed.length}
        lines={lines}
        cycle={0}
      />
    );
  }
  const written = typeof args.content === 'string' ? args.content : undefined;
  if (written !== undefined) {
    if (!written.trim()) return null;
    const lines = linesOf(written);
    return (
      <CodeDiff
        data-testid="tool-body-file"
        className={`${FULL_WIDTH} max-h-72 overflow-auto`}
        filename={filename}
        additions={lines.length}
        deletions={0}
        lines={lines.map(text => ({ kind: 'added' as const, text }))}
        cycle={0}
      />
    );
  }
  // A read changed nothing, so a diff header ("+0 −0") would mislead: show the
  // content as a fenced code block through the chat's markdown renderer.
  const content = typeof result === 'string' ? result : '';
  if (!content.trim()) return null;
  const language = /\.([a-z0-9]+)$/i.exec(path)?.[1]?.toLowerCase() ?? '';
  const fence = content.includes('```') ? '````' : '```';
  return (
    <div data-testid="tool-body-file" className="max-h-72 overflow-auto text-xs">
      <BubbleMarkdown content={`${fence}${language}\n${linesOf(content).join('\n')}\n${fence}`} />
    </div>
  );
}
