import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import bigInt from "big-integer";
import type { ApiUser } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "export_contacts",
  description: "Export all contacts from Telegram",
  inputSchema: { type: "object", properties: {} },
};

export async function exportContacts(
  _args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.contacts.GetContacts({ hash: bigInt(0) }));
    });

    if (
      !result ||
      !("users" in result) ||
      !Array.isArray(result.users) ||
      result.users.length === 0
    ) {
      return { content: [{ type: "text", text: "No contacts to export." }] };
    }

    const contacts = result.users.map((u: ApiUser) => ({
      id: String(u.id),
      firstName: u.firstName ?? "",
      lastName: u.lastName ?? "",
      username: u.username ?? "",
      phone: u.phone ?? "",
    }));

    return {
      content: [{ type: "text", text: JSON.stringify(contacts, null, 2) }],
    };
  } catch (error) {
    return logAndFormatError(
      "export_contacts",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.CONTACT,
    );
  }
}
