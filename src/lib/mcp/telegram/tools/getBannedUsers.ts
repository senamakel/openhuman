import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { validateId } from "../../validation";
import { getChatById } from "../telegramApi";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import { optNumber } from "../args";
import bigInt from "big-integer";
import type { ApiUser } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "get_banned_users",
  description: "Get banned users in a group or channel",
  inputSchema: {
    type: "object",
    properties: {
      chat_id: { type: "string", description: "Chat ID or username" },
      limit: { type: "number", description: "Max results", default: 50 },
    },
    required: ["chat_id"],
  },
};

export async function getBannedUsers(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const chatId = validateId(args.chat_id, "chat_id");
    const limit = optNumber(args, "limit", 50);

    const chat = getChatById(chatId);
    if (!chat)
      return {
        content: [{ type: "text", text: `Chat not found: ${chatId}` }],
        isError: true,
      };

    if (chat.type !== "channel" && chat.type !== "supergroup") {
      return {
        content: [
          {
            type: "text",
            text: "Banned users list is only available for channels/supergroups.",
          },
        ],
        isError: true,
      };
    }

    const client = mtprotoService.getClient();
    const entity = chat.username ? chat.username : chat.id;

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      const inputChannel = await client.getInputEntity(entity);
      return client.invoke(
        new Api.channels.GetParticipants({
          channel: inputChannel as unknown as Api.TypeInputChannel,
          filter: new Api.ChannelParticipantsKicked({ q: "" }),
          offset: 0,
          limit,
          hash: bigInt(0),
        }),
      );
    });

    if (
      !result ||
      !("users" in result) ||
      !Array.isArray(result.users) ||
      result.users.length === 0
    ) {
      return { content: [{ type: "text", text: "No banned users found." }] };
    }

    const lines = result.users.map((u: ApiUser) => {
      const name =
        [u.firstName, u.lastName].filter(Boolean).join(" ") || "Unknown";
      const username = u.username ? `@${u.username}` : "";
      return `ID: ${u.id} | ${name} ${username}`.trim();
    });

    return {
      content: [
        {
          type: "text",
          text: `${lines.length} banned users:\n${lines.join("\n")}`,
        },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "get_banned_users",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.ADMIN,
    );
  }
}
