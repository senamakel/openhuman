import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import { optString } from "../args";
import type { ResultWithChats } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "create_channel",
  description: "Create a new channel",
  inputSchema: {
    type: "object",
    properties: {
      title: { type: "string", description: "Channel title" },
      about: { type: "string", description: "Channel description" },
      megagroup: {
        type: "boolean",
        description: "Create as supergroup instead of channel",
        default: false,
      },
    },
    required: ["title"],
  },
};

export async function createChannel(
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

    const about = optString(args, "about") ?? "";
    const megagroup = args.megagroup === true;
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(
        new Api.channels.CreateChannel({
          title,
          about,
          megagroup,
          broadcast: !megagroup,
        }),
      );
    });

    const channelId = (result as unknown as ResultWithChats)?.chats?.[0]?.id ?? "unknown";
    const type = megagroup ? "Supergroup" : "Channel";
    return {
      content: [
        { type: "text", text: `${type} "${title}" created. ID: ${channelId}` },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "create_channel",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
