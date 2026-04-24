import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));

import {
  Message,
  MessageContent,
  MessageActions,
  MessageToolbar,
} from "@/components/ai-elements/message";

describe("Message", () => {
  it("renders with user role", () => {
    const { container } = render(<Message from="user">Hello</Message>);
    // Should have a child element
    expect(container.firstChild).not.toBeNull();
  });

  it("renders with assistant role", () => {
    const { container } = render(<Message from="assistant">Hi there</Message>);
    expect(container.firstChild).not.toBeNull();
    expect(screen.getByText("Hi there")).toBeTruthy();
  });

  it("renders empty content gracefully", () => {
    const { container } = render(<Message from="assistant"></Message>);
    expect(container.firstChild).not.toBeNull();
  });
});

describe("MessageContent", () => {
  it("renders children", () => {
    render(<MessageContent>Content here</MessageContent>);
    expect(screen.getByText("Content here")).toBeTruthy();
  });
});

describe("MessageActions", () => {
  it("renders action children as buttons", () => {
    render(
      <MessageActions>
        <button type="button">Copy</button>
        <button type="button">Edit</button>
      </MessageActions>,
    );
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBe(2);
  });
});

describe("MessageToolbar", () => {
  it("renders toolbar children", () => {
    render(
      <MessageToolbar>
        <span>Left</span>
        <span>Right</span>
      </MessageToolbar>,
    );
    expect(screen.getByText("Left")).toBeTruthy();
    expect(screen.getByText("Right")).toBeTruthy();
  });
});
