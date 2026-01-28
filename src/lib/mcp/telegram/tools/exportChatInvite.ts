import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { validateId } from '../../validation';
import { getChatById } from '../telegramApi';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import { optString } from '../args';
import type { ChatInviteResult } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'export_chat_invite',
  description: 'Export a new chat invite link',
  inputSchema: {
    type: 'object',
    properties: {
      chat_id: { type: 'string', description: 'Chat ID or username' },
      title: { type: 'string', description: 'Link title' },
      expire_date: { type: 'number', description: 'Expiration timestamp' },
      usage_limit: { type: 'number', description: 'Max number of uses' },
    },
    required: ['chat_id'],
  },
};

export async function exportChatInvite(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const chatId = validateId(args.chat_id, 'chat_id');
    const title = optString(args, 'title');
    const expireDate = typeof args.expire_date === 'number' ? args.expire_date : undefined;
    const usageLimit = typeof args.usage_limit === 'number' ? args.usage_limit : undefined;

    const chat = getChatById(chatId);
    if (!chat) return { content: [{ type: 'text', text: `Chat not found: ${chatId}` }], isError: true };

    const client = mtprotoService.getClient();
    const entity = chat.username ? chat.username : chat.id;

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      const inputPeer = await client.getInputEntity(entity);
      return client.invoke(
        new Api.messages.ExportChatInvite({
          peer: inputPeer,
          title: title ?? undefined,
          expireDate: expireDate ?? undefined,
          usageLimit: usageLimit ?? undefined,
        }),
      );
    });

    const link = (result as unknown as ChatInviteResult)?.link;
    if (!link) {
      return { content: [{ type: 'text', text: 'Could not create invite link.' }], isError: true };
    }

    return { content: [{ type: 'text', text: `Invite link created: ${link}` }] };
  } catch (error) {
    return logAndFormatError(
      'export_chat_invite',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.GROUP,
    );
  }
}
