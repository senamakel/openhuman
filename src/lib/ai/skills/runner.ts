import { invoke } from "@tauri-apps/api/core";
import type {
  SkillContext,
  SkillDefinition,
} from "./types";
import type { MemoryManager } from "../memory/manager";
import type { SessionManager } from "../sessions/manager";
import type { ToolRegistry } from "../tools/registry";
import type { EntityManager } from "../entities/manager";

/** Default timeout for hook execution (10 seconds) */
const HOOK_TIMEOUT_MS = 10_000;

/**
 * Create a SkillContext for a skill.
 */
export function createSkillContext(params: {
  skillName: string;
  memory: MemoryManager;
  session: SessionManager;
  tools: ToolRegistry;
  entities: EntityManager;
}): SkillContext {
  const { skillName, memory, session, tools, entities } = params;
  const dataDir = `skills/${skillName}/data`;

  return {
    memory,
    session,
    tools,
    entities,
    dataDir,

    async readData(filename: string): Promise<string> {
      return invoke<string>("ai_read_memory_file", {
        relativePath: `${dataDir}/${filename}`,
      });
    },

    async writeData(filename: string, content: string): Promise<void> {
      await invoke("ai_write_memory_file", {
        relativePath: `${dataDir}/${filename}`,
        content,
      });
    },

    log(message: string): void {
      console.log(`[skill:${skillName}] ${message}`);
    },
  };
}

/**
 * Execute a lifecycle hook safely with timeout and error catching.
 * Returns the result of the hook, or undefined if the hook threw or timed out.
 */
export async function runHook(
  definition: SkillDefinition,
  hookName: string,
  ctx: SkillContext,
  ...args: unknown[]
): Promise<unknown> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const hook = (definition.hooks as any)[hookName];
  if (typeof hook !== "function") return undefined;

  try {
    const result = await Promise.race([
      hook(ctx, ...args),
      new Promise((_, reject) =>
        setTimeout(
          () => reject(new Error(`Hook ${hookName} timed out after ${HOOK_TIMEOUT_MS}ms`)),
          HOOK_TIMEOUT_MS,
        ),
      ),
    ]);
    return result;
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error);
    ctx.log(`Hook ${hookName} failed: ${msg}`);
    return undefined;
  }
}

/**
 * Run onBeforeMessage on all skills, allowing each to transform the message.
 */
export async function runBeforeMessage(
  skills: Array<{ definition: SkillDefinition; context: SkillContext }>,
  message: string,
): Promise<string> {
  let result = message;
  for (const { definition, context } of skills) {
    if (definition.hooks.onBeforeMessage) {
      const transformed = await runHook(definition, "onBeforeMessage", context, result);
      if (typeof transformed === "string") {
        result = transformed;
      }
    }
  }
  return result;
}

/**
 * Run onAfterResponse on all skills, allowing each to transform the response.
 */
export async function runAfterResponse(
  skills: Array<{ definition: SkillDefinition; context: SkillContext }>,
  response: string,
): Promise<string> {
  let result = response;
  for (const { definition, context } of skills) {
    if (definition.hooks.onAfterResponse) {
      const transformed = await runHook(definition, "onAfterResponse", context, result);
      if (typeof transformed === "string") {
        result = transformed;
      }
    }
  }
  return result;
}
