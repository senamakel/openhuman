import type { SkillFrontmatter } from "./types";

/**
 * Parse YAML frontmatter from a SKILL.md file.
 *
 * Expected format:
 * ```
 * ---
 * name: skill-name
 * description: Brief description of what this skill does.
 * ---
 *
 * # Skill Name
 * ...
 * ```
 */
export function parseFrontmatter(content: string): {
  frontmatter: SkillFrontmatter;
  body: string;
} {
  const match = content.match(/^---\s*\n([\s\S]*?)\n---\s*\n?([\s\S]*)$/);

  if (!match) {
    // No frontmatter — try to extract name from first heading
    const headingMatch = content.match(/^#\s+(.+)/m);
    return {
      frontmatter: {
        name: headingMatch?.[1]?.toLowerCase().replace(/\s+/g, "-") || "unnamed",
        description: "",
      },
      body: content,
    };
  }

  const yamlBlock = match[1];
  const body = match[2];

  // Simple YAML parser for flat key-value pairs
  const frontmatter = parseSimpleYaml(yamlBlock);

  return {
    frontmatter: {
      name: String(frontmatter.name || "unnamed"),
      description: String(frontmatter.description || ""),
      ...(frontmatter.metadata ? { metadata: frontmatter.metadata as SkillFrontmatter["metadata"] } : {}),
    },
    body,
  };
}

/**
 * Simple YAML parser for flat frontmatter.
 * Handles basic key: value pairs and simple nested objects.
 */
function parseSimpleYaml(yaml: string): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  const lines = yaml.split("\n");

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const colonIndex = trimmed.indexOf(":");
    if (colonIndex === -1) continue;

    const key = trimmed.slice(0, colonIndex).trim();
    const value = trimmed.slice(colonIndex + 1).trim();

    if (value) {
      // Remove surrounding quotes if present
      result[key] = value.replace(/^["']|["']$/g, "");
    }
  }

  return result;
}

/**
 * Generate frontmatter string from a SkillFrontmatter object.
 */
export function generateFrontmatter(fm: SkillFrontmatter): string {
  const lines = ["---"];
  lines.push(`name: ${fm.name}`);
  lines.push(`description: ${fm.description}`);
  lines.push("---");
  return lines.join("\n");
}
