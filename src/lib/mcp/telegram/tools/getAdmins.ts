import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { validateId } from "../../validation";
import { getChatById } from "../telegramApi";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import bigInt from "big-integer";
import type { ApiUser } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "get_admins",
  description: "Get admins of a group or channel",
  inputSchema: {
    type: "object",
    properties: {
      chat_id: { type: "string", description: "Chat ID or username" },
    },
    required: ["chat_id"],
  },
};

export async function getAdmins(
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

    let admins: ApiUser[] = [];

    if (chat.type === "channel" || chat.type === "supergroup") {
      const result = await mtprotoService.withFloodWaitHandling(async () => {
        const inputChannel = await client.getInputEntity(entity);
        return client.invoke(
          new Api.channels.GetParticipants({
            channel: inputChannel as unknown as Api.TypeInputChannel,
            filter: new Api.ChannelParticipantsAdmins(),
            offset: 0,
            limit: 100,
            hash: bigInt(0),
          }),
        );
      });
      if (result && "users" in result && Array.isArray(result.users)) {
        admins = result.users as unknown as ApiUser[];
      }
    } else {
      const result = await mtprotoService.withFloodWaitHandling(async () => {
        return client.invoke(
          new Api.messages.GetFullChat({ chatId: bigInt(chat.id) }),
        );
      });
      if (result && "users" in result && Array.isArray(result.users)) {
        admins = result.users as unknown as ApiUser[];
      }
    }

    if (admins.length === 0) {
      return { content: [{ type: "text", text: "No admins found." }] };
    }

    const lines = admins.map((u: ApiUser) => {
      const name =
        [u.firstName, u.lastName].filter(Boolean).join(" ") || "Unknown";
      const username = u.username ? `@${u.username}` : "";
      return `ID: ${u.id} | ${name} ${username}`.trim();
    });

    return {
      content: [
        { type: "text", text: `${lines.length} admins:\n${lines.join("\n")}` },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "get_admins",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.ADMIN,
    );
  }
}
