import { type ReactNode, forwardRef } from 'react';

import ComposerTokenStats from '../../../components/chat/ComposerTokenStats';

/**
 * Shared frame for the conversation flow, used in two formats:
 *
 *  - `page` — the centered, width-capped main chat (the `/chat` route and the
 *    embedded mascot/Human panels). Composer present.
 *  - `panel` — the right-docked side panel (a sub-agent's transcript). Same
 *    body rendering, but flush against the panel edge, the composer removed,
 *    and the bottom token stats kept.
 *
 * The component owns only the *layout*: a scrollable body (whatever transcript
 * the caller renders as `children` — message bubbles, the tool timeline, a
 * sub-agent transcript) sitting above a footer that stacks any banners, an
 * optional composer, and the `ComposerTokenStats` strip. Keeping the body as
 * `children` lets both surfaces reuse the exact same flow atoms
 * (`AgentMessageBubble`, `ToolTimelineBlock`, …) without this frame knowing
 * anything about messages, Redux, or sending — so a sub-agent view is "the
 * conversations flow with the composer removed", recursively.
 *
 * The scroll container ref is forwarded so the caller can keep its existing
 * auto-scroll / scroll-anchoring behaviour.
 */
export interface ConversationViewProps {
  /** The transcript body: message bubbles, tool timeline, sub-agent transcript. */
  children: ReactNode;
  /**
   * `page` (default) centers + width-caps the body and floats the footer over
   * a fade. `panel` renders flush for a side drawer: no centering, no fade, the
   * footer pinned in-flow at the bottom.
   */
  variant?: 'page' | 'panel';
  /**
   * Banners / advisories / todo strip stacked directly above the composer
   * (upsell, budget, send advisories). Rendered inside the footer so it shares
   * the footer's shrink-to-fit behaviour on short windows.
   */
  footerLead?: ReactNode;
  /**
   * The composer element (textarea, mic, voice). Omit entirely to remove the
   * composer — the `panel` variant for a read-only sub-agent transcript passes
   * nothing here, which is what makes it "the conversations flow without the
   * composer".
   */
  composer?: ReactNode;
  /** Show the `model · in · out · turns · context%` stats strip. */
  showStats?: boolean;
  /** Resolved model id for the stats strip. */
  statsModel?: string | null;
  /** Controls rendered on the stats row's trailing edge (quick/reasoning
   * toggle, background-processes button, files chip) — page variant only. */
  statsTrailing?: ReactNode;
}

export const ConversationView = forwardRef<HTMLDivElement, ConversationViewProps>(
  function ConversationView(
    { children, variant = 'page', footerLead, composer, showStats, statsModel, statsTrailing },
    scrollRef
  ) {
    const isPanel = variant === 'panel';

    return (
      <>
        {/* Scrollable transcript body. `min-h-0` lets this basis-0 flex child
            shrink to 0 so the footer can take the space (and scroll) on short
            windows. */}
        <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
          {children}
        </div>

        {/* Full-width fade so messages dissolve into the background behind the
            floating composer. Page variant only — the panel pins its footer. */}
        {!isPanel && (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-28 bg-gradient-to-t from-white via-white/90 to-transparent dark:from-black dark:via-black/90"
          />
        )}

        <div
          data-testid="conversation-footer"
          className={
            isPanel
              ? 'min-h-0 shrink overflow-y-auto border-t border-stone-200 px-4 py-3 dark:border-neutral-800'
              : 'absolute inset-x-0 bottom-0 z-20 mx-auto w-full max-w-[48.75rem] px-4 pb-4 pt-6'
          }>
          {footerLead}
          {composer}
          {showStats && (
            <div
              className={`flex items-center justify-between gap-2 ${composer || footerLead ? 'mt-2' : ''}`}
              data-walkthrough="chat-agent-panel">
              <ComposerTokenStats model={statsModel} />
              {statsTrailing}
            </div>
          )}
        </div>
      </>
    );
  }
);
