import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import type { ResultWithChats } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "import_chat_invite",
  description: "Join a chat using an invite hash",
  inputSchema: {
    type: "object",
    properties: {
      hash: {
        type: "string",
        description: "Invite hash (from t.me/+HASH or t.me/joinchat/HASH)",
      },
    },
    required: ["hash"],
  },
};

export async function importChatInvite(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const hash = typeof args.hash === "string" ? args.hash : "";
    if (!hash)
      return {
        content: [{ type: "text", text: "hash is required" }],
        isError: true,
      };

    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.messages.ImportChatInvite({ hash }));
    });

    const chatTitle = (result as unknown as ResultWithChats)?.chats?.[0]?.title ?? "unknown";
    return { content: [{ type: "text", text: `Joined chat: ${chatTitle}` }] };
  } catch (error) {
    return logAndFormatError(
      "import_chat_invite",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
