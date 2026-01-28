import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import type { ResultWithChats } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'join_chat_by_link',
  description: 'Join a chat using an invite link',
  inputSchema: {
    type: 'object',
    properties: {
      link: { type: 'string', description: 'Invite link (e.g. https://t.me/+HASH or https://t.me/joinchat/HASH)' },
    },
    required: ['link'],
  },
};

export async function joinChatByLink(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const link = typeof args.link === 'string' ? args.link : '';
    if (!link) return { content: [{ type: 'text', text: 'link is required' }], isError: true };

    // Extract hash from link
    let hash = link;
    const plusMatch = link.match(/t\.me\/\+(.+)/);
    const joinMatch = link.match(/t\.me\/joinchat\/(.+)/);
    if (plusMatch) hash = plusMatch[1];
    else if (joinMatch) hash = joinMatch[1];

    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      return client.invoke(new Api.messages.ImportChatInvite({ hash }));
    });

    const chatTitle = (result as unknown as ResultWithChats)?.chats?.[0]?.title ?? 'unknown';
    return { content: [{ type: 'text', text: `Joined chat: ${chatTitle}` }] };
  } catch (error) {
    return logAndFormatError(
      'join_chat_by_link',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
