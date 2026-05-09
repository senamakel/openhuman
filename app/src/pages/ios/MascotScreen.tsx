/**
 * MascotScreen — iOS-only full-screen mascot chat interface.
 *
 * Layout:
 *   - Small header: paired desktop label + Disconnect button
 *   - YellowMascot canvas (fills the upper ~60% of screen)
 *   - Scrolling transcript of messages above the input row
 *   - Text input row pinned to bottom
 *   - Disabled PTT round button (Layer 6 placeholder)
 *
 * Chat:
 *   - Sends via openhuman.channel_web_chat RPC (same as desktop chat).
 *   - Subscribes to chat events (text_delta, chat_done, chat_error) for
 *     mascot face transitions and transcript display.
 *   - Uses useHumanMascot() to drive face/viseme state.
 *
 * PTT:
 *   - Placeholder button — disabled, tooltip "PTT coming soon".
 *   - Layer 6 will replace it with the real plugin call.
 */
import debug from 'debug';
import { type FC, type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { YellowMascot } from '../../features/human/Mascot';
import { useHumanMascot } from '../../features/human/useHumanMascot';
import {
  type ChatDoneEvent,
  type ChatErrorEvent,
  chatSend,
  type ChatTextDeltaEvent,
  subscribeChatEvents,
} from '../../services/chatService';
import { deleteProfile, listProfiles } from '../../services/transport/profileStore';

const log = debug('ios:mascot-screen');
const logErr = debug('ios:mascot-screen:error');

// -- constants ---------------------------------------------------------------

/** Default thread ID for the iOS mascot chat. Static for now. */
const IOS_THREAD_ID = 'ios-mascot-thread';

/** Model to use for iOS chat. Falls through to core default if empty. */
const IOS_CHAT_MODEL = '';

// -- types -------------------------------------------------------------------

interface Message {
  id: string;
  role: 'user' | 'assistant';
  text: string;
  /** True while a streaming response is still accumulating. */
  streaming?: boolean;
}

// -- sub-components ----------------------------------------------------------

interface TranscriptProps {
  messages: Message[];
}

const MascotChatTranscript: FC<TranscriptProps> = ({ messages }) => {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  if (messages.length === 0) return null;

  return (
    <div className="flex flex-col gap-2 px-4 pb-2 overflow-y-auto max-h-[30vh]">
      {messages.map(msg => (
        <div
          key={msg.id}
          className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}>
          <div
            className={`max-w-[80%] rounded-2xl px-3 py-2 text-sm leading-snug
              ${
                msg.role === 'user'
                  ? 'bg-[#4A83DD] text-white rounded-br-sm'
                  : 'bg-white/10 text-white/90 rounded-bl-sm'
              }
              ${msg.streaming ? 'animate-pulse' : ''}`}>
            {msg.text}
            {msg.streaming && <span className="ml-1 opacity-60">...</span>}
          </div>
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  );
};

// -- PTT placeholder ---------------------------------------------------------

const PTTButton: FC = () => {
  const [showTooltip, setShowTooltip] = useState(false);

  return (
    <div className="relative flex items-center justify-center">
      {showTooltip && (
        <div
          className="absolute bottom-full mb-2 px-3 py-1.5 rounded-lg bg-black/80 text-white text-xs
                     whitespace-nowrap pointer-events-none z-10">
          PTT coming soon
        </div>
      )}
      {/* Layer 6 will wire this button to the PTT Swift plugin.
          The onClick hook will be: startPTT() / stopPTT() from the plugin. */}
      <button
        type="button"
        disabled
        aria-label="Push to talk (coming soon)"
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
        onTouchStart={() => setShowTooltip(true)}
        onTouchEnd={() => setShowTooltip(false)}
        className="w-14 h-14 rounded-full bg-white/10 border border-white/20
                   flex items-center justify-center opacity-40 cursor-not-allowed
                   transition-opacity">
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path d="M12 1a4 4 0 0 1 4 4v6a4 4 0 0 1-8 0V5a4 4 0 0 1 4-4z" fill="white" />
          <path
            d="M19 10a1 1 0 0 1 2 0 9 9 0 0 1-18 0 1 1 0 0 1 2 0 7 7 0 0 0 14 0z"
            fill="white"
            fillOpacity="0.8"
          />
          <line
            x1="12"
            y1="19"
            x2="12"
            y2="23"
            stroke="white"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </div>
  );
};

// -- main component ----------------------------------------------------------

export const MascotScreen: FC = () => {
  const navigate = useNavigate();
  const { face } = useHumanMascot({ speakReplies: false });

  const [messages, setMessages] = useState<Message[]>([]);
  const [inputText, setInputText] = useState('');
  const [isSending, setIsSending] = useState(false);

  const streamingIdRef = useRef<string | null>(null);

  // Derive label from stored profile.
  const pairedLabel = (() => {
    const profiles = listProfiles();
    return profiles[0]?.label ?? 'Desktop';
  })();

  log('[ios] mascot screen mounted pairedLabel=%s', pairedLabel);

  // Subscribe to chat events for transcript streaming.
  useEffect(() => {
    const unsub = subscribeChatEvents({
      onTextDelta: (e: ChatTextDeltaEvent) => {
        const sid = streamingIdRef.current;
        if (!sid) return;
        setMessages(prev =>
          prev.map(m => (m.id === sid ? { ...m, text: m.text + e.delta, streaming: true } : m))
        );
      },
      onDone: (e: ChatDoneEvent) => {
        const sid = streamingIdRef.current;
        log('[ios] chat done thread_id=%s', e.thread_id);
        streamingIdRef.current = null;
        setMessages(prev =>
          prev.map(m => (m.id === sid ? { ...m, text: e.full_response, streaming: false } : m))
        );
        setIsSending(false);
      },
      onError: (e: ChatErrorEvent) => {
        logErr(
          '[ios] chat error thread_id=%s type=%s message=%s',
          e.thread_id,
          e.error_type,
          e.message
        );
        streamingIdRef.current = null;
        setIsSending(false);
        setMessages(prev => [
          ...prev,
          {
            id: `err-${Date.now()}`,
            role: 'assistant' as const,
            text: 'Something went wrong. Please try again.',
            streaming: false,
          },
        ]);
      },
    });
    return unsub;
  }, []);

  const handleSend = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      const text = inputText.trim();
      if (!text || isSending) return;

      log('[ios] sending message len=%d thread_id=%s', text.length, IOS_THREAD_ID);
      setInputText('');

      const userMsg: Message = { id: `user-${Date.now()}`, role: 'user', text };
      const assistantId = `asst-${Date.now()}`;
      streamingIdRef.current = assistantId;

      setMessages(prev => [
        ...prev,
        userMsg,
        { id: assistantId, role: 'assistant', text: '', streaming: true },
      ]);
      setIsSending(true);

      try {
        await chatSend({ threadId: IOS_THREAD_ID, message: text, model: IOS_CHAT_MODEL });
        log('[ios] chatSend enqueued thread_id=%s', IOS_THREAD_ID);
      } catch (err) {
        logErr('[ios] chatSend failed: %o', err);
        streamingIdRef.current = null;
        setIsSending(false);
        setMessages(prev =>
          prev.map(m =>
            m.id === assistantId
              ? { ...m, text: 'Failed to send. Check your connection.', streaming: false }
              : m
          )
        );
      }
    },
    [inputText, isSending]
  );

  function handleDisconnect() {
    log('[ios] disconnecting — clearing profile and navigating to /pair');
    const profiles = listProfiles();
    profiles.forEach(p => deleteProfile(p.id));
    navigate('/pair', { replace: true });
  }

  return (
    <div className="flex flex-col h-screen bg-[#0f1117] text-white overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 pt-safe-top py-3 border-b border-white/10 shrink-0">
        <div className="flex flex-col">
          <span className="text-xs text-white/40 uppercase tracking-wide">Connected to</span>
          <span className="text-sm font-medium text-white/90 truncate max-w-[200px]">
            {pairedLabel}
          </span>
        </div>
        <button
          type="button"
          onClick={handleDisconnect}
          className="px-3 py-1.5 rounded-lg text-xs text-red-400 border border-red-400/30
                     active:opacity-70 transition-opacity">
          Disconnect
        </button>
      </div>

      {/* Mascot canvas */}
      <div className="flex-1 flex items-center justify-center overflow-hidden min-h-0 py-4">
        <div className="w-full max-w-xs aspect-square">
          <YellowMascot face={face} arm="none" size="100%" />
        </div>
      </div>

      {/* Transcript */}
      <MascotChatTranscript messages={messages} />

      {/* Input row */}
      <div className="shrink-0 border-t border-white/10 px-4 pb-safe-bottom py-3">
        <form onSubmit={e => void handleSend(e)} className="flex items-center gap-3">
          {/* PTT placeholder — Layer 6 will enable this */}
          <PTTButton />

          {/* Text input */}
          <input
            type="text"
            value={inputText}
            onChange={e => setInputText(e.target.value)}
            disabled={isSending}
            placeholder={isSending ? 'Thinking...' : 'Type a message...'}
            className="flex-1 bg-white/10 text-white placeholder-white/30 rounded-xl
                       px-4 py-3 text-sm outline-none border border-white/10
                       focus:border-[#4A83DD]/60 transition-colors
                       disabled:opacity-50"
          />

          {/* Send button */}
          <button
            type="submit"
            disabled={!inputText.trim() || isSending}
            aria-label="Send message"
            className="w-10 h-10 rounded-xl bg-[#4A83DD] flex items-center justify-center
                       disabled:opacity-30 active:opacity-70 transition-opacity shrink-0">
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none" aria-hidden="true">
              <path
                d="M2 9h14M10 3l6 6-6 6"
                stroke="white"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        </form>
      </div>
    </div>
  );
};
