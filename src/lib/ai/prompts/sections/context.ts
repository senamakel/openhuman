/**
 * User context section of the system prompt.
 * Injects preferences, timezone, and project-specific context.
 */

export interface UserContext {
  /** User's timezone (IANA format) */
  timezone?: string;
  /** User display name */
  displayName?: string;
  /** User preferences loaded from memory */
  preferences?: string;
  /** Content of memory.md (always in context) */
  memoryContext?: string;
  /** Content of identity.md */
  identityContext?: string;
}

/**
 * Build the user context section.
 */
export function buildContextSection(context: UserContext): string {
  const parts: string[] = [];

  if (context.displayName || context.timezone) {
    parts.push("## User Context\n");
    if (context.displayName) {
      parts.push(`- **User**: ${context.displayName}`);
    }
    if (context.timezone) {
      parts.push(`- **Timezone**: ${context.timezone}`);
    }
    parts.push("");
  }

  if (context.preferences) {
    parts.push("## User Preferences\n");
    parts.push(context.preferences);
    parts.push("");
  }

  if (context.memoryContext) {
    parts.push("## Project Context (memory.md)\n");
    parts.push(context.memoryContext);
    parts.push("");
  }

  if (context.identityContext) {
    parts.push("## Agent Persona (identity.md)\n");
    parts.push(context.identityContext);
    parts.push("");
  }

  return parts.join("\n");
}
