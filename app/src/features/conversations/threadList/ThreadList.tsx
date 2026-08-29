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
      {/* Rows carry no padding gutter of their own — a thread pill spans the
          full width the scroll container gives it, so its hover/selected fill
          reads as the width of the list rather than a floating inset card, with
          `px-2` breathing it an equal 8px off either edge.

          `scrollbar-width` makes the bar an OVERLAY here, and it is doing so by
          opting this one pane OUT of the app-wide rules rather than by adding
          anything. `index.css` paints every pane's bar with `::-webkit-scrollbar`
          at a fixed 10px whose track is permanently reserved (only the thumb's
          colour animates — toggling `width` would reflow the pane on every
          scroll), so a full-bleed list silently lost 10px on the right the
          moment it overflowed. That stylesheet's own comment records the escape
          hatch: a standard `scrollbar-*` property takes precedence and disables
          the `::-webkit-scrollbar` styling entirely. The runtime is Wry as of
          #5456 (`app/src-tauri/Cargo.toml` enables the `wry` feature; the CEF
          notes around it are historical), so on macOS/Linux WebKit that hands
          the pane back the platform's native overlay bar — zero reserved width,
          fading on its own, which is what the `data-scrolling` machinery in
          `lib/autoHideScrollbars.ts` exists to imitate everywhere else.

          `scrollbar-gutter` stays for the platform where that is not true.
          Windows WebView2 is Chromium and still lays a classic bar out in flow;
          per spec a gutter is ignored for overlay bars, so the declaration is
          inert on macOS and reserves a matched band on both sides on Windows.
          The pill is therefore symmetric on every platform and never resizes as
          the list crosses the overflow threshold — it is simply 8px inset where
          the bar overlays and 8px + the bar's width where it does not.

          Only `scrollbar-width` is set, not `scrollbar-color`: colouring the
          thumb is what tips WebKit out of overlay mode and back into a laid-out
          bar, which would undo the whole point. Native overlay bars already
          track the platform's light/dark appearance.

          Vertical rhythm is `gap` on the column, not a margin on each row — a
          margin also lands after the last row and pads the scroll floor
          unevenly against `pb-3`. */}
      <div className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-3 [scrollbar-gutter:stable_both-edges] [scrollbar-width:thin]">
        {/* "New conversation" as a row, not a header icon. It is the same
            affordance as a thread row — pick a conversation to work in — so it
            takes the same shape: `h-8` pill, same radius, same hover fill, same
            14px label, sitting in the same column. As a 20px icon docked in a
            section header it was both the smallest hit target in the sidebar
            and the only control there that did not look like the thing it
            produced. That header is gone with it: it was a group label for a
            list that is already the only thing in its region, under a separator
            that already divides it from the nav above.

            A `<button>` rather than a `div[role=button]` like the thread rows:
            those rows carry nested action buttons (rename, delete) and cannot
            legally nest a button inside a button, which is why they hand-roll
            the role and key handling. This row has no children, so it can be
            the real element and get Enter/Space, focus and semantics for free.

            Outline, not filled: a solid accent button would make the loudest
            thing in the sidebar an action nobody needs most of the time, and it
            would outrank the selected conversation, which is the one row that
            should carry emphasis. A border states the affordance and leaves
            `text-content-muted` matching an unselected thread row. The border
            uses the same `content-faint` token as the composer's outline, so
            the two read as one edge language rather than two.

            `mb-1` on top of the column's `gap-0.5`: this row is a different
            kind of thing from the conversations under it, and an outlined box
            sitting on the exact rhythm of the plain rows reads as the first
            item in the list rather than as its own control. */}
        <button
          type="button"
          data-testid="new-thread-button"
          data-analytics-id="chat-sidebar-new-thread"
          onClick={onCreateThread}
          title={t('chat.newThreadShortcut')}
          className="group mb-1 flex h-8 w-full flex-none cursor-pointer items-center gap-1.5 rounded-md border border-content-faint/35 px-3 text-left text-[14px] text-content-muted transition-colors hover:border-content-faint/60 hover:bg-surface/40 hover:text-content-secondary dark:hover:bg-surface/60">
          <svg
            className="h-3.5 w-3.5 flex-none"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
          </svg>
          <span className="truncate">{t('chat.newConversation')}</span>
        </button>
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
              className={`group flex h-8 w-full flex-none cursor-pointer items-center rounded-md px-3 text-left transition-colors ${
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
