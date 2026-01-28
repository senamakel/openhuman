import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import { optNumber } from '../args';
import type { ApiUser } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'search_contacts',
  description: 'Search contacts by name or username',
  inputSchema: {
    type: 'object',
    properties: {
      query: { type: 'string', description: 'Search query' },
      limit: { type: 'number', description: 'Max results', default: 20 },
    },
    required: ['query'],
  },
};

export async function searchContacts(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const query = typeof args.query === 'string' ? args.query : '';
    if (!query) {
      return { content: [{ type: 'text', text: 'query is required' }], isError: true };
    }
    const limit = optNumber(args, 'limit', 20);
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.contacts.Search({ q: query, limit }));
    });

    if (!result || !('users' in result) || !Array.isArray(result.users) || result.users.length === 0) {
      return { content: [{ type: 'text', text: `No contacts found for "${query}".` }] };
    }

    const lines = result.users.map((u: ApiUser) => {
      const name = [u.firstName, u.lastName].filter(Boolean).join(' ') || 'Unknown';
      const username = u.username ? `@${u.username}` : '';
      return `ID: ${u.id} | ${name} ${username}`.trim();
    });

    return { content: [{ type: 'text', text: lines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'search_contacts',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.CONTACT,
    );
  }
}
