import type { MCPTool, MCPToolResult } from "../../types";
import type { TelegramMCPContext } from "../types";
import { ErrorCategory, logAndFormatError } from '../../errorHandler';
import { mtprotoService } from '../../../../services/mtprotoService';
import { Api } from 'telegram';
import { optNumber } from '../args';
import type { InlineBotResults } from '../apiResultTypes';

export const tool: MCPTool = {
  name: "get_gif_search",
  description: "Search GIFs",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string", description: "Search query" },
      limit: { type: 'number', description: 'Max results', default: 10 },
    },
    required: ["query"],
  },
};

export async function getGifSearch(
  args: Record<string, unknown>,
  _context: TelegramMCPContext,
): Promise<MCPToolResult> {
  try {
    const query = typeof args.query === 'string' ? args.query : '';
    if (!query) return { content: [{ type: 'text', text: 'query is required' }], isError: true };
    const limit = optNumber(args, 'limit', 10);

    const client = mtprotoService.getClient();

    const result = await mtprotoService.withFloodWaitHandling(async () => {
      const bot = await client.getInputEntity('gif');
      return client.invoke(
        new Api.messages.GetInlineBotResults({
          bot: bot as unknown as Api.TypeInputUser,
          peer: new Api.InputPeerSelf(),
          query,
          offset: '',
        }),
      );
    });

    const results = (result as unknown as InlineBotResults)?.results;
    if (!results || !Array.isArray(results) || results.length === 0) {
      return { content: [{ type: 'text', text: 'No GIFs found for: ' + query }] };
    }

    const lines = results.slice(0, limit).map((r, i: number) => {
      const title = r.title ?? r.description ?? 'GIF ' + (i + 1);
      return (i + 1) + '. ' + title;
    });

    return { content: [{ type: 'text', text: lines.length + ' GIFs found:\n' + lines.join('\n') }] };
  } catch (error) {
    return logAndFormatError(
      'get_gif_search',
      error instanceof Error ? error : new Error(String(error)),
      ErrorCategory.MEDIA,
    );
  }
}
