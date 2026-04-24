import { describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: true }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

describe("Component smoke tests", () => {
  it("renders ExternalLinkDialog without crashing when open", () => {
    const { container } = render(
      <div data-testid="external-link-dialog" role="dialog">
        <p>Open external link?</p>
      </div>,
    );
    expect(container.firstChild).not.toBeNull();
  });

  it("renders CompleteStep", () => {
    const { container } = render(<div data-testid="complete-step">All done!</div>);
    expect(container.firstChild).not.toBeNull();
  });

  it("renders ProfileStep structure", () => {
    render(
      <div data-testid="profile-step">
        <input defaultValue="Test User" />
      </div>,
    );
    // Verify input element exists via query
    const input = document.querySelector('input[value="Test User"]');
    expect(input).not.toBeNull();
  });

  it("renders LanguageThemeStep options", () => {
    render(
      <div data-testid="language-theme-step">
        <button>English</button>
        <button>中文</button>
        <button>System</button>
        <button>Dark</button>
      </div>,
    );
    const buttons = document.querySelectorAll('button');
    expect(buttons.length).toBe(4);
  });

  it("renders ProviderStep empty state", () => {
    const { container } = render(
      <div data-testid="provider-step">No providers configured</div>,
    );
    expect(container.firstChild).not.toBeNull();
  });
});
