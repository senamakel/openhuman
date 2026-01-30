/**
 * Entity builders — convert raw GramJS objects to Redux-compatible types.
 */

import type {
  TelegramChat,
  TelegramMessage,
  TelegramUser,
} from "../../store/telegram/types";

// ---------------------------------------------------------------------------
// Peer ID helpers
// ---------------------------------------------------------------------------

/**
 * Convert a GramJS peer object to a string ID.
 * Handles PeerUser, PeerChat, PeerChannel, and InputPeer* variants.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function buildPeerId(peer: any): string {
  if (!peer || typeof peer !== "object") return "";
  if (peer.userId != null) return String(peer.userId);
  if (peer.chatId != null) return String(peer.chatId);
  if (peer.channelId != null) return String(peer.channelId);
  return "";
}

/**
 * Determine chat type from a GramJS entity object.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getChatType(entity: any): TelegramChat["type"] {
  if (!entity) return "private";
  const className: string = entity.className ?? "";
  if (className === "Channel") {
    return entity.megagroup ? "supergroup" : "channel";
  }
  if (className === "Chat" || className === "ChatForbidden") {
    return "group";
  }
  return "private";
}

// ---------------------------------------------------------------------------
// Chat builder
// ---------------------------------------------------------------------------

/**
 * Build a TelegramChat from a GramJS dialog + its peer entity.
 *
 * @param dialog - Raw GramJS Dialog object (has peer, unreadCount, pinned, etc.)
 * @param entity - The resolved entity (User / Chat / Channel) for this dialog
 * @param lastMsg - Optional last message already converted
 */
export function buildChat(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  dialog: any,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  entity: any,
  lastMsg?: TelegramMessage,
): TelegramChat {
  const id = buildPeerId(dialog.peer ?? entity);
  const type = getChatType(entity);

  const chat: TelegramChat = {
    id,
    type,
    unreadCount: dialog.unreadCount ?? 0,
    isPinned: Boolean(dialog.pinned),
  };

  // Title
  if (type === "private") {
    const firstName: string = entity?.firstName ?? "";
    const lastName: string = entity?.lastName ?? "";
    chat.title = [firstName, lastName].filter(Boolean).join(" ") || `User ${id}`;
  } else {
    chat.title = entity?.title ?? `Chat ${id}`;
  }

  // Username
  if (entity?.username) chat.username = entity.username;

  // Access hash
  if (entity?.accessHash != null) {
    chat.accessHash = String(entity.accessHash);
  }

  // Photo
  if (entity?.photo && entity.photo.className !== "ChatPhotoEmpty") {
    chat.photo = {
      smallFileId: entity.photo.photoId
        ? String(entity.photo.photoId)
        : undefined,
    };
  }

  // Participants count
  if (entity?.participantsCount != null) {
    chat.participantsCount = entity.participantsCount;
  }

  // Last message
  if (lastMsg) {
    chat.lastMessage = lastMsg;
    chat.lastMessageDate = lastMsg.date;
  }

  return chat;
}

// ---------------------------------------------------------------------------
// Message builder
// ---------------------------------------------------------------------------

/**
 * Build a TelegramMessage from a raw GramJS message object.
 * Compatible with both Api.Message and the raw objects from GetDialogs/GetHistory.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function buildMessage(msg: any, fallbackChatId?: string): TelegramMessage | null {
  if (!msg || typeof msg !== "object") return null;

  const id = msg.id;
  if (id === undefined || id === null) return null;

  // Determine chat ID from peerId or fallback
  let chatId = fallbackChatId ?? "";
  if (msg.peerId) {
    chatId = buildPeerId(msg.peerId) || chatId;
  }

  const telegramMsg: TelegramMessage = {
    id: String(id),
    chatId,
    date: typeof msg.date === "number" ? msg.date : 0,
    message: typeof msg.message === "string" ? msg.message : "",
    isOutgoing: Boolean(msg.out),
    isEdited: Boolean(msg.editDate),
    isForwarded: Boolean(msg.fwdFrom),
  };

  // From ID
  if (msg.fromId && typeof msg.fromId === "object") {
    telegramMsg.fromId = buildPeerId(msg.fromId);
  }

  // Reply
  if (msg.replyTo && typeof msg.replyTo === "object") {
    if (msg.replyTo.replyToMsgId) {
      telegramMsg.replyToMessageId = String(msg.replyTo.replyToMsgId);
    }
    if (msg.replyTo.replyToTopId) {
      telegramMsg.threadId = String(msg.replyTo.replyToTopId);
    }
  }

  // Media
  if (msg.media && typeof msg.media === "object") {
    const className = (msg.media.constructor as { className?: string })?.className;
    telegramMsg.media = { type: className ?? "unknown" };
  }

  // Views
  if (typeof msg.views === "number") {
    telegramMsg.views = msg.views;
  }

  return telegramMsg;
}

// ---------------------------------------------------------------------------
// User builder
// ---------------------------------------------------------------------------

/**
 * Build a TelegramUser from a raw GramJS User object.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function buildUser(user: any): TelegramUser | null {
  if (!user || typeof user !== "object") return null;
  if (user.id === undefined || user.id === null) return null;

  return {
    id: String(user.id),
    firstName: user.firstName ?? "",
    lastName: user.lastName ?? undefined,
    username: user.username ?? undefined,
    phoneNumber: user.phone ?? undefined,
    isBot: Boolean(user.bot),
    isVerified: user.verified ? true : undefined,
    isPremium: user.premium ? true : undefined,
    accessHash: user.accessHash != null ? String(user.accessHash) : undefined,
  };
}

// ---------------------------------------------------------------------------
// Entity map builder (from GetDialogs response)
// ---------------------------------------------------------------------------

/**
 * Build a map of entities (users + chats/channels) indexed by their ID.
 * GramJS returns separate `users` and `chats` arrays — this merges them.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function buildEntityMap(result: any): Map<string, any> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const map = new Map<string, any>();

  const users: unknown[] = result?.users ?? [];
  const chats: unknown[] = result?.chats ?? [];

  for (const u of users) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const user = u as any;
    if (user?.id != null) {
      map.set(String(user.id), user);
    }
  }

  for (const c of chats) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const chat = c as any;
    if (chat?.id != null) {
      map.set(String(chat.id), chat);
    }
  }

  return map;
}
