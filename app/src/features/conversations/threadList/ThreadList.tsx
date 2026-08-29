import type { RefObject } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import type { Thread } from '../../../types/thread';
import { isImeCompositionKeyEvent } from '../Conversations';

interface ThreadListProps {
  /** Threads visible after the sidebar's search/tab filtering. */
  threads: Thread[];
  selectedThreadId: string | null;
  onCreateThread: () => void;
  /** Select a thread (owns dispatch + message load + route sync). */
  onSelectThread: (threadId: string) => void;
  /** Stable, human-readable title for a thread id. */
  resolveTitle: (threadId: string) => string;
  onRequestDelete: (thread: Thread) => void;
  // Inline title rename — controlled by the parent so the edit state stays
  // co-located with the rest of the panel's thread state.
  editingThreadId: string | null;
  editTitleValue: string;
  editTitleInputRef: RefObject<HTMLInputElement | null>;
  onEditTitleValueChange: (value: string) => void;
  onStartEditTitle: (threadId: string) => void;
  onCommitTitle: (threadId: string) => void;
  onCancelEditTitle: () => void;
  onBlurTitle: (threadId: string) => void;
}

/**
 * The conversations left rail: a section header with the "new conversation"
 * affordance docked on the right, above the scrollable thread list with inline
 * rename + delete. Presentational, driven entirely by props so it can be reused
 * by the page and sidebar shells.
 */
export function ThreadList({
  threads,
  selectedThreadId,
  onCreateThread,
  onSelectThread,
  resolveTitle,
  onRequestDelete,
  editingThreadId,
  editTitleValue,
  editTitleInputRef,
  onEditTitleValueChange,
  onStartEditTitle,
  onCommitTitle,
  onCancelEditTitle,
  onBlurTitle,
}: ThreadListProps) {
  const { t } = useT();
  return (
    // Card background / rounded corners come from TwoPanelLayout's pane styling.
    <div className="h-full flex flex-col">
      {/* Section header: a muted group label with the "new" affordance docked on
          the right, replacing the old full-width centered button. Mirrors the
          grouped-nav idiom the settings sidebar already uses. */}
      <div className="flex shrink-0 items-center justify-between px-3 pb-1.5 pt-4">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-content-muted">
          {t('chat.conversationsHeading')}
        </span>
        <button
          type="button"
          data-testid="new-thread-button"
          data-analytics-id="chat-sidebar-new-thread"
          onClick={onCreateThread}
          title={t('chat.newThreadShortcut')}
          aria-label={t('chat.newConversation')}
          className="flex h-5 w-5 flex-none items-center justify-center rounded text-content-faint transition-colors hover:bg-surface/40 hover:text-content-secondary">
          <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
          </svg>
        </button>
      </div>
      {/* Rows are inset pills, so the scroll container carries the gutter, and
          it is px-3 — the shell sidebar's own (`SidebarGroup`/`SidebarHeader`).
          This list is projected into that column, so a narrower inset of its
          own put thread rows and app-nav rows on two different left edges. */}
      <div className="flex-1 overflow-y-auto px-3 pb-3">
        {threads.length === 0 ? (
          <p className="px-4 py-6 text-xs text-content-faint text-center">{t('chat.noThreads')}</p>
        ) : (
          threads.map(thread => (
            <div
              key={thread.id}
              data-testid={`thread-row-${thread.id}`}
              data-analytics-id="chat-sidebar-thread-row"
              role="button"
              tabIndex={0}
              onClick={() => onSelectThread(thread.id)}
              onKeyDown={e => {
                if (e.target !== e.currentTarget) return;
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelectThread(thread.id);
                }
              }}
              // A rounded pill per row, separated by spacing rather than
              // hairlines — six dividers in a short list read as a table, not a
              // list of destinations. Alpha fills so the row lifts identically
              // whether the list is projected into the (translucent) app sidebar
              // or rendered inside the opaque chat aside.
              // Fixed `h-8` matching SidebarNav's rows: the hover-revealed
              // actions are taller than the title's line box, so a padding-sized
              // row would grow 4px the moment the pointer entered it and the
              // whole list would shift under the cursor.
              className={`group mb-0.5 flex h-8 w-full cursor-pointer items-center rounded-md px-2.5 text-left transition-colors ${
                selectedThreadId === thread.id
                  ? 'bg-surface/70'
                  : 'hover:bg-surface/40 dark:hover:bg-surface/60'
              }`}>
              <div className="flex w-full min-w-0 items-center gap-1.5">
                {editingThreadId === thread.id ? (
                  <input
                    ref={editTitleInputRef}
                    value={editTitleValue}
                    onClick={e => e.stopPropagation()}
                    onChange={e => onEditTitleValueChange(e.target.value)}
                    onKeyDown={e => {
                      e.stopPropagation();
                      // Ignore the Enter that confirms an IME composition
                      // candidate (CJK input) so it doesn't prematurely commit.
                      if (isImeCompositionKeyEvent(e)) return;
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        onCommitTitle(thread.id);
                      } else if (e.key === 'Escape') {
                        // Escape is an explicit cancel — suppress the commit the
                        // ensuing blur would otherwise fire.
                        onCancelEditTitle();
                      }
                    }}
                    onBlur={() => onBlurTitle(thread.id)}
                    aria-label={t('chat.editThreadTitle')}
                    data-testid={`thread-title-input-${thread.id}`}
                    className="h-5 min-w-0 flex-1 border-b border-primary-400 bg-transparent py-0 text-xs font-medium leading-none text-content-secondary outline-hidden"
                    autoFocus
                  />
                ) : (
                  <>
                    <p
                      className={`truncate flex-1 text-[14px] ${
                        selectedThreadId === thread.id
                          ? 'font-semibold text-content'
                          : 'text-content-muted'
                      }`}>
                      {resolveTitle(thread.id)}
                    </p>
                    {/* Message count occupies the trailing slot at rest and
                        yields to the row actions on hover, so the row never
                        grows or reflows between the two states. */}
                    {thread.messageCount > 0 && (
                      <span
                        data-testid={`thread-count-${thread.id}`}
                        className="flex-none rounded-full bg-surface/60 px-1.5 text-[10px] leading-4 text-content-faint group-hover:hidden">
                        {thread.messageCount > 99 ? '99+' : thread.messageCount}
                      </span>
                    )}
                  </>
                )}
                <button
                  type="button"
                  data-analytics-id="chat-sidebar-edit-thread-title"
                  onClick={e => {
                    e.stopPropagation();
                    onStartEditTitle(thread.id);
                  }}
                  aria-label={t('chat.editThreadTitle')}
                  title={t('chat.editThreadTitle')}
                  // `hidden`, not `opacity-0`: an invisible-but-laid-out button
                  // would keep reserving the trailing slot the count badge now
                  // occupies, squeezing the title on every row.
                  className="hidden h-5 w-5 flex-none items-center justify-center rounded text-content-faint transition-colors hover:bg-surface/60 hover:text-primary-500 group-hover:inline-flex">
                  <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
                    />
                  </svg>
                </button>
                <button
                  type="button"
                  data-analytics-id="chat-sidebar-delete-thread"
                  onClick={e => {
                    e.stopPropagation();
                    onRequestDelete(thread);
                  }}
                  className="hidden h-5 w-5 flex-none items-center justify-center rounded text-content-faint transition-colors hover:bg-surface/60 hover:text-coral-500 group-hover:inline-flex"
                  title={t('chat.deleteThread')}>
                  <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
                </button>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
