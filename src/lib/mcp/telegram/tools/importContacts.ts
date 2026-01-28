import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from "../../errorHandler";
import { mtprotoService } from "../../../../services/mtprotoService";
import { Api } from "telegram";
import bigInt from "big-integer";
import type { ContactInput, ImportContactsResult } from "../apiResultTypes";

export const tool: MCPTool = {
  name: "import_contacts",
  description: "Import contacts to Telegram",
  inputSchema: {
    type: "object",
    properties: {
      contacts: {
        type: "array",
        description: "Array of contacts: [{phone, first_name, last_name?}]",
        items: {
          type: "object",
          properties: {
            phone: { type: "string" },
            first_name: { type: "string" },
            last_name: { type: "string" },
          },
          required: ["phone", "first_name"],
        },
      },
    },
    required: ["contacts"],
  },
};

export async function importContacts(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const contactsArg = args.contacts;
    if (!Array.isArray(contactsArg) || contactsArg.length === 0) {
      return {
        content: [
          {
            type: "text",
            text: "contacts array is required and must not be empty.",
          },
        ],
        isError: true,
      };
    }

    const inputContacts = contactsArg.map(
      (c: ContactInput, i: number) =>
        new Api.InputPhoneContact({
          clientId: bigInt(i),
          phone: String(c.phone ?? ""),
          firstName: String(c.first_name ?? ""),
          lastName: String(c.last_name ?? ""),
        }),
    );

    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(
        new Api.contacts.ImportContacts({ contacts: inputContacts }),
      );
    });

    const imported = (result as unknown as ImportContactsResult)?.imported?.length ?? 0;
    return {
      content: [
        {
          type: "text",
          text: `Imported ${imported} of ${contactsArg.length} contacts.`,
        },
      ],
    };
  } catch (error) {
    return logAndFormatError(
      "import_contacts",
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.CONTACT,
    );
  }
}
