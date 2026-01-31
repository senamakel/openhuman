import { describe, it, expect } from "vitest";
import { parseFrontmatter, generateFrontmatter } from "../frontmatter";

describe("parseFrontmatter", () => {
  it("should parse valid YAML frontmatter", () => {
    const content = `---
name: price-tracker
description: Track crypto token prices and set alerts.
---

# Price Tracker

## Overview
Track prices.`;

    const { frontmatter, body } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("price-tracker");
    expect(frontmatter.description).toBe(
      "Track crypto token prices and set alerts.",
    );
    expect(body).toContain("# Price Tracker");
    expect(body).toContain("## Overview");
  });

  it("should handle quoted values", () => {
    const content = `---
name: "my-skill"
description: 'A skill with quotes'
---

Body.`;

    const { frontmatter } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("my-skill");
    expect(frontmatter.description).toBe("A skill with quotes");
  });

  it("should handle missing frontmatter by extracting name from heading", () => {
    const content = `# Portfolio Analysis

This skill analyzes portfolios.`;

    const { frontmatter, body } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("portfolio-analysis");
    expect(frontmatter.description).toBe("");
    expect(body).toBe(content);
  });

  it("should handle content with no frontmatter and no heading", () => {
    const content = "Just some content without structure.";
    const { frontmatter } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("unnamed");
  });

  it("should handle empty frontmatter", () => {
    const content = `---
---

Body content.`;

    // Empty frontmatter (no content between delimiters) doesn't match
    // the regex, so falls through to "no frontmatter" branch
    const { frontmatter, body } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("unnamed");
    expect(body).toBe(content);
  });

  it("should handle frontmatter with extra whitespace", () => {
    const content = `---
name:   spaced-skill
description:   A skill with spaces
---

Content.`;

    const { frontmatter } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("spaced-skill");
    expect(frontmatter.description).toBe("A skill with spaces");
  });

  it("should ignore comment lines in frontmatter", () => {
    const content = `---
name: my-skill
# This is a comment
description: My description
---

Content.`;

    const { frontmatter } = parseFrontmatter(content);
    expect(frontmatter.name).toBe("my-skill");
    expect(frontmatter.description).toBe("My description");
  });
});

describe("generateFrontmatter", () => {
  it("should generate valid YAML frontmatter string", () => {
    const result = generateFrontmatter({
      name: "test-skill",
      description: "A test skill for testing.",
    });
    expect(result).toBe(
      "---\nname: test-skill\ndescription: A test skill for testing.\n---",
    );
  });

  it("should roundtrip through parse", () => {
    const original = {
      name: "roundtrip-skill",
      description: "Test roundtrip parsing.",
    };
    const generated = generateFrontmatter(original);
    const body = "\n\n# Content\nHello.";
    const { frontmatter } = parseFrontmatter(generated + body);
    expect(frontmatter.name).toBe(original.name);
    expect(frontmatter.description).toBe(original.description);
  });
});
