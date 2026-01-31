import { invoke } from "@tauri-apps/api/core";
import { parseFrontmatter } from "./frontmatter";
import { MEMORY_PATHS } from "../memory/types";

/** Installation result */
export interface InstallResult {
  success: boolean;
  skillName: string;
  error?: string;
}

/**
 * Install a skill from a GitHub repository.
 *
 * Fetches the SKILL.md and any associated files from the repo,
 * validates the format, and copies them to the local skills directory.
 */
export async function installSkill(params: {
  /** GitHub repo URL or shorthand (e.g., 'alphahuman/alphahuman-skills') */
  repoUrl: string;
  /** Skill name (directory name in the repo's skills/ folder) */
  skillName: string;
  /** Branch to fetch from (default: 'main') */
  branch?: string;
}): Promise<InstallResult> {
  const { repoUrl, skillName, branch = "main" } = params;

  try {
    // Normalize repo URL to raw GitHub content URL
    const rawBase = toRawGitHubUrl(repoUrl, branch);
    const skillPath = `skills/${skillName}`;

    // Fetch SKILL.md
    const skillMdUrl = `${rawBase}/${skillPath}/SKILL.md`;
    const response = await fetch(skillMdUrl);
    if (!response.ok) {
      return {
        success: false,
        skillName,
        error: `Skill not found at ${skillMdUrl} (${response.status})`,
      };
    }

    const content = await response.text();

    // Validate frontmatter
    const { frontmatter } = parseFrontmatter(content);
    if (!frontmatter.name || !frontmatter.description) {
      return {
        success: false,
        skillName,
        error: "Invalid SKILL.md: missing name or description in frontmatter",
      };
    }

    // Write to local skills directory
    const localDir = `${MEMORY_PATHS.SKILLS_DIR}/${skillName}`;
    await invoke("ai_write_memory_file", {
      relativePath: `${localDir}/SKILL.md`,
      content,
    });

    // Try to fetch skill.ts (optional — not all skills have one)
    try {
      const skillTsUrl = `${rawBase}/${skillPath}/skill.ts`;
      const tsResponse = await fetch(skillTsUrl);
      if (tsResponse.ok) {
        const tsContent = await tsResponse.text();
        await invoke("ai_write_memory_file", {
          relativePath: `${localDir}/skill.ts`,
          content: tsContent,
        });
      }
    } catch {
      // skill.ts fetch failed — that's fine, skill works as prompt-only
    }

    return { success: true, skillName: frontmatter.name };
  } catch (error) {
    return {
      success: false,
      skillName,
      error: `Installation failed: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
}

/**
 * Uninstall a skill by removing its directory.
 */
export async function uninstallSkill(skillName: string): Promise<boolean> {
  try {
    // Remove SKILL.md (we can't delete directories via our Rust commands,
    // but removing the SKILL.md effectively disables the skill)
    await invoke("ai_write_memory_file", {
      relativePath: `${MEMORY_PATHS.SKILLS_DIR}/${skillName}/SKILL.md`,
      content: "",
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * List available skills from a GitHub repository.
 */
export async function listRepoSkills(params: {
  repoUrl: string;
  branch?: string;
}): Promise<string[]> {
  const { repoUrl, branch = "main" } = params;

  try {
    // Use GitHub API to list directory contents
    const apiUrl = toGitHubApiUrl(repoUrl, branch, "skills");
    const response = await fetch(apiUrl, {
      headers: { Accept: "application/vnd.github.v3+json" },
    });

    if (!response.ok) return [];

    const entries: Array<{ name: string; type: string }> = await response.json();
    return entries
      .filter((e) => e.type === "dir")
      .map((e) => e.name);
  } catch {
    return [];
  }
}

/**
 * Convert a GitHub repo reference to a raw content URL.
 */
function toRawGitHubUrl(repoUrl: string, branch: string): string {
  // Handle shorthand (owner/repo)
  if (!repoUrl.includes("://")) {
    return `https://raw.githubusercontent.com/${repoUrl}/${branch}`;
  }
  // Handle full URL
  const match = repoUrl.match(
    /github\.com\/([^/]+)\/([^/]+)/,
  );
  if (match) {
    return `https://raw.githubusercontent.com/${match[1]}/${match[2]}/${branch}`;
  }
  return repoUrl;
}

/**
 * Convert a GitHub repo reference to an API URL.
 */
function toGitHubApiUrl(
  repoUrl: string,
  branch: string,
  path: string,
): string {
  if (!repoUrl.includes("://")) {
    return `https://api.github.com/repos/${repoUrl}/contents/${path}?ref=${branch}`;
  }
  const match = repoUrl.match(
    /github\.com\/([^/]+)\/([^/]+)/,
  );
  if (match) {
    return `https://api.github.com/repos/${match[1]}/${match[2]}/contents/${path}?ref=${branch}`;
  }
  return repoUrl;
}
