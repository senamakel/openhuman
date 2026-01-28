import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import type { ResultWithChats } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "create_group",
  description: "Create a new group chat",
  inputSchema: {
    type: "object",
    properties: {
      title: { type: "string", description: "Group title" },
      user_ids: {
        type: "array",
        items: { type: "string" },
        description: "User IDs to add",
      },
    },
    required: ["title", "user_ids"],
  },
};

export async function createGroup(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const title = typeof args.title === "string" ? args.title : "";
    if (!title)
      return {
        content: [{ type: "text", text: "title is required" }],
        isError: true,
      };

    const userIds = Array.isArray(args.user_ids) ? args.user_ids : [];
    if (userIds.length === 0)
      return {
        content: [{ type: "text", text: "user_ids must not be empty" }],
        isError: true,
      };

    const client = mtprotoService.getClient();

    const users: Api.TypeInputUser[] = [];
    for (const uid of userIds) {
      const inputUser = await client.getInputEntity(String(uid));
      users.push(inputUser as unknown as Api.TypeInputUser);
    }

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.messages.CreateChat({ title, users }));
    });

    const chatId = (result as unknown as ResultWithChats)?.chats?.[0]?.id ?? "unknown";
    return {
      content: [
        { type: "text", text: `Group "${title}" created. Chat ID: ${chatId}` },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "create_group",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
