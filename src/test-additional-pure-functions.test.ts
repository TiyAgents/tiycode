import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));

import { messagesToMarkdown } from "@/components/ai-elements/conversation";
import { isOnboardingCompleted } from "@/modules/onboarding/model/use-onboarding";
import { countDiffLineChanges } from "@/modules/workbench-shell/model/file-mutation-presentation";
import { buildThreadTitle } from "@/modules/workbench-shell/model/helpers";
import { streamdownLinkSafety } from "@/shared/lib/streamdown-link-safety";
import { ONBOARDING_COMPLETED_KEY, ONBOARDING_STEPS } from "@/modules/onboarding/model/types";

describe("messagesToMarkdown", () => {
  it("returns empty string for empty messages", () => {
    expect(messagesToMarkdown([])).toBe("");
  });

  it.each([
    ["user", "Hello AI"],
    ["assistant", "Hi there!"],
  ])("converts %s message to text", (role, text) => {
    const result = messagesToMarkdown([
      { id: "1", role, parts: [{ type: "text", text }] },
    ] as any);
    expect(result).toContain(text);
  });
});

describe("isOnboardingCompleted", () => {
  afterEach(() => {
    localStorage.removeItem(ONBOARDING_COMPLETED_KEY);
  });

  it("returns false when key not set", () => {
    expect(isOnboardingCompleted()).toBe(false);
  });

  it("returns true when key is set", () => {
    localStorage.setItem(ONBOARDING_COMPLETED_KEY, "true");
    expect(isOnboardingCompleted()).toBe(true);
  });

  it("returns false for non-true values", () => {
    localStorage.setItem(ONBOARDING_COMPLETED_KEY, "false");
    expect(isOnboardingCompleted()).toBe(false);
  });
});

describe("ONBOARDING_STEPS", () => {
  it("has at least one step", () => {
    expect(ONBOARDING_STEPS.length).toBeGreaterThan(0);
  });

  it("each step is a non-empty string", () => {
    for (const step of ONBOARDING_STEPS) {
      expect(typeof step).toBe("string");
      expect(step.length).toBeGreaterThan(0);
    }
  });

  it("contains the expected steps", () => {
    expect(ONBOARDING_STEPS).toContain("language-theme");
    expect(ONBOARDING_STEPS).toContain("provider");
    expect(ONBOARDING_STEPS).toContain("complete");
  });
});

describe("countDiffLineChanges", () => {
  it("counts added lines starting with +", () => {
    const diff = "+ line 1\n+ line 2\n- line 3\n  context";
    const result = countDiffLineChanges(diff);
    expect(result.linesAdded).toBe(2);
    expect(result.linesRemoved).toBe(1);
  });

  it("handles empty diff", () => {
    const result = countDiffLineChanges("");
    expect(result.linesAdded).toBe(0);
    expect(result.linesRemoved).toBe(0);
  });
});

describe("buildThreadTitle", () => {
  it("uses prompt directly as title", () => {
    expect(buildThreadTitle("Hello world")).toBe("Hello world");
  });

  it("truncates long prompts", () => {
    const long = "a".repeat(200);
    const title = buildThreadTitle(long);
    expect(title.length).toBeLessThan(200);
  });

  it("handles empty string", () => {
    expect(buildThreadTitle("")).toBe("");
  });
});

describe("streamdownLinkSafety", () => {
  it("has expected structure with boolean flags", () => {
    expect(typeof streamdownLinkSafety.enabled).toBe("boolean");
    expect(typeof streamdownLinkSafety.renderModal).toBe("function");
  });
});
