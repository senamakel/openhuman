import type bigInt from "big-integer";

/** User object as returned by Telegram MTProto API */
export interface ApiUser {
  id: bigInt.BigInteger;
  firstName?: string;
  lastName?: string;
  username?: string;
  phone?: string;
  bot?: boolean;
  status?: {
    className?: string;
    wasOnline?: number;
  };
}

/** Result containing chats array (CreateChat, CreateChannel, ImportChatInvite) */
export interface ResultWithChats {
  chats?: Array<{ id: bigInt.BigInteger; title?: string }>;
}

/** Full user info from users.GetFullUser */
export interface FullUserResult {
  fullUser?: {
    about?: string;
    botInfo?: {
      description?: string;
      commands?: Array<{ command: string; description: string }>;
    };
  };
  users?: ApiUser[];
}

/** Sticker sets from messages.GetAllStickers */
export interface StickerSetsResult {
  sets?: Array<{ id: bigInt.BigInteger; title?: string; count?: number }>;
}

/** Admin log from channels.GetAdminLog */
export interface AdminLogResult {
  events?: Array<{ date?: number; action?: { className?: string } }>;
}

/** Forum topics from channels.GetForumTopics */
export interface ForumTopicsResult {
  topics?: Array<{ id: number; title?: string }>;
}

/** Privacy rules from account.GetPrivacy */
export interface PrivacyResult {
  rules?: Array<{ className?: string }>;
}

/** Inline bot results from messages.GetInlineBotResults */
export interface InlineBotResults {
  results?: Array<{ title?: string; description?: string }>;
}

/** Chat invite from messages.ExportChatInvite */
export interface ChatInviteResult {
  link?: string;
}

/** Updates result (drafts, reactions) */
export interface UpdatesResult {
  updates?: Array<{
    draft?: { message?: string };
    peer?: { userId?: number; chatId?: number; channelId?: number };
    reactions?: {
      results?: Array<{
        reaction?: { emoticon?: string; className?: string };
        count?: number;
      }>;
    };
  }>;
}

/** Bot callback answer */
export interface BotCallbackAnswer {
  message?: string;
}

/** Contact import result */
export interface ImportContactsResult {
  imported?: unknown[];
}

/** Contact ID entry */
export interface ContactIdEntry {
  userId?: number;
}

/** Photo object from the API */
export interface ApiPhoto {
  id: bigInt.BigInteger;
  date?: number;
  accessHash?: bigInt.BigInteger;
  fileReference?: Buffer;
}

/** API message shape (from search results) */
export interface ApiMessage {
  id: number;
  message?: string;
  date?: number;
  pinned?: boolean;
}

/** Reply markup types for inline buttons */
export interface ReplyMarkupRow {
  buttons?: Array<{ text?: string }>;
}

export interface MessageWithReplyMarkup {
  id: string | number;
  replyMarkup?: { rows?: ReplyMarkupRow[] };
}

/** Bot command input shape */
export interface BotCommandInput {
  command?: string;
  description?: string;
}

/** Contact input shape (for importContacts) */
export interface ContactInput {
  phone?: string;
  first_name?: string;
  last_name?: string;
}
