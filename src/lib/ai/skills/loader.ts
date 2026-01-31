import { invoke } from "@tauri-apps/api/core";
import type { SkillEntry, SkillDefinition } from "./types";
import { parseFrontmatter } from "./frontmatter";
import { MEMORY_PATHS } from "../memory/types";

/**
 * Load skills from one or more directories.
 * Each skill is a directory containing a SKILL.md file and optionally a skill.ts.
 */
export async function loadSkills(
  dirs: string[] = [MEMORY_PATHS.SKILLS_DIR],
): Promise<SkillEntry[]> {
  const skills: SkillEntry[] = [];

  for (const dir of dirs) {
    try {
      const entries = await invoke<string[]>("ai_list_memory_files", {
        relativeDir: dir,
      });

      // Look for SKILL.md files directly, or subdirectories containing them
      for (const entry of entries) {
        if (entry === "SKILL.md") {
          // Skill file directly in the dir
          const skill = await loadSkillFromDir(dir);
          if (skill) skills.push(skill);
        }
      }

      // Also check subdirectories
      const potentialDirs = entries.filter((e) => !e.includes("."));
      for (const subdir of potentialDirs) {
        const skillPath = `${dir}/${subdir}`;
        const skill = await loadSkillFromDir(skillPath);
        if (skill) skills.push(skill);
      }
    } catch {
      // Directory doesn't exist yet
    }
  }

  return skills;
}

/**
 * Load a single skill from a directory.
 * Reads SKILL.md for prompt content, and optionally loads skill.ts definition.
 */
async function loadSkillFromDir(
  dirPath: string,
): Promise<SkillEntry | null> {
  try {
    const content = await invoke<string>("ai_read_memory_file", {
      relativePath: `${dirPath}/SKILL.md`,
    });

    const { frontmatter } = parseFrontmatter(content);

    // Try to load skill.ts definition
    const definition = await loadSkillDefinition(dirPath);

    return {
      name: frontmatter.name,
      description: frontmatter.description,
      location: dirPath,
      content,
      installed: true,
      source: "local",
      definition,
    };
  } catch {
    return null;
  }
}

/**
 * Try to load a skill.ts definition from a skill directory.
 * Returns null if no skill.ts exists or it fails to load.
 *
 * Skill definitions are read as JSON content from the skill's data.
 * In a full implementation, these would be dynamically imported.
 * For now, we read the skill.ts content and parse the exported definition.
 */
async function loadSkillDefinition(
  dirPath: string,
): Promise<SkillDefinition | undefined> {
  try {
    // Check if skill.ts exists by trying to read it
    const skillTsContent = await invoke<string>("ai_read_memory_file", {
      relativePath: `${dirPath}/skill.ts`,
    });

    if (!skillTsContent || skillTsContent.trim().length === 0) {
      return undefined;
    }

    // Parse the skill definition from the TypeScript source.
    // Extract the exported definition object using a simple regex-based approach.
    // In production, this would use a proper TS compiler or dynamic import.
    return parseSkillDefinitionFromSource(skillTsContent);
  } catch {
    // No skill.ts — prompt-only skill
    return undefined;
  }
}

/**
 * Parse a SkillDefinition from TypeScript source content.
 *
 * Extracts the name, description, version, and hook/tool declarations
 * from the source text. This is a lightweight static analysis approach;
 * hooks are registered separately by the registry at runtime.
 */
function parseSkillDefinitionFromSource(source: string): SkillDefinition | undefined {
  // Extract basic fields from the skill definition object
  const nameMatch = source.match(/name:\s*["']([^"']+)["']/);
  const descMatch = source.match(/description:\s*["']([^"']+)["']/);
  const versionMatch = source.match(/version:\s*["']([^"']+)["']/);
  const tickMatch = source.match(/tickInterval:\s*(\d[\d_]*)/);

  if (!nameMatch || !descMatch) return undefined;

  // Detect which hooks are defined
  const hookNames = [
    "onLoad",
    "onUnload",
    "onSessionStart",
    "onSessionEnd",
    "onBeforeMessage",
    "onAfterResponse",
    "onMemoryFlush",
    "onTick",
  ] as const;

  const hooks: SkillDefinition["hooks"] = {};
  for (const hookName of hookNames) {
    // Check if the hook is defined (async function or arrow function)
    const hookPattern = new RegExp(
      `(?:async\\s+)?${hookName}\\s*(?:\\(|:)`,
    );
    if (hookPattern.test(source)) {
      // Mark hook as present with a placeholder.
      // The actual implementation will be provided by the runtime loader.
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (hooks as any)[hookName] = async () => {};
    }
  }

  return {
    name: nameMatch[1],
    description: descMatch[1],
    version: versionMatch?.[1] ?? "1.0.0",
    hooks,
    tickInterval: tickMatch
      ? parseInt(tickMatch[1].replace(/_/g, ""), 10)
      : undefined,
  };
}

/**
 * Load skills from the bundled skills directory.
 * These are skills that ship with the app.
 */
export async function loadBundledSkills(): Promise<SkillEntry[]> {
  return [];
}
