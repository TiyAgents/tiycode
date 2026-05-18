import { describe, expect, it } from "vitest";

import { isLocalRelativeUrl, normalizeRelativePath } from "./markdown-preview-paths";

describe("normalizeRelativePath", () => {
  it("collapses empty, current-directory, and parent-directory segments", () => {
    expect(normalizeRelativePath("./public/../assets/logo.png")).toBe("assets/logo.png");
    expect(normalizeRelativePath("docs//images/./logo.png")).toBe("docs/images/logo.png");
  });

  it("preserves leading parent-directory traversal so backend validation can reject it", () => {
    expect(normalizeRelativePath("../secret.png")).toBe("../secret.png");
    expect(normalizeRelativePath("../../secret.png")).toBe("../../secret.png");
    expect(normalizeRelativePath("docs/../../../secret.png")).toBe("../../secret.png");
  });

  it("handles empty and root-like relative inputs", () => {
    expect(normalizeRelativePath("")).toBe("");
    expect(normalizeRelativePath(".")).toBe("");
    expect(normalizeRelativePath("./")).toBe("");
    expect(normalizeRelativePath("a/b/c/")).toBe("a/b/c");
  });
});

describe("isLocalRelativeUrl", () => {
  it("accepts local relative URLs", () => {
    expect(isLocalRelativeUrl("image.png")).toBe(true);
    expect(isLocalRelativeUrl("./image.png")).toBe(true);
    expect(isLocalRelativeUrl("../image.png")).toBe(true);
    expect(isLocalRelativeUrl("docs/image.png?raw=1#anchor")).toBe(true);
  });

  it("rejects non-local or non-image-reference URL forms", () => {
    expect(isLocalRelativeUrl(undefined)).toBe(false);
    expect(isLocalRelativeUrl("")).toBe(false);
    expect(isLocalRelativeUrl("https://example.com/image.png")).toBe(false);
    expect(isLocalRelativeUrl("data:image/png;base64,abc")).toBe(false);
    expect(isLocalRelativeUrl("javascript:alert(1)")).toBe(false);
    expect(isLocalRelativeUrl("//cdn.example.com/image.png")).toBe(false);
    expect(isLocalRelativeUrl("#section")).toBe(false);
  });
});
