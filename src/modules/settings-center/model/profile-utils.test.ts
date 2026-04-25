import { describe, expect, it } from "vitest";
import type { AgentProfile } from "./types";
import { compareAgentProfilesByName, sortAgentProfilesByName } from "./profile-utils";

function profile(id: string, name: string): AgentProfile {
  return {
    id,
    name,
    customInstructions: "",
    commitMessagePrompt: "",
    responseStyle: "balanced",
    responseLanguage: "zh-CN",
    commitMessageLanguage: "en",
    thinkingLevel: "medium",
    primaryProviderId: "",
    primaryModelId: "",
    assistantProviderId: "",
    assistantModelId: "",
    liteProviderId: "",
    liteModelId: "",
  };
}

describe("profile utils", () => {
  it("compares names case-insensitively and falls back to id", () => {
    expect(compareAgentProfilesByName(profile("b", "alpha"), profile("a", "Alpha"))).toBeGreaterThan(0);
    expect(compareAgentProfilesByName(profile("a", "Beta"), profile("b", "gamma"))).toBeLessThan(0);
  });

  it("sorts without mutating the input array", () => {
    const profiles = [profile("2", "Beta"), profile("1", "alpha")];
    const sorted = sortAgentProfilesByName(profiles);

    expect(sorted.map((entry) => entry.id)).toEqual(["1", "2"]);
    expect(profiles.map((entry) => entry.id)).toEqual(["2", "1"]);
  });
});
