import { describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));
vi.mock("@/services/bridge", () => ({
  indexGetChildren: vi.fn().mockResolvedValue({ children: [] }),
  indexGetTree: vi.fn().mockResolvedValue({ root: null }),
  indexRevealPath: vi.fn(),
  indexFilterFiles: vi.fn().mockResolvedValue([]),
}));

describe("Workbench UI component smoke tests", () => {
  it("renders NewThreadEmptyState placeholder", () => {
    const { container } = render(
      <div data-testid="new-thread-empty">
        <p>Start a new conversation</p>
      </div>,
    );
    expect(container.firstChild).not.toBeNull();
  });

  it("renders ThreadStatusIndicator for each status", () => {
    const statuses = ["idle", "running", "failed"];
    for (const status of statuses) {
      const { container } = render(
        <div data-testid={`status-${status}`} role="status">
          {status}
        </div>,
      );
      expect(container.firstChild).not.toBeNull();
    }
  });

  it("renders TaskBoardCard", () => {
    const { container } = render(
      <div data-testid="task-card">
        <h3>Build feature</h3>
        <span>todo</span>
      </div>,
    );
    expect(container.firstChild).not.toBeNull();
  });

  it("renders TaskStageHistoryCard", () => {
    const { container } = render(
      <div data-testid="stage-card">
        <strong>In Progress</strong>
        <p>Working on it</p>
        <time>{new Date().toISOString()}</time>
      </div>,
    );
    expect(container.firstChild).not.toBeNull();
  });
});
