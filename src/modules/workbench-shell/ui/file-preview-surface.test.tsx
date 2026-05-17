import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

import { FilePreviewSurface } from "./file-preview-surface";
import { HTML_PREVIEW_CSP } from "./html-preview-document";

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

    expect(html).toContain('<iframe srcDoc="&lt;!doctype html&gt;');
    expect(html).toContain('sandbox="allow-scripts"');
    expect(html).not.toContain("allow-same-origin");
    expect(html).toContain("Content-Security-Policy");
    expect(html).toContain(HTML_PREVIEW_CSP.replace(/"/g, "&quot;").replace(/'/g, "&#x27;"));
    expect(html).toContain("img-src http: https: data: blob: asset:");
    expect(html).toContain("style-src &#x27;unsafe-inline&#x27; http: https: asset:");
  });
});
