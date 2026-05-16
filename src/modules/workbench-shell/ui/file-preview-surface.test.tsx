import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { FilePreviewSurface } from "./file-preview-surface";

vi.mock("@/modules/workbench-shell/ui/workbench-preview-overlay", () => ({
  WorkbenchPreviewOverlay: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

describe("FilePreviewSurface", () => {
  it("renders HTML previews in a sandbox without same-origin privileges", () => {
    const html = renderToStaticMarkup(
      <FilePreviewSurface
        open
        onClose={() => undefined}
        source="<script>window.parent.postMessage('x', '*')</script>"
        contentType="html"
      />,
    );

    expect(html).toContain('<iframe srcDoc="&lt;script&gt;');
    expect(html).toContain('sandbox="allow-scripts"');
    expect(html).not.toContain("allow-same-origin");
  });
});
