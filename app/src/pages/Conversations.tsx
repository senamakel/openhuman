/**
 * Thin re-export shim for the conversations panel.
 *
 * The implementation moved to `app/src/features/conversations/Conversations.tsx`
 * as part of the timeline-refactor (see `docs/plans/conversations-timeline-refactor.md`,
 * Phase 1). This shim keeps existing importers (`HumanPage`, the co-located test
 * suites) working via their current `pages/Conversations` paths during the
 * migration; it is removed in Phase 6 once consumers point at
 * `features/conversations`.
 */
export {
  default,
  ConversationsPage,
  isComposerInteractionBlocked,
  isImeCompositionKeyEvent,
  formatThreadLoadError,
} from '../features/conversations/Conversations';
