/**
 * Telegram Sync Service
 *
 * Loads all chats via GetDialogs in 100-chat batches, preloads messages for
 * the top 20 chats, and registers real-time update handling.
 *
 * Coexists independently with the existing MCP tool system.
 */

import { Api } from "telegram/tl";
import { Raw } from "telegram/events";
import bigInt from "big-integer";
import { mtprotoService } from "../mtprotoService";
import { updateManager } from "../updateManager";
import { store } from "../../store";
import {
  setSyncStatus,
  replaceChats,
  addChats,
  setUsers,
  addUsers,
  addChatMessagesById,
  setCommonBoxState,
  setChannelPts,
} from "../../store/telegram";
import type { TelegramChat, TelegramUser } from "../../store/telegram/types";
import {
  buildChat,
  buildMessage,
  buildUser,
  buildEntityMap,
  buildPeerId,
} from "./entityBuilders";
import { handleUpdate } from "./updateHandler";
import { pause } from "./schedulers";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const CHAT_LIST_LOAD_SLICE = 100;
const TOP_CHAT_MESSAGES_PRELOAD_LIMIT = 20;
const TOP_CHAT_MESSAGES_PRELOAD_INTERVAL = 100; // ms between each preload
const MESSAGE_LIST_SLICE_DESKTOP = 60;
const MESSAGE_LIST_SLICE_MOBILE = 40;
const SYNC_SAFETY_TIMEOUT = 15_000; // 15s safety timeout
const INFINITE_LOOP_MARKER = 100; // max iterations for chat loading

const LOG_PREFIX = "[TelegramSync]";

function getMessageSlice(): number {
  return typeof window !== "undefined" && window.innerWidth < 768
    ? MESSAGE_LIST_SLICE_MOBILE
    : MESSAGE_LIST_SLICE_DESKTOP;
}

// ---------------------------------------------------------------------------
// Sync Service
// ---------------------------------------------------------------------------

class TelegramSyncService {
  private isSyncing = false;
  private isSynced = false;
  private userId: string | null = null;
  private safetyTimeout: ReturnType<typeof setTimeout> | undefined;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private boundProcessUpdate: ((update: any) => void) | null = null;

  /**
   * Start the full sync flow:
   * 1. Get update state (pts/seq/qts/date)
   * 2. Register real-time update handler
   * 3. Load all active chats (100 per batch)
   * 4. After first batch: load messages for open chat + mark UI ready
   * 5. Background: load archived chats + preload top 20 chat messages
   */
  async startSync(userId: string): Promise<void> {
    if (this.isSyncing) {
      console.log(LOG_PREFIX, "Sync already in progress, skipping");
      return;
    }
    if (this.isSynced && this.userId === userId) {
      console.log(LOG_PREFIX, "Already synced for this user, skipping");
      return;
    }

    console.log(LOG_PREFIX, "Starting sync for user", userId);
    this.userId = userId;
    this.isSyncing = true;
    this.isSynced = false;

    store.dispatch(setSyncStatus({ userId, isSyncing: true, isSynced: false }));

    // Safety timeout: release isSyncing if stuck
    this.safetyTimeout = setTimeout(() => {
      if (this.isSyncing) {
        console.warn(
          LOG_PREFIX,
          "Safety timeout reached — releasing sync lock",
        );
        this.isSyncing = false;
        store.dispatch(setSyncStatus({ userId, isSyncing: false }));
      }
    }, SYNC_SAFETY_TIMEOUT);

    try {
      // 1. Initialize update state + register handler
      await this.initUpdateManager();

      // 2. Load all active chats
      await this.loadAllChats("active", async () => {
        // After first batch callback:
        // Load messages for currently open chat
        await this.loadAndReplaceMessages();

        // Mark sync as complete for UI
        this.isSyncing = false;
        this.isSynced = true;
        clearTimeout(this.safetyTimeout);
        store.dispatch(
          setSyncStatus({ userId, isSyncing: false, isSynced: true }),
        );
        console.log(LOG_PREFIX, "Initial sync complete — UI ready");

        // Background tasks (non-blocking)
        this.loadAllChats("archived").catch((e) =>
          console.warn(LOG_PREFIX, "Archived chat load failed:", e),
        );
        this.preloadTopChatMessages().catch((e) =>
          console.warn(LOG_PREFIX, "Top chat preload failed:", e),
        );
      });
    } catch (error) {
      console.error(LOG_PREFIX, "Sync failed:", error);
      this.isSyncing = false;
      clearTimeout(this.safetyTimeout);
      store.dispatch(setSyncStatus({ userId, isSyncing: false }));
    }
  }

  /**
   * Stop sync and clean up resources.
   */
  stopSync(): void {
    console.log(LOG_PREFIX, "Stopping sync");
    clearTimeout(this.safetyTimeout);

    // Remove update handler from client
    if (this.boundProcessUpdate) {
      try {
        mtprotoService
          .getClient()
          .removeEventHandler(this.boundProcessUpdate, new Raw({}));
      } catch {
        // Client may not be available
      }
      this.boundProcessUpdate = null;
    }

    updateManager.destroy();
    this.isSyncing = false;
    this.isSynced = false;
    this.userId = null;
  }

  // -------------------------------------------------------------------------
  // Update Manager initialization
  // -------------------------------------------------------------------------

  private async initUpdateManager(): Promise<void> {
    const client = mtprotoService.getClient();
    const userId = this.userId!;

    // Fetch current update state from Telegram
    const state = await mtprotoService.invoke<Api.updates.State>(
      new Api.updates.GetState(),
    );

    const commonBoxState = {
      seq: state.seq,
      date: state.date,
      pts: state.pts,
      qts: state.qts,
    };

    console.log(LOG_PREFIX, "Update state:", commonBoxState);

    // Store in Redux
    store.dispatch(setCommonBoxState({ userId, commonBoxState }));

    // Initialize the update manager with client + handler
    updateManager.init(client, (update, source) => {
      handleUpdate(update, userId, source);
    });
    updateManager.setInitialState(commonBoxState);

    // Register GramJS event handler
    this.boundProcessUpdate = (update) => {
      updateManager.processUpdate(update);
    };
    client.addEventHandler(this.boundProcessUpdate);

    console.log(
      LOG_PREFIX,
      "Update manager initialized, event handler registered",
    );
  }

  // -------------------------------------------------------------------------
  // Chat loading
  // -------------------------------------------------------------------------

  /**
   * Load all chats in a list (active or archived) via GetDialogs.
   * Fetches in 100-chat batches until all loaded.
   *
   * @param listType - 'active' or 'archived'
   * @param whenFirstBatchDone - callback fired after the first batch completes
   */
  private async loadAllChats(
    listType: "active" | "archived",
    whenFirstBatchDone?: () => Promise<void>,
  ): Promise<void> {
    const userId = this.userId!;
    const isArchived = listType === "archived";
    let isFirstBatch = true;
    let offsetDate = 0;
    let offsetId = 0;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    let offsetPeer: any = new Api.InputPeerEmpty();
    let iterations = 0;

    console.log(LOG_PREFIX, `Loading ${listType} chats...`);

    // Also fetch pinned dialogs on first active batch
    if (!isArchived) {
      try {
        const pinned = await mtprotoService.invoke<Api.messages.PeerDialogs>(
          new Api.messages.GetPinnedDialogs({ folderId: 0 }),
        );
        if (pinned?.dialogs?.length) {
          this.processDialogsResult(pinned, userId, true);
        }
      } catch (e) {
        console.warn(LOG_PREFIX, "Failed to fetch pinned dialogs:", e);
      }
    }

    while (iterations < INFINITE_LOOP_MARKER) {
      iterations++;

      const result = await mtprotoService.invoke<Api.messages.TypeDialogs>(
        new Api.messages.GetDialogs({
          offsetDate,
          offsetId,
          offsetPeer,
          limit: CHAT_LIST_LOAD_SLICE,
          folderId: isArchived ? 1 : 0,
          hash: bigInt(0),
        }),
      );

      // Handle different response types
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const dialogs: any[] = (result as any).dialogs ?? [];
      if (dialogs.length === 0) {
        console.log(
          LOG_PREFIX,
          `${listType} chats fully loaded (${iterations} batches)`,
        );
        break;
      }

      // Process this batch
      const chatIds = this.processDialogsResult(result, userId, isFirstBatch);

      // Fire first-batch callback
      if (isFirstBatch && whenFirstBatchDone) {
        isFirstBatch = false;
        await whenFirstBatchDone();
      } else {
        isFirstBatch = false;
      }

      // Check if we've loaded everything
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const totalCount = (result as any).count;
      if (totalCount !== undefined && chatIds.length === 0) {
        break;
      }
      if (dialogs.length < CHAT_LIST_LOAD_SLICE) {
        console.log(
          LOG_PREFIX,
          `${listType} chats fully loaded (last batch < slice)`,
        );
        break;
      }

      // Prepare pagination offsets from the last dialog
      const lastDialog = dialogs[dialogs.length - 1];
      const lastPeerId = buildPeerId(lastDialog.peer);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const entityMap = buildEntityMap(result as any);
      const lastEntity = entityMap.get(lastPeerId);

      // Get offset info from the last message in the batch
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const rawMessages: any[] = (result as any).messages ?? [];
      if (rawMessages.length > 0) {
        const lastMsg = rawMessages[rawMessages.length - 1];
        offsetDate = lastMsg.date ?? 0;
        offsetId = lastMsg.id ?? 0;
      }

      // Build offset peer for pagination
      if (lastEntity) {
        const className: string = lastEntity.className ?? "";
        if (className === "User") {
          offsetPeer = new Api.InputPeerUser({
            userId: bigInt(lastPeerId),
            accessHash: bigInt(lastEntity.accessHash ?? 0),
          });
        } else if (className === "Channel") {
          offsetPeer = new Api.InputPeerChannel({
            channelId: bigInt(lastPeerId),
            accessHash: bigInt(lastEntity.accessHash ?? 0),
          });
        } else {
          offsetPeer = new Api.InputPeerChat({
            chatId: bigInt(lastPeerId),
          });
        }
      }

      // No explicit delay between batches — sequential await paces naturally
    }

    if (iterations >= INFINITE_LOOP_MARKER) {
      console.warn(LOG_PREFIX, `${listType} chat loading hit loop guard`);
    }
  }

  /**
   * Process a GetDialogs response — extract chats, users, messages
   * and dispatch to Redux.
   *
   * @returns Array of chat IDs from this batch
   */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private processDialogsResult(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    result: any,
    userId: string,
    isFirstBatch: boolean,
  ): string[] {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const dialogs: any[] = result.dialogs ?? [];
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const rawMessages: any[] = result.messages ?? [];

    // Build entity map (users + chats/channels indexed by ID)
    const entityMap = buildEntityMap(result);

    // Build message map for last-message lookup
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const messageByPeerId = new Map<string, any>();
    for (const msg of rawMessages) {
      if (msg?.peerId) {
        const peerId = buildPeerId(msg.peerId);
        if (peerId) {
          // Keep the most recent message per peer
          const existing = messageByPeerId.get(peerId);
          if (!existing || (msg.date && msg.date > (existing.date ?? 0))) {
            messageByPeerId.set(peerId, msg);
          }
        }
      }
    }

    // Build chats
    const chats: Record<string, TelegramChat> = {};
    const chatOrder: string[] = [];

    for (const dialog of dialogs) {
      const peerId = buildPeerId(dialog.peer);
      if (!peerId) continue;

      const entity = entityMap.get(peerId);
      const rawLastMsg = messageByPeerId.get(peerId);
      const lastMsg = rawLastMsg ? buildMessage(rawLastMsg, peerId) : undefined;
      const chat = buildChat(dialog, entity, lastMsg ?? undefined);

      chats[chat.id] = chat;
      chatOrder.push(chat.id);
    }

    // Build users
    const users: Record<string, TelegramUser> = {};
    const rawUsers: unknown[] = result.users ?? [];
    for (const raw of rawUsers) {
      const u = buildUser(raw);
      if (u) users[u.id] = u;
    }

    // Store channel PTS for update tracking
    for (const dialog of dialogs) {
      if (dialog.pts && dialog.peer?.channelId) {
        const channelId = String(dialog.peer.channelId);
        store.dispatch(setChannelPts({ userId, channelId, pts: dialog.pts }));
        updateManager.setChannelPts(channelId, dialog.pts);
      }
    }

    // Dispatch to Redux
    if (isFirstBatch) {
      store.dispatch(replaceChats({ userId, chats, chatsOrder: chatOrder }));
      store.dispatch(setUsers({ userId, users }));
    } else {
      store.dispatch(addChats({ userId, chats, appendOrder: chatOrder }));
      store.dispatch(addUsers({ userId, users }));
    }

    return chatOrder;
  }

  // -------------------------------------------------------------------------
  // Message loading
  // -------------------------------------------------------------------------

  /**
   * Load messages for the currently open chat.
   * Uses the same slice sizes (60 desktop / 40 mobile).
   */
  private async loadAndReplaceMessages(): Promise<void> {
    const userId = this.userId!;
    const state = store.getState();
    const userState = state.telegram.byUser[userId];
    const selectedChatId = userState?.selectedChatId;

    if (!selectedChatId) {
      console.log(LOG_PREFIX, "No chat selected, skipping message load");
      return;
    }

    const slice = getMessageSlice();

    try {
      console.log(
        LOG_PREFIX,
        `Loading ${slice} messages for chat ${selectedChatId}`,
      );

      const result = await mtprotoService.invoke(
        new Api.messages.GetHistory({
          peer: selectedChatId,
          offsetId: 0,
          addOffset: 0,
          limit: slice,
        }),
      );

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const rawMessages: any[] = (result as any)?.messages ?? [];
      const messages = rawMessages
        .map((m) => buildMessage(m, selectedChatId))
        .filter(Boolean) as NonNullable<ReturnType<typeof buildMessage>>[];

      if (messages.length > 0) {
        store.dispatch(
          addChatMessagesById({ userId, chatId: selectedChatId, messages }),
        );
      }

      console.log(
        LOG_PREFIX,
        `Loaded ${messages.length} messages for chat ${selectedChatId}`,
      );
    } catch (error) {
      console.warn(
        LOG_PREFIX,
        `Failed to load messages for chat ${selectedChatId}:`,
        error,
      );
    }
  }

  /**
   * Background preload: fetch messages for the top 20 chats.
   * 100ms pause between each to avoid rate limits.
   */
  private async preloadTopChatMessages(): Promise<void> {
    const userId = this.userId!;
    const state = store.getState();
    const userState = state.telegram.byUser[userId];
    if (!userState) return;

    const chatsOrder = userState.chatsOrder;
    const selectedChatId = userState.selectedChatId;
    const slice = getMessageSlice();

    let preloaded = 0;

    for (const chatId of chatsOrder) {
      if (preloaded >= TOP_CHAT_MESSAGES_PRELOAD_LIMIT) break;

      // Skip the already-loaded selected chat
      if (chatId === selectedChatId) continue;

      // Skip if we already have messages for this chat
      if (
        userState.messages[chatId] &&
        Object.keys(userState.messages[chatId]).length > 0
      ) {
        continue;
      }

      await pause(TOP_CHAT_MESSAGES_PRELOAD_INTERVAL);

      try {
        const result = await mtprotoService.invoke(
          new Api.messages.GetHistory({
            peer: chatId,
            offsetId: 0,
            addOffset: 0,
            limit: slice,
          }),
        );

        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const rawMessages: any[] = (result as any)?.messages ?? [];
        const messages = rawMessages
          .map((m) => buildMessage(m, chatId))
          .filter(Boolean) as NonNullable<ReturnType<typeof buildMessage>>[];

        if (messages.length > 0) {
          store.dispatch(addChatMessagesById({ userId, chatId, messages }));
        }

        preloaded++;
      } catch (error) {
        console.warn(
          LOG_PREFIX,
          `Failed to preload messages for chat ${chatId}:`,
          error,
        );
        // Continue with next chat
      }
    }

    console.log(LOG_PREFIX, `Preloaded messages for ${preloaded} chats`);
  }
}

export const telegramSyncService = new TelegramSyncService();
