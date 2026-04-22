import debug from 'debug';

import { socketService } from '../../services/socketService';
import { store } from '../../store';
import {
  type NotificationCategory,
  type NotificationItem,
  notificationReceived,
} from '../../store/notificationSlice';
import { showNativeNotification } from './tauriBridge';

const log = debug('native-notifications');

let started = false;

interface ChatDonePayload {
  thread_id?: string;
  request_id?: string;
  full_response?: string;
  rounds_used?: number;
}

interface ChatErrorPayload {
  thread_id?: string;
  request_id?: string;
  message?: string;
}

function windowIsFocused(): boolean {
  if (typeof document === 'undefined') return true;
  return document.hasFocus();
}

function dispatchAndMaybeBanner(
  category: NotificationCategory,
  item: Omit<NotificationItem, 'category' | 'timestamp' | 'read'>
): void {
  const prefs = store.getState().notifications.preferences;
  if (!prefs[category]) {
    log('category %s disabled, skipping', category);
    return;
  }
  const full: NotificationItem = { ...item, category, timestamp: Date.now(), read: false };
  store.dispatch(notificationReceived(full));
  // Only fire OS-level banner when the user isn't already looking at the
  // window — otherwise the in-app center is enough and a native toast is
  // redundant noise.
  if (!windowIsFocused()) {
    void showNativeNotification({ title: full.title, body: full.body, tag: full.id });
  }
}

function truncate(input: string, max: number): string {
  if (input.length <= max) return input;
  return `${input.slice(0, max - 1)}…`;
}

/**
 * Subscribe to socket events that should surface as notifications (agent
 * completions, chat errors, connection drops). Idempotent. Safe to call at
 * app boot before the socket has connected — the socketService queues
 * listeners until the socket is ready.
 */
export function startNativeNotificationsService(): void {
  if (started) return;
  started = true;

  socketService.on('chat_done', (...args: unknown[]) => {
    const p = (args[0] ?? {}) as ChatDonePayload;
    dispatchAndMaybeBanner('agents', {
      id: `chat_done:${p.thread_id ?? 'unknown'}:${p.request_id ?? Date.now()}`,
      title: 'Agent reply ready',
      body: truncate(p.full_response?.trim() || 'Agent finished processing.', 160),
      deepLink: '/chat',
    });
  });

  socketService.on('chat_error', (...args: unknown[]) => {
    const p = (args[0] ?? {}) as ChatErrorPayload;
    dispatchAndMaybeBanner('system', {
      id: `chat_error:${p.thread_id ?? 'unknown'}:${p.request_id ?? Date.now()}`,
      title: 'Agent error',
      body: truncate(p.message || 'An error occurred during inference.', 160),
      deepLink: '/chat',
    });
  });

  socketService.on('disconnect', (...args: unknown[]) => {
    const reason = typeof args[0] === 'string' ? args[0] : 'unknown';
    dispatchAndMaybeBanner('system', {
      id: `socket_disconnect:${Date.now()}`,
      title: 'Connection lost',
      body: `OpenHuman lost its connection to the core service (${truncate(reason, 80)}).`,
    });
  });
}

export function stopNativeNotificationsService(): void {
  started = false;
}

/** Exposed for tests — dispatch as if a chat_done event arrived. */
export function __handleChatDoneForTests(payload: ChatDonePayload): void {
  dispatchAndMaybeBanner('agents', {
    id: `chat_done:${payload.thread_id ?? 'unknown'}:${payload.request_id ?? Date.now()}`,
    title: 'Agent reply ready',
    body: truncate(payload.full_response?.trim() || 'Agent finished processing.', 160),
    deepLink: '/chat',
  });
}

/** Exposed for tests — resets module singletons between runs. */
export function __resetForTests(): void {
  started = false;
}
