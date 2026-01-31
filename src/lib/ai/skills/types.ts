import type { MemoryManager } from "../memory/manager";
import type { SessionManager } from "../sessions/manager";
import type { ToolRegistry } from "../tools/registry";
import type { AITool } from "../tools/registry";
import type { EntityManager } from "../entities/manager";

/** Parsed skill entry (backward-compatible with prompt-only skills) */
export interface SkillEntry {
  /** Skill name from frontmatter */
  name: string;
  /** Description from frontmatter */
  description: string;
  /** File system path to the skill directory */
  location?: string;
  /** Full SKILL.md content */
  content: string;
  /** Whether the skill is installed locally */
  installed: boolean;
  /** Skill source (local, repo, bundled) */
  source: "local" | "repo" | "bundled";
  /** TypeScript skill definition (if skill.ts exists) */
  definition?: SkillDefinition;
}

/** SKILL.md frontmatter metadata */
export interface SkillFrontmatter {
  name: string;
  description: string;
  /** Optional metadata section */
  metadata?: {
    alphahuman?: {
      /** Emoji icon for the skill */
      emoji?: string;
      /** Required binaries */
      requires?: { bins?: string[] };
      /** Installation instructions */
      install?: Array<{
        id: string;
        kind: string;
        package: string;
        bins?: string[];
      }>;
    };
  };
}

/** Skill directory structure */
export interface SkillDirectory {
  /** Path to the skill directory */
  path: string;
  /** Has a valid SKILL.md */
  hasSkillFile: boolean;
  /** Has a skill.ts entry point */
  hasSkillTs: boolean;
  /** Has scripts directory */
  hasScripts: boolean;
  /** Has references directory */
  hasReferences: boolean;
  /** Has assets directory */
  hasAssets: boolean;
}

/** Skill registry snapshot for session persistence */
export interface SkillSnapshot {
  /** Formatted prompt text */
  prompt: string;
  /** Loaded skills with basic info */
  skills: Array<{ name: string; hasDefinition: boolean }>;
  /** Snapshot version for cache invalidation */
  version: number;
}

// --- Skill Definition System ---

/** Context passed to every lifecycle hook */
export interface SkillContext {
  /** Memory manager for reading/writing memory files */
  memory: MemoryManager;
  /** Session manager for current session */
  session: SessionManager;
  /** Tool registry to register custom tools */
  tools: ToolRegistry;
  /** Entity manager for querying the platform graph */
  entities: EntityManager;
  /** Skill's own storage directory path (relative): skills/{name}/data/ */
  dataDir: string;
  /** Read a file from the skill's data directory */
  readData(filename: string): Promise<string>;
  /** Write a file to the skill's data directory */
  writeData(filename: string, content: string): Promise<void>;
  /** Log a message to the skill's log */
  log(message: string): void;
}

/** Lifecycle hooks that a skill can implement */
export interface SkillHooks {
  /** Called when skill is loaded at startup */
  onLoad?(ctx: SkillContext): Promise<void>;

  /** Called when skill is unloaded (app shutdown) */
  onUnload?(ctx: SkillContext): Promise<void>;

  /** Called when a new session starts */
  onSessionStart?(ctx: SkillContext, sessionId: string): Promise<void>;

  /** Called when a session ends */
  onSessionEnd?(ctx: SkillContext, sessionId: string): Promise<void>;

  /** Called before the AI processes a user message */
  onBeforeMessage?(ctx: SkillContext, message: string): Promise<string | void>;

  /** Called after the AI generates a response */
  onAfterResponse?(ctx: SkillContext, response: string): Promise<string | void>;

  /** Called before memory compaction (memory flush) */
  onMemoryFlush?(ctx: SkillContext): Promise<void>;

  /** Called on a schedule (e.g., every N minutes while active) */
  onTick?(ctx: SkillContext): Promise<void>;
}

/** What each skill.ts exports */
export interface SkillDefinition {
  name: string;
  description: string;
  version: string;

  /** Lifecycle hooks */
  hooks: SkillHooks;

  /** Custom tools this skill registers */
  tools?: AITool[];

  /** Tick interval in ms (default: no tick) */
  tickInterval?: number;
}
