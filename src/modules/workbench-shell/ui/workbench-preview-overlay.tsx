import { type ReactNode } from "react";
import { CircleXIcon } from "lucide-react";
import { useT } from "@/i18n";

export type WorkbenchPreviewOverlayProps = {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Optional extra class on the root overlay container */
  overlayClassName?: string;
  /** Optional extra class on the inner card container */
  className?: string;
};

/**
 * A full-screen overlay container for workbench previews (HTML, SVG, Markdown,
 * diff, etc.). Matches the Git diff preview panel's visual spec:
 * - Fixed inset overlay with backdrop blur
 * - Centered card: h-[min(82vh,860px)] max-w-7xl rounded-[24px]
 * - Standard header with title + close button
 * - Scrollable content area slot via children
 */
export function WorkbenchPreviewOverlay({
  open,
  title,
  onClose,
  children,
  overlayClassName,
  className,
}: WorkbenchPreviewOverlayProps) {
  const t = useT();
  if (!open) return null;

  return (
    <div
      className={`fixed inset-0 z-[80] flex items-center justify-center bg-app-chrome/50 px-6 py-12 backdrop-blur-sm ${overlayClassName ?? ""}`}
      onClick={onClose}
    >
      <div
        className={`flex h-[min(82vh,860px)] w-full max-w-7xl flex-col overflow-hidden rounded-[24px] border border-app-border bg-app-surface shadow-[0_32px_96px_rgba(15,23,42,0.28)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.56)] ${className ?? ""}`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="shrink-0 border-b border-app-border/30 px-5 py-4">
          <div className="flex items-center justify-between gap-4">
            <h2 className="text-sm font-medium text-app-foreground">{title}</h2>
            <button
              type="button"
              aria-label={t("fileEditor.closePreview")}
              title={t("fileEditor.closePreview")}
              className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onClose}
            >
              <CircleXIcon className="size-4" />
            </button>
          </div>
        </div>

        {/* Content */}
        <div className="min-h-0 flex-1 overflow-auto bg-app-canvas/70 select-text">
          {children}
        </div>
      </div>
    </div>
  );
}
