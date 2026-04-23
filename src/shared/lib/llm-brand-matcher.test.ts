import { describe, expect, it } from "vitest";
import { matchModelIcon } from "@/shared/lib/llm-brand-matcher";

describe("matchModelIcon", () => {
  it("maps hunyuan short model aliases like hy3 and hy4 to the hunyuan icon slug", () => {
    expect(matchModelIcon("hy3")).toBe("hunyuan");
    expect(matchModelIcon("hy4")).toBe("hunyuan");
    expect(matchModelIcon("foo/hy5-chat")).toBe("hunyuan");
    expect(matchModelIcon("model-hy12-preview")).toBe("hunyuan");
  });

  it("does not match unrelated names that only contain hy without digits", () => {
    expect(matchModelIcon("hyperbolic")).toBeNull();
    expect(matchModelIcon("my-hybrid-model")).toBeNull();
    expect(matchModelIcon("hy-model")).toBeNull();
  });
});
