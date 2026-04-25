import { describe, expect, it } from "vitest";
import { translate } from "./index";

describe("translate", () => {
  it("returns the translated string for a valid key", () => {
    expect(translate("en", "time.justNow")).toBe("Just now");
    expect(translate("zh-CN", "time.justNow")).toBe("刚刚");
  });

  it("falls back to zh-CN when a key is missing in the target language", () => {
    expect(translate("en", "dashboard.newThread")).toBe("New thread");
  });

  it("interpolates {{count}} params into the translated string", () => {
    const result = translate("en", "time.minutesAgo", { count: 5 });
    expect(result).toContain("5");
  });

  it("returns the key itself when the translation is missing", () => {
    expect(translate("en", "nonexistent.key" as never)).toBe("nonexistent.key");
    expect(translate("zh-CN", "nonexistent.key" as never)).toBe("nonexistent.key");
  });

  it("handles empty params gracefully", () => {
    expect(translate("en", "time.justNow")).toBe("Just now");
  });
});
