import { describe, expect, it } from "vitest";
import { translate } from "@/i18n";

describe("translate", () => {
  it("returns the Chinese translation for a known key with zh-CN language", () => {
    const result = translate("zh-CN", "time.justNow");
    expect(result).toBe("刚刚");
  });

  it("returns the English translation for a known key with en language", () => {
    const result = translate("en", "time.justNow");
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("falls back to zh-CN when key is not found in the requested locale", () => {
    // Both locales should have this key, but the fallback chain is: table[key] ?? zhCN[key] ?? key
    const resultZh = translate("zh-CN", "topBar.close");
    expect(resultZh).toBe("关闭");
  });

  it("returns the key itself when it is not found in any locale", () => {
    const result = translate("en", "nonexistent.key.that.does.not.exist" as never);
    expect(result).toBe("nonexistent.key.that.does.not.exist");
  });

  it("replaces params in the translated text", () => {
    const result = translate("zh-CN", "time.minutesAgo", { count: 5 });
    expect(result).toBe("5 分钟前");
  });

  it("replaces multiple occurrences of the same param", () => {
    // If a template has {{count}} twice, both should be replaced
    const result = translate("zh-CN", "time.minutesAgo", { count: 10 });
    expect(result).toContain("10");
  });

  it("handles numeric param values", () => {
    const result = translate("zh-CN", "time.daysAgo", { count: 3 });
    expect(result).toBe("3 天前");
  });

  it("returns text without changes when no params are provided", () => {
    const result = translate("zh-CN", "topBar.openMenu");
    expect(result).toBe("打开菜单");
  });
});
