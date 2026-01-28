import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { getChatById, getMessages, formatMessage } from '../telegramApi';
import { validateId } from '../../validation';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import bigInt from 'big-integer';
import type { ApiMessage } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'get_pinned_messages',
  description: 'Get pinned messages from a chat',
  inputSchema: {
    type: 'object',
    properties: { chat_id: { type: 'string', description: 'Chat ID or username' } },
    required: ['chat_id'],
  },
};

export async function getPinnedMessages(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const chatId = validateId(args.chat_id, 'chat_id');

    const chat = getChatById(chatId);
    if (!chat) {
      return { content: [{ type: 'text', text: `Chat not found: ${chatId}` }], isError: true };
    }

    const entity = chat.username ? chat.username : chat.id;
    const client = mtprotoService.getClient();

    let pinnedLines: string[] = [];

    try {
      const inputPeer = await client.getInputEntity(entity);
      const result = await client.invoke(
        new Api.messages.Search({
          peer: inputPeer,
          q: '',
          filter: new Api.InputMessagesFilterPinned(),
          minDate: 0,
          maxDate: 0,
          offsetId: 0,
          addOffset: 0,
          limit: 50,
          maxId: 0,
          minId: 0,
          hash: bigInt(0),
        }),
      );

      if ('messages' in result && Array.isArray(result.messages)) {
        pinnedLines = (result.messages as unknown as ApiMessage[]).map((msg) => {
          const id = msg.id ?? '?';
          const text = msg.message ?? '[Media/No text]';
          const date = msg.date ? new Date(msg.date * 1000).toISOString() : 'unknown';
          return `ID: ${id} | Date: ${date} | ${text}`;
        });
      }
    } catch {
      // Fallback: check cached messages for pinned flag
      const allMessages = await getMessages(chatId, 500, 0);
      if (allMessages) {
        const pinned = allMessages.filter((m) => (m as unknown as ApiMessage).pinned);
        pinnedLines = pinned.map((msg) => {
          const f = formatMessage(msg);
          return `ID: ${f.id} | Date: ${f.date} | ${f.text || '[Media/No text]'}`;
        });
      }
    }

    if (pinnedLines.length === 0) {
      return { content: [{ type: 'text', text: 'No pinned messages found.' }] };
    }

    return { content: [{ type: 'text', text: pinnedLines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'get_pinned_messages',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.MSG,
    );
  }
}
