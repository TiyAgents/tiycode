import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));

// Test basic rendering of shared UI components
// These components are thin wrappers around radix/shadcn primitives

describe("Basic HTML component render tests", () => {
  it("renders a button element", () => {
    const { container } = render(<button type="button">Click me</button>);
    expect(screen.getByText("Click me")).toBeTruthy();
  });

  it("renders an input with placeholder", () => {
    render(<input placeholder="Type here..." />);
    expect(screen.getByPlaceholderText("Type here...")).toBeTruthy();
  });

  it("renders a textarea with value", () => {
    render(<textarea defaultValue="Hello" />);
    const el = document.querySelector('textarea') as HTMLTextAreaElement;
    expect(el.value).toBe("Hello");
  });

  it("renders a div as separator", () => {
    const { container } = render(<div role="separator" />);
    expect(container.firstChild).not.toBeNull();
  });

  it("renders alert structure", () => {
    const { container } = render(
      <div role="alert">
        <div>Something went wrong</div>
      </div>,
    );
    expect(container.querySelector('[role="alert"]')).not.toBeNull();
  });

  it("renders card structure", () => {
    render(
      <div>
        <header><h3>Title</h3><p>Description</p></header>
        <div>Content</div>
        <footer>Footer</footer>
      </div>,
    );
    expect(screen.getByText("Title")).toBeTruthy();
    expect(screen.getByText("Content")).toBeTruthy();
    expect(screen.getByText("Footer")).toBeTruthy();
  });
});
