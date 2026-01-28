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
  name: "get_participants",
  description: "Get participants of a group or channel",
  inputSchema: {
    type: "object",
    properties: {
      chat_id: { type: "string", description: "Chat ID or username" },
      limit: { type: "number", description: "Max results", default: 50 },
    },
    required: ["chat_id"],
  },
};

export async function getParticipants(
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

    const client = mtprotoService.getClient();
    const entity = chat.username ? chat.username : chat.id;

    let participants: ApiUser[] = [];

    if (chat.type === "channel" || chat.type === "supergroup") {
      const result = await mtprotoService.withFloodWaitHandling(async () => {
        const inputChannel = await client.getInputEntity(entity);
        return client.invoke(
          new Api.channels.GetParticipants({
            channel: inputChannel as unknown as Api.TypeInputChannel,
            filter: new Api.ChannelParticipantsRecent(),
            offset: 0,
            limit,
            hash: bigInt(0),
          }),
        );
      });
      if (result && "users" in result && Array.isArray(result.users)) {
        participants = result.users as unknown as ApiUser[];
      }
    } else {
      const result = await mtprotoService.withFloodWaitHandling(async () => {
        return client.invoke(
          new Api.messages.GetFullChat({ chatId: bigInt(chat.id) }),
        );
      });
      if (result && "users" in result && Array.isArray(result.users)) {
        participants = result.users as unknown as ApiUser[];
      }
    }

    if (participants.length === 0) {
      return { content: [{ type: "text", text: "No participants found." }] };
    }

    const lines = participants.map((u: ApiUser) => {
      const name =
        [u.firstName, u.lastName].filter(Boolean).join(" ") || "Unknown";
      const username = u.username ? `@${u.username}` : "";
      return `ID: ${u.id} | ${name} ${username}`.trim();
    });

    return {
      content: [
        {
          type: "text",
          text: `${lines.length} participants:\n${lines.join("\n")}`,
        },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "get_participants",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
