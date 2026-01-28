import type { MCPTool, MCPToolResult } from '../../types';
import type { TelegramMCPContext } from '../types';
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { validateId } from '../../validation';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import bigInt from 'big-integer';
import { optNumber } from '../args';
import type { ApiPhoto } from '../apiResultTypes';

export const tool: MCPTool = {
  name: 'get_user_photos',
  description: 'Get profile photos of a user',
  inputSchema: {
    type: 'object',
    properties: {
      user_id: { type: 'string', description: 'User ID' },
      limit: { type: 'number', description: 'Max photos', default: 10 },
    },
    required: ['user_id'],
  },
};

export async function getUserPhotos(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const userId = validateId(args.user_id, 'user_id');
    const limit = optNumber(args, 'limit', 10);
    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      const inputUser = await client.getInputEntity(userId);
      return client.invoke(
        new Api.photos.GetUserPhotos({
          userId: inputUser as unknown as Api.TypeInputUser,
          offset: 0,
          maxId: bigInt(0),
          limit,
        }),
      );
    });

    if (!result || !('photos' in result) || !Array.isArray(result.photos) || result.photos.length === 0) {
      return { content: [{ type: 'text', text: 'No photos found.' }] };
    }

    const lines = result.photos.map((photo: ApiPhoto, i: number) => {
      const date = photo.date ? new Date(photo.date * 1000).toISOString() : 'unknown';
      return 'Photo ' + (i + 1) + ': ID ' + photo.id + ' | Date: ' + date;
    });

    return { content: [{ type: 'text', text: lines.length + ' photos found:\n' + lines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'get_user_photos',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.PROFILE,
    );
  }
}
