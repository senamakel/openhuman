import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import { optNumber } from '../args';
import type { ApiUser } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'get_blocked_users',
  description: 'Get list of blocked users',
  inputSchema: {
    type: 'object',
    properties: {
      limit: { type: 'number', description: 'Max results', default: 50 },
    },
  },
};

export async function getBlockedUsers(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const limit = optNumber(args, 'limit', 50);
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(
        new Api.contacts.GetBlocked({ offset: 0, limit }),
      );
    });

    if (!result || !('users' in result) || !Array.isArray(result.users) || result.users.length === 0) {
      return { content: [{ type: 'text', text: 'No blocked users.' }] };
    }

    const lines = result.users.map((u: ApiUser) => {
      const name = [u.firstName, u.lastName].filter(Boolean).join(' ') || 'Unknown';
      const username = u.username ? `@${u.username}` : '';
      return `ID: ${u.id} | ${name} ${username}`.trim();
    });

    return { content: [{ type: 'text', text: lines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'get_blocked_users',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.CONTACT,
    );
  }
}
