import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { validateId } from "../../validation";
import { getChatById } from "../telegramApi";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import type { ChatInviteResult } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "get_invite_link",
  description: "Get the invite link for a chat",
  inputSchema: {
    type: "object",
    properties: {
      chat_id: { type: "string", description: "Chat ID or username" },
    },
    required: ["chat_id"],
  },
};

export async function getInviteLink(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const chatId = validateId(args.chat_id, "chat_id");

    const chat = getChatById(chatId);
    if (!chat)
      return {
        content: [{ type: "text", text: `Chat not found: ${chatId}` }],
        isError: true,
      };

    const client = mtprotoService.getClient();
    const entity = chat.username ? chat.username : chat.id;

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      const inputPeer = await client.getInputEntity(entity);
      return client.invoke(
        new Api.messages.ExportChatInvite({
          peer: inputPeer,
          legacyRevokePermanent: true,
        }),
      );
    });

    const link = (result as unknown as ChatInviteResult)?.link;
    if (!link) {
      return {
        content: [{ type: "text", text: "Could not generate invite link." }],
        isError: true,
      };
    }

    return { content: [{ type: "text", text: `Invite link: ${link}` }] };
  } catch (error) {
    return logAndFormatError(
      "get_invite_link",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
