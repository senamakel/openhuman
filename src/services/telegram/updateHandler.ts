/**
 * Update handler — routes processed Telegram updates to Redux dispatches.
 *
 * Only handles essential update types:
 * - New/edit/delete messages
 * - Read status changes
 * - Chat metadata updates
 */

import { Api } from "telegram/tl";
import { store } from "../../store";
import {
  addMessage,
  updateMessage,
  deleteChatMessages,
  updateChat,
} from "../../store/telegram";
import { buildMessage, buildPeerId } from "./entityBuilders";

const LOG_PREFIX = "[TelegramSync]";

/**
 * Handle a single update from the UpdateManager.
 * Called for both real-time updates and difference-recovery updates.
 */
export function handleUpdate(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  update: any,
  userId: string,
  _source: "realtime" | "difference" = "realtime",
): void {
  const dispatch = store.dispatch;

  // -------------------------------------------------------------------------
  // Force sync signal from UpdateManager
  // -------------------------------------------------------------------------
  if (update && update._ === "forceSync") {
    console.log(
      LOG_PREFIX,
      "Force sync requested",
      update.channelId ? `for channel ${update.channelId}` : "(full)",
    );
    return;
  }

  // -------------------------------------------------------------------------
  // New messages
  // -------------------------------------------------------------------------
  if (
    update instanceof Api.UpdateNewMessage ||
    update instanceof Api.UpdateNewChannelMessage
  ) {
    const msg = buildMessage(update.message);
    if (msg) {
      dispatch(addMessage({ userId, message: msg }));
      dispatch(
        updateChat({
          userId,
          id: msg.chatId,
          updates: {
            lastMessage: msg,
            lastMessageDate: msg.date,
          },
        }),
      );
    }
    return;
  }

  // -------------------------------------------------------------------------
  // Edited messages
  // -------------------------------------------------------------------------
  if (
    update instanceof Api.UpdateEditMessage ||
    update instanceof Api.UpdateEditChannelMessage
  ) {
    const msg = buildMessage(update.message);
    if (msg) {
      dispatch(
        updateMessage({
          userId,
          chatId: msg.chatId,
          messageId: msg.id,
          updates: msg,
        }),
      );
    }
    return;
  }

  // -------------------------------------------------------------------------
  // Deleted messages
  // -------------------------------------------------------------------------
  if (update instanceof Api.UpdateDeleteMessages) {
    const messageIds = update.messages.map(String);
    // DeleteMessages doesn't include chatId — we need to find it from our state.
    // For common messages, we search all chats for these message IDs.
    const state = store.getState();
    const userState = state.telegram.byUser[userId];
    if (userState) {
      for (const chatId of Object.keys(userState.messages)) {
        const chatMsgs = userState.messages[chatId];
        const matching = messageIds.filter((id) => chatMsgs[id]);
        if (matching.length > 0) {
          dispatch(deleteChatMessages({ userId, chatId, messageIds: matching }));
        }
      }
    }
    return;
  }

  if (update instanceof Api.UpdateDeleteChannelMessages) {
    const channelId = String(update.channelId);
    const messageIds = update.messages.map(String);
    dispatch(deleteChatMessages({ userId, chatId: channelId, messageIds }));
    return;
  }

  // -------------------------------------------------------------------------
  // Read status (inbox)
  // -------------------------------------------------------------------------
  if (update instanceof Api.UpdateReadHistoryInbox) {
    const chatId = buildPeerId(update.peer);
    if (chatId) {
      dispatch(
        updateChat({
          userId,
          id: chatId,
          updates: { unreadCount: update.stillUnreadCount },
        }),
      );
    }
    return;
  }

  // Read status (outbox) — no direct UI action needed for outbox reads typically
  if (update instanceof Api.UpdateReadHistoryOutbox) {
    return;
  }

  // Channel read
  if (update instanceof Api.UpdateReadChannelInbox) {
    const chatId = String(update.channelId);
    dispatch(
      updateChat({
        userId,
        id: chatId,
        updates: { unreadCount: update.stillUnreadCount },
      }),
    );
    return;
  }

  // -------------------------------------------------------------------------
  // Chat/channel metadata updates
  // -------------------------------------------------------------------------
  if (update instanceof Api.UpdateChannel) {
    // The UpdateChannel event only gives us the channel ID.
    // Full re-fetch is handled by sync service if needed.
    return;
  }

  // -------------------------------------------------------------------------
  // User status (online/offline) — could extend TelegramUser in future
  // -------------------------------------------------------------------------
  if (update instanceof Api.UpdateUserStatus) {
    return;
  }

  // -------------------------------------------------------------------------
  // Raw messages from difference (not wrapped in Update*)
  // -------------------------------------------------------------------------
  if (update instanceof Api.Message || update instanceof Api.MessageService) {
    const msg = buildMessage(update);
    if (msg) {
      dispatch(addMessage({ userId, message: msg }));
      dispatch(
        updateChat({
          userId,
          id: msg.chatId,
          updates: {
            lastMessage: msg,
            lastMessageDate: msg.date,
          },
        }),
      );
    }
    return;
  }
}
