import { describe, it, expect } from "vitest";
import { SkillRegistry } from "../registry";
import type { SkillEntry } from "../types";

// Mock the loader since it calls Tauri invoke
vi.mock("../loader", () => ({
  loadSkills: async (): Promise<SkillEntry[]> => [
    {
      name: "price-tracker",
      description: "Track crypto prices",
      location: "skills/price-tracker",
      content: "# Price Tracker\n...",
      installed: true,
      source: "local" as const,
    },
    {
      name: "portfolio-analysis",
      description: "Analyze portfolio allocations",
      location: "skills/portfolio-analysis",
      content: "# Portfolio\n...",
      installed: true,
      source: "local" as const,
    },
  ],
}));

// Mock the runner since it imports Tauri invoke
vi.mock("../runner", () => ({
  createSkillContext: () => ({}),
  runHook: async () => undefined,
  runBeforeMessage: async (_skills: unknown, message: string) => message,
  runAfterResponse: async (_skills: unknown, response: string) => response,
}));

describe("SkillRegistry", () => {
  it("should start with no skills", () => {
    const registry = new SkillRegistry();
    expect(registry.count).toBe(0);
    expect(registry.getSkills()).toHaveLength(0);
  });

  it("should load skills on reload", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    expect(registry.count).toBe(2);
    expect(registry.getSkills()).toHaveLength(2);
  });

  it("should find skill by name", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    const skill = registry.findSkill("price-tracker");
    expect(skill).toBeDefined();
    expect(skill?.name).toBe("price-tracker");
  });

  it("should find skill case-insensitively", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    expect(registry.findSkill("Price-Tracker")).toBeDefined();
  });

  it("should return undefined for unknown skill", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    expect(registry.findSkill("nonexistent")).toBeUndefined();
  });

  it("should search skills by query", async () => {
    const registry = new SkillRegistry();
    await registry.reload();

    const priceResults = registry.searchSkills("price");
    expect(priceResults).toHaveLength(1);
    expect(priceResults[0].name).toBe("price-tracker");

    const portfolioResults = registry.searchSkills("portfolio");
    expect(portfolioResults).toHaveLength(1);
    expect(portfolioResults[0].name).toBe("portfolio-analysis");
  });

  it("should search by description", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    const results = registry.searchSkills("allocations");
    expect(results).toHaveLength(1);
  });

  it("should build prompt section", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    const prompt = registry.buildPromptSection();
    expect(prompt).toContain("<available_skills>");
    expect(prompt).toContain("price-tracker");
    expect(prompt).toContain("portfolio-analysis");
  });

  it("should create snapshot", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    const snapshot = registry.createSnapshot();
    expect(snapshot.skills).toHaveLength(2);
    expect(snapshot.prompt).toContain("<available_skills>");
    expect(snapshot.version).toBeGreaterThan(0);
  });

  it("should return immutable skills array", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    const skills1 = registry.getSkills();
    const skills2 = registry.getSkills();
    expect(skills1).not.toBe(skills2); // Different array references
    expect(skills1).toEqual(skills2); // Same content
  });

  it("should track active count separately from total count", async () => {
    const registry = new SkillRegistry();
    await registry.reload();
    // No managers set, so no skills should be active
    expect(registry.count).toBe(2);
    expect(registry.activeCount).toBe(0);
  });
});
