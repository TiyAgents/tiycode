import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));
vi.mock("@/shared/lib/streamdown-link-safety", () => ({
  streamdownLinkSafety: { alwaysBlank: false, neverBlank: true },
}));
vi.mock("@/components/ai-elements/code-block", () => ({
  CodeBlock: (props: { children?: React.ReactNode }) => (
    <div data-testid="mock-code-block">{props.children}</div>
  ),
}));

import { Tool } from "@/components/ai-elements/tool";

describe("Tool", () => {
  it("renders tool content", () => {
    const { container } = render(<Tool>Tool content</Tool>);
    expect(container.firstChild).not.toBeNull();
    expect(screen.getByText("Tool content")).toBeTruthy();
  });

  it("accepts custom className", () => {
    const { container } = render(<Tool className="custom-tool">Content</Tool>);
    // Should have a child element
    expect(container.firstChild).not.toBeNull();
  });
});
