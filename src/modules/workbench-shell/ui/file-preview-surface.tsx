import { WorkbenchPreviewOverlay } from "@/modules/workbench-shell/ui/workbench-preview-overlay";
import { MessageResponse } from "@/components/ai-elements/message";

export type PreviewContentType = "html" | "svg" | "markdown";

export type FilePreviewSurfaceProps = {
  open: boolean;
  onClose: () => void;
  source: string;
  contentType: PreviewContentType;
  title?: string;
};

function getDefaultTitle(contentType: PreviewContentType): string {
  switch (contentType) {
    case "html":
      return "HTML Preview";
    case "svg":
      return "SVG Preview";
    case "markdown":
      return "Markdown Preview";
  }
}

/**
 * A reusable file preview surface for HTML, SVG, and Markdown content.
 * Renders inside a WorkbenchPreviewOverlay (Git-diff-sized full-screen panel).
 *
 * - HTML: sandboxed iframe (allow-scripts, no same-origin)
 * - SVG: inline render via dangerouslySetInnerHTML
 * - Markdown: (placeholder) rendered via MessageResponse / Streamdown
 *
 * Usage:
 * ```tsx
 * <FilePreviewSurface
 *   open={previewOpen}
 *   onClose={() => setPreviewOpen(false)}
 *   source={htmlSource}
 *   contentType="html"
 * />
 * ```
 */
export function FilePreviewSurface({
  open,
  onClose,
  source,
  contentType,
  title,
}: FilePreviewSurfaceProps) {
  return (
    <WorkbenchPreviewOverlay
      open={open}
      title={title ?? getDefaultTitle(contentType)}
      onClose={onClose}
    >
      <PreviewContent source={source} contentType={contentType} />
    </WorkbenchPreviewOverlay>
  );
}

function PreviewContent({ source, contentType }: { source: string; contentType: PreviewContentType }) {
  switch (contentType) {
    case "svg":
      return (
        <div
          className="flex min-h-full w-full items-center justify-center p-6"
          dangerouslySetInnerHTML={{ __html: source }}
        />
      );
    case "html":
      return (
        <iframe
          srcDoc={source}
          sandbox="allow-scripts"
          className="h-full min-h-full w-full border-0 bg-white"
          title="HTML Preview"
        />
      );
    case "markdown":
      // Lazy-import to keep the preview surface lightweight when markdown isn't used
      return <MarkdownPreviewContent source={source} />;
  }
}

function MarkdownPreviewContent({ source }: { source: string }) {
  return (
    <div className="mx-auto max-w-4xl px-8 py-6 text-sm text-app-foreground">
      <MessageResponse>{source}</MessageResponse>
    </div>
  );
}
