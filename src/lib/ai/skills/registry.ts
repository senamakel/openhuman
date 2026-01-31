import type {
  SkillEntry,
  SkillSnapshot,
  SkillContext,
  SkillDefinition,
} from "./types";
import { loadSkills } from "./loader";
import { buildSkillsSection } from "../prompts/sections/skills";
import { createSkillContext, runHook, runBeforeMessage, runAfterResponse } from "./runner";
import type { MemoryManager } from "../memory/manager";
import type { SessionManager } from "../sessions/manager";
import type { ToolRegistry } from "../tools/registry";
import type { EntityManager } from "../entities/manager";

interface ActiveSkill {
  entry: SkillEntry;
  definition: SkillDefinition;
  context: SkillContext;
  tickTimer?: ReturnType<typeof setInterval>;
}

/**
 * Skill registry manages loaded skills and their lifecycles.
 *
 * Handles:
 * - Loading skills from directories (SKILL.md + optional skill.ts)
 * - Calling lifecycle hooks (onLoad, onUnload, onSessionStart, etc.)
 * - Managing tick timers for skills with tickInterval
 * - Registering custom tools from skills
 * - Providing skills to the prompt system
 */
export class SkillRegistry {
  private skills: SkillEntry[] = [];
  private activeSkills: ActiveSkill[] = [];
  private version = 0;
  private managers: {
    memory?: MemoryManager;
    session?: SessionManager;
    tools?: ToolRegistry;
    entities?: EntityManager;
  } = {};

  /** Set the managers used to create SkillContext for skills */
  setManagers(params: {
    memory: MemoryManager;
    session: SessionManager;
    tools: ToolRegistry;
    entities: EntityManager;
  }): void {
    this.managers = params;
  }

  /** Load all skills from configured directories */
  async reload(dirs?: string[]): Promise<void> {
    // Unload previous active skills
    await this.unloadAll();

    this.skills = await loadSkills(dirs);
    this.version++;

    // Activate skills that have definitions
    await this.activateSkills();
  }

  /** Activate skills with TypeScript definitions */
  private async activateSkills(): Promise<void> {
    const { memory, session, tools, entities } = this.managers;
    if (!memory || !session || !tools || !entities) return;

    for (const entry of this.skills) {
      const def = entry.definition;
      if (!def) continue;

      const context = createSkillContext({
        skillName: entry.name,
        memory,
        session,
        tools,
        entities,
      });

      const active: ActiveSkill = {
        entry,
        definition: def,
        context,
      };

      // Register custom tools
      if (def.tools) {
        for (const tool of def.tools) {
          tools.register(tool);
        }
      }

      // Call onLoad hook
      await runHook(def, "onLoad", context);

      // Start tick timer if configured
      if (def.tickInterval && def.hooks.onTick) {
        active.tickTimer = setInterval(async () => {
          await runHook(def, "onTick", context);
        }, def.tickInterval);
      }

      this.activeSkills.push(active);
    }
  }

  /** Unload all active skills (call onUnload, clear timers) */
  async unloadAll(): Promise<void> {
    for (const active of this.activeSkills) {
      // Clear tick timer
      if (active.tickTimer) {
        clearInterval(active.tickTimer);
      }

      // Call onUnload hook
      await runHook(active.definition, "onUnload", active.context);

      // Unregister custom tools
      if (active.definition.tools) {
        const { tools } = this.managers;
        if (tools) {
          for (const tool of active.definition.tools) {
            tools.unregister(tool.definition.name);
          }
        }
      }
    }
    this.activeSkills = [];
  }

  /** Notify all active skills of a new session */
  async onSessionStart(sessionId: string): Promise<void> {
    for (const active of this.activeSkills) {
      await runHook(
        active.definition,
        "onSessionStart",
        active.context,
        sessionId,
      );
    }
  }

  /** Notify all active skills of session end */
  async onSessionEnd(sessionId: string): Promise<void> {
    for (const active of this.activeSkills) {
      await runHook(
        active.definition,
        "onSessionEnd",
        active.context,
        sessionId,
      );
    }
  }

  /** Run onBeforeMessage on all active skills, allowing message transformation */
  async onBeforeMessage(message: string): Promise<string> {
    return runBeforeMessage(
      this.activeSkills.map((a) => ({
        definition: a.definition,
        context: a.context,
      })),
      message,
    );
  }

  /** Run onAfterResponse on all active skills, allowing response transformation */
  async onAfterResponse(response: string): Promise<string> {
    return runAfterResponse(
      this.activeSkills.map((a) => ({
        definition: a.definition,
        context: a.context,
      })),
      response,
    );
  }

  /** Notify all active skills of memory flush */
  async onMemoryFlush(): Promise<void> {
    for (const active of this.activeSkills) {
      await runHook(active.definition, "onMemoryFlush", active.context);
    }
  }

  /** Get all loaded skills */
  getSkills(): SkillEntry[] {
    return [...this.skills];
  }

  /** Find a skill by name */
  findSkill(name: string): SkillEntry | undefined {
    return this.skills.find(
      (s) => s.name.toLowerCase() === name.toLowerCase(),
    );
  }

  /** Find skills matching a query (fuzzy name/description match) */
  searchSkills(query: string): SkillEntry[] {
    const lower = query.toLowerCase();
    return this.skills.filter(
      (s) =>
        s.name.toLowerCase().includes(lower) ||
        s.description.toLowerCase().includes(lower),
    );
  }

  /** Generate the skills prompt section */
  buildPromptSection(): string {
    return buildSkillsSection(this.skills);
  }

  /** Create a snapshot for session persistence */
  createSnapshot(): SkillSnapshot {
    return {
      prompt: this.buildPromptSection(),
      skills: this.skills.map((s) => ({
        name: s.name,
        hasDefinition: !!s.definition,
      })),
      version: this.version,
    };
  }

  /** Get count of loaded skills */
  get count(): number {
    return this.skills.length;
  }

  /** Get count of active skills (with TypeScript definitions) */
  get activeCount(): number {
    return this.activeSkills.length;
  }
}
