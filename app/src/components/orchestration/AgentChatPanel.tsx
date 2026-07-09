/**
 * AgentChatPanel — chat with the main agent, styled like the app's normal chat.
 *
 * Reuses the app's normal chat window look: a single centered message pane
 * rendered with the shared {@link SessionTranscript} (chat-window bubbles +
 * inline harness activity), the {@link ChatNewWindowHero} welcome panel on an
 * empty conscious thread, the Master composer, and — in place of the generic
 * chat's super-context / quick-reasoning controls — a bottom **Conscious /
 * Subconscious** toggle. "Conscious" is the orchestration master session;
 * "Subconscious" is the subconscious steering loop (which adds a steering
 * header and drops the composer, since you watch it rather than message it).
 *
 * When the agent engages a fleet session (a session parked on an approval), an
 * inline **View session** card surfaces below the thread; opening it slides in a
 * right-hand session side-tab showing that session's live chat + a reply
 * composer. The side-tab never opens on its own — the user clicks the card.
 */
import debugFactory from 'debug';
import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import {
  orchestrationClient,
  type SessionSummary,
} from '../../lib/orchestration/orchestrationClient';
import {
  MASTER_CHAT_KEY,
  SUBCONSCIOUS_CHAT_KEY,
  useOrchestrationChats,
} from '../../lib/orchestration/useOrchestrationChats';
import {
  useContactSessions,
  useSessionTranscript,
} from '../../lib/orchestration/useOrchestrationSessions';
import { subconsciousTrigger } from '../../utils/tauriCommands/subconscious';
import ChatNewWindowHero from '../chat/ChatNewWindowHero';
import Button from '../ui/Button';
import SessionTranscript from './SessionTranscript';

const debug = debugFactory('orchestration:agent-chat');

function sessionLabel(session: SessionSummary): string {
  return session.label?.trim() || session.sessionId;
}

/** Right-hand session side-tab: a fleet session's live chat + reply composer. */
function SessionDrawer({ session, onClose }: { session: SessionSummary; onClose: () => void }) {
  const { t } = useT();
  const { state, messages, refresh } = useSessionTranscript(session.sessionId);
  const [body, setBody] = useState('');
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);

  const submit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const trimmed = body.trim();
      if (!trimmed || sending) return;
      setSending(true);
      setSendError(null);
      debug('[orchestration:agent-chat] session reply: send session=%s', session.sessionId);
      void orchestrationClient
        .sendMasterMessage({
          body: trimmed,
          recipient: session.agentId,
          sessionId: session.sessionId,
        })
        .then(() => {
          setBody('');
          void refresh();
        })
        .catch((error: unknown) => {
          const message = error instanceof Error ? error.message : String(error);
          debug(
            '[orchestration:agent-chat] session reply: failed session=%s %s',
            session.sessionId,
            message
          );
          setSendError(message);
        })
        .finally(() => setSending(false));
    },
    [body, sending, session.agentId, session.sessionId, refresh]
  );

  return (
    <aside
      data-testid="orch-agent-session-drawer"
      className="absolute inset-y-0 right-0 z-30 flex w-[24rem] flex-col border-l border-line bg-surface shadow-[-8px_0_24px_-12px_rgba(0,0,0,0.25)]">
      <div className="flex items-center gap-2 border-b border-line px-4 py-2.5">
        <span className="flex h-8 w-8 flex-none items-center justify-center rounded-full border border-line bg-surface-strong text-[11px] font-semibold text-content-secondary">
          {sessionLabel(session).slice(0, 2)}
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold text-content">{sessionLabel(session)}</p>
          <p className="truncate text-[11px] text-content-muted">
            {session.status === 'waiting-approval' ? t('orchPage.connections.status.needsYou') : ''}
          </p>
        </div>
        <button
          type="button"
          onClick={onClose}
          data-testid="orch-agent-drawer-close"
          className="flex-none rounded p-1 text-content-faint transition hover:bg-surface-hover">
          ✕
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {state.status === 'loading' ? (
          <p className="py-6 text-center text-sm text-content-muted">
            {t('tinyplaceOrchestration.loading')}
          </p>
        ) : state.status === 'error' ? (
          <p className="py-6 text-center text-sm text-coral-600 dark:text-coral-300">
            {t('tinyplaceOrchestration.failedToLoad')}: {state.message}
          </p>
        ) : messages.length === 0 ? (
          <p className="py-6 text-center text-sm text-content-faint">
            {t('tinyplaceOrchestration.noMessages')}
          </p>
        ) : (
          <SessionTranscript messages={messages} />
        )}
      </div>
      <form className="border-t border-line p-3" onSubmit={submit}>
        {sendError ? (
          <p
            data-testid="orch-agent-drawer-reply-error"
            className="mb-2 rounded-md bg-coral-50 px-2 py-1 text-xs text-coral-700 dark:bg-coral-500/10 dark:text-coral-300">
            {t('tinyplaceOrchestration.composer.sendFailed')}: {sendError}
          </p>
        ) : null}
        <div className="flex gap-2">
          <input
            value={body}
            onChange={e => setBody(e.target.value)}
            placeholder={t('orchPage.connections.replyPlaceholder')}
            data-testid="orch-agent-drawer-reply"
            className="min-w-0 flex-1 rounded-md border border-line bg-surface px-3 py-2 text-sm text-content outline-none transition focus:border-primary-500 focus:ring-2 focus:ring-primary-500/20"
          />
          <Button type="submit" variant="primary" size="sm" disabled={!body.trim() || sending}>
            {t('tinyplaceOrchestration.composer.send')}
          </Button>
        </div>
      </form>
    </aside>
  );
}

export default function AgentChatPanel() {
  const { t } = useT();
  const {
    sessionsState,
    messagesState,
    chats,
    selectedId,
    selected,
    status,
    masterError,
    selectChat,
    refresh,
    sendMessage,
  } = useOrchestrationChats(t);
  const contactSessions = useContactSessions();

  const [composerBody, setComposerBody] = useState('');
  const [sending, setSending] = useState(false);
  const [runningReview, setRunningReview] = useState(false);
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const submitComposer = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const body = composerBody.trim();
      if (!body || sending) return;
      setSending(true);
      void sendMessage(selected, body).then(ok => {
        if (!mountedRef.current) return;
        if (ok) setComposerBody('');
        setSending(false);
      });
    },
    [composerBody, sending, sendMessage, selected]
  );

  const runSteeringReview = useCallback(async () => {
    debug('steering review: trigger');
    setRunningReview(true);
    try {
      await subconsciousTrigger('tinyplace');
    } catch (err) {
      debug('steering review trigger failed: %o', err);
    } finally {
      setRunningReview(false);
    }
  }, []);

  const steeringText = status?.steering?.text?.trim() || null;
  const isMasterSelected = selectedId === MASTER_CHAT_KEY;
  const isSubconscious = selectedId === SUBCONSCIOUS_CHAT_KEY;

  const rail = chats.filter(c => c.id === MASTER_CHAT_KEY || c.id === SUBCONSCIOUS_CHAT_KEY);
  // Sessions the agent has engaged that are parked on an approval → "needs you".
  const pinged = contactSessions.sessions.filter(s => s.status === 'waiting-approval');
  const openSession = contactSessions.sessions.find(s => s.sessionId === openSessionId) ?? null;
  // Empty conscious thread → show the shared welcome hero (reuses the generic
  // chat's "new window" panel) instead of the bare "no messages" line.
  const showHero = isMasterSelected && (selected?.messages.length ?? 0) === 0;

  return (
    <div className="relative flex h-full min-h-[520px] flex-col overflow-hidden rounded-xl border border-line bg-surface shadow-soft">
      {/* Message pane. */}
      <main className="relative flex min-w-0 flex-1 flex-col bg-surface/70 dark:bg-black/40">
        {/* Subconscious steering header. */}
        {isSubconscious ? (
          <div
            data-testid="orch-agent-steering-header"
            className="flex items-center justify-between gap-3 border-b border-line bg-amber-50/40 px-5 py-2.5 dark:bg-amber-500/5">
            <div className="min-w-0">
              <p className="text-xs font-medium text-content">
                {steeringText
                  ? t('tinyplaceOrchestration.steeringHeader.current')
                  : t('tinyplaceOrchestration.steeringHeader.none')}
              </p>
              {steeringText ? (
                <p className="mt-0.5 truncate text-xs text-content-muted">{steeringText}</p>
              ) : null}
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void runSteeringReview()}
              disabled={runningReview}>
              {runningReview
                ? t('tinyplaceOrchestration.steeringHeader.running')
                : t('tinyplaceOrchestration.steeringHeader.runReview')}
            </Button>
          </div>
        ) : null}

        {sessionsState.status === 'loading' || messagesState.status === 'loading' ? (
          <div className="flex flex-1 items-center justify-center text-sm text-content-muted">
            {t('tinyplaceOrchestration.loading')}
          </div>
        ) : sessionsState.status === 'error' || messagesState.status === 'error' ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-coral-600 dark:text-coral-300">
            <p>
              {t('tinyplaceOrchestration.failedToLoad')}:{' '}
              {sessionsState.status === 'error'
                ? sessionsState.message
                : messagesState.status === 'error'
                  ? messagesState.message
                  : ''}
            </p>
            <Button variant="secondary" size="sm" onClick={() => void refresh()}>
              {t('common.retry')}
            </Button>
          </div>
        ) : showHero && pinged.length === 0 ? (
          // Empty conscious thread: reuse the generic chat's welcome hero. It
          // sizes to h-full and centers, so give it the whole pane. Skipped when
          // there are View-session cards to surface (they need the list layout).
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5">
            <ChatNewWindowHero />
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <div className="mx-auto w-full max-w-[48.75rem] space-y-3 px-5 py-5">
              {selected?.messages.length ? (
                <SessionTranscript messages={selected.messages} />
              ) : showHero ? null : (
                <p className="py-10 text-center text-sm text-content-faint">
                  {t('tinyplaceOrchestration.noMessages')}
                </p>
              )}

              {/* View-session cards for sessions the agent engaged (needs you). */}
              {pinged.map(session => (
                <div key={session.sessionId} className="flex justify-start">
                  <button
                    type="button"
                    data-testid={`orch-agent-view-session-${session.sessionId}`}
                    onClick={() => setOpenSessionId(session.sessionId)}
                    className="flex w-full max-w-[85%] items-center gap-3 rounded-xl border border-primary-200 bg-primary-50 px-3 py-2.5 text-left transition hover:bg-primary-100/60 dark:border-primary-500/30 dark:bg-primary-900/20">
                    <span className="flex h-8 w-8 flex-none items-center justify-center rounded-lg bg-primary-500/15 text-sm text-primary-600 dark:text-primary-300">
                      ⧉
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium text-content">
                        {sessionLabel(session)}
                      </span>
                      <span className="block truncate text-[11px] text-amber-600 dark:text-amber-300">
                        {t('orchPage.connections.status.needsYou')}
                      </span>
                    </span>
                    <span className="flex-none rounded-lg bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white">
                      {t('orchPage.agent.viewSession')}
                    </span>
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Master composer. */}
        {isMasterSelected && sessionsState.status === 'ok' ? (
          <form
            className="flex flex-col gap-2 border-t border-line px-5 py-3"
            onSubmit={submitComposer}>
            {masterError ? (
              <p className="rounded-md bg-coral-50 px-2 py-1 text-xs text-coral-700 dark:bg-coral-500/10 dark:text-coral-300">
                {t('tinyplaceOrchestration.composer.sendFailed')}: {masterError}
              </p>
            ) : null}
            <div className="flex gap-2">
              <input
                data-testid="orch-agent-composer-input"
                value={composerBody}
                onChange={event => setComposerBody(event.target.value)}
                placeholder={t('tinyplaceOrchestration.composer.placeholder')}
                className="min-w-0 flex-1 rounded-md border border-line bg-surface px-3 py-2 text-sm text-content outline-none transition focus:border-primary-500 focus:ring-2 focus:ring-primary-500/20"
              />
              <Button
                type="submit"
                variant="primary"
                size="sm"
                data-testid="orch-agent-composer-send"
                disabled={!composerBody.trim() || sending}>
                {t('tinyplaceOrchestration.composer.send')}
              </Button>
            </div>
          </form>
        ) : null}

        {/* Conscious / Subconscious toggle — reuses the generic chat's footer
            control-row slot (where super-context + quick/reasoning live), but
            here it switches the master vs subconscious orchestration session. */}
        <div
          className={`flex items-center gap-2 px-5 py-3 ${isMasterSelected ? '' : 'border-t border-line'}`}>
          <div
            className="flex h-7 items-center rounded-full border border-line bg-surface-subtle p-0.5"
            role="radiogroup"
            aria-label={t('orchPage.agent.modeLabel')}>
            {rail.map(chat => {
              const active = selectedId === chat.id;
              const label =
                chat.id === SUBCONSCIOUS_CHAT_KEY
                  ? t('orchPage.agent.subconsciousTab')
                  : t('orchPage.agent.consciousTab');
              return (
                <button
                  key={chat.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  data-testid={`orch-agent-tab-${chat.id}`}
                  onClick={() => selectChat(chat.id)}
                  className={`flex items-center gap-1.5 rounded-full px-3 py-0.5 text-xs font-medium transition-all ${
                    active
                      ? 'bg-surface text-content shadow-sm'
                      : 'text-content-muted hover:text-content-secondary'
                  }`}>
                  {label}
                  {chat.unread > 0 ? (
                    <span className="inline-flex min-w-4 items-center justify-center rounded-full bg-primary-500 px-1 text-[10px] font-semibold text-content-inverted">
                      {chat.unread}
                    </span>
                  ) : null}
                </button>
              );
            })}
          </div>
        </div>
      </main>

      {/* Session side-tab (opens on demand from a View-session card). */}
      {openSession ? (
        // `key` resets the drawer's local composer/sending state when switching
        // to a different session, so a draft reply never leaks across sessions.
        <SessionDrawer
          key={openSession.sessionId}
          session={openSession}
          onClose={() => setOpenSessionId(null)}
        />
      ) : null}
    </div>
  );
}
