import { forwardRef, type ReactNode } from 'react';

/**
 * Shared frame for the conversation flow, used in two formats:
 *
 *  - `page` / `sidebar` — the main chat (the `/chat` route and the embedded
 *    mascot/Human right-rail). Footer carries the composer + token stats.
 *  - `panel` — the right-docked side panel (a sub-agent's transcript). The same
 *    body rendering, flush against the panel edge, with the composer simply
 *    left out of the footer and the `ComposerTokenStats` strip kept.
 *
 * The component owns only the *layout*: a scrollable body (whatever transcript
 * the caller renders as `children` — message bubbles, the tool timeline, a
 * sub-agent transcript) sitting above a caller-owned `footer`. Keeping both the
 * body and the footer as slots lets every surface reuse the exact same flow
 * atoms (`AgentMessageBubble`, `ToolTimelineBlock`, `ComposerTokenStats`, …)
 * without this frame knowing anything about messages, Redux, or sending — so a
 * sub-agent view is literally "the conversations flow, footer minus the
 * composer", recursively.
 *
 * The scroll-container ref is forwarded so the caller keeps its existing
 * auto-scroll / scroll-anchoring behaviour.
 */
export interface ConversationViewProps {
  /** The transcript body: message bubbles, tool timeline, sub-agent transcript. */
  children: ReactNode;
  /**
   * Layout format:
   *  - `page` (default) — floats the footer absolutely over a bottom fade; the
   *    body scrolls under it (the body supplies its own bottom padding).
   *  - `sidebar` — the embedded right-rail chat: in-flow footer, width-capped
   *    and centered, shrink-to-fit on short windows (#3785). No fade.
   *  - `panel` — a side drawer (sub-agent transcript): flush, no centering, no
   *    fade, the footer pinned in-flow with a top border.
   */
  variant?: 'page' | 'sidebar' | 'panel';
  /**
   * Footer content — banners, the composer, the `ComposerTokenStats` strip,
   * whatever the surface needs. Rendered inside the positioned footer shell.
   * Omit (or pass nothing) to render no footer at all. A sub-agent panel passes
   * just the stats strip here: that absence of a composer is what makes it the
   * conversations flow "with the composer removed".
   */
  footer?: ReactNode;
  /**
   * `data-walkthrough` anchor for the footer container (the page/sidebar chat
   * tags it `home-cta`). Omitted for the panel variant.
   */
  footerWalkthrough?: string;
}

export const ConversationView = forwardRef<HTMLDivElement, ConversationViewProps>(
  ({ children, variant = 'page', footer, footerWalkthrough }, scrollRef) => {
    // Only the page variant floats its footer over a fade; sidebar/panel pin it.
    const showFade = variant === 'page';
    const footerClass =
      variant === 'page'
        ? 'absolute inset-x-0 bottom-0 z-20 mx-auto w-full max-w-[48.75rem] px-4 pb-4 pt-6'
        : variant === 'sidebar'
          ? 'mx-auto w-full max-w-[48.75rem] min-h-0 overflow-y-auto px-4 py-3'
          : 'min-h-0 shrink overflow-y-auto border-t border-stone-200 px-4 py-3 dark:border-neutral-800';

    return (
      <>
        {/* Scrollable transcript body. `min-h-0` lets this basis-0 flex child
            shrink to 0 so the footer can take the space (and scroll) on short
            windows (#3785). */}
        <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto">
          {children}
        </div>

        {/* Full-width fade so messages dissolve into the background behind the
            floating composer. Page variant only — sidebar/panel pin the footer. */}
        {showFade && (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-28 bg-gradient-to-t from-white via-white/90 to-transparent dark:from-black dark:via-black/90"
          />
        )}

        {footer != null && (
          <div
            data-testid="conversation-footer"
            data-walkthrough={footerWalkthrough}
            className={footerClass}>
            {footer}
          </div>
        )}
      </>
    );
  }
);

ConversationView.displayName = 'ConversationView';
