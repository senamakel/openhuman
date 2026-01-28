import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import bigInt from 'big-integer';
import type { ApiUser } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'list_contacts',
  description: 'List all contacts in your Telegram account',
  inputSchema: { type: 'object', properties: {} },
};

export async function listContacts(
  _args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.contacts.GetContacts({ hash: bigInt(0) }));
    });

    if (!result || !('users' in result) || !Array.isArray(result.users)) {
      return { content: [{ type: 'text', text: 'No contacts found.' }] };
    }

    const lines = result.users.map((u: ApiUser) => {
      const name = [u.firstName, u.lastName].filter(Boolean).join(' ') || 'Unknown';
      const username = u.username ? `@${u.username}` : '';
      const phone = u.phone ? `+${u.phone}` : '';
      return `ID: ${u.id} | ${name} ${username} ${phone}`.trim();
    });

    if (lines.length === 0) {
      return { content: [{ type: 'text', text: 'No contacts found.' }] };
    }

    return { content: [{ type: 'text', text: lines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'list_contacts',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.CONTACT,
    );
  }
}
