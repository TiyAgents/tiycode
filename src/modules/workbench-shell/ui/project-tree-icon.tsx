import { cn } from "@/shared/lib/utils";
import { getFileTypeIconHref } from "@/shared/lib/file-type-icons";

/**
 * File/folder type icon rendered from the SVG sprite.
 * Rendered monochrome via CSS filter, adapts to light/dark theme automatically.
 */
export function ProjectTreeIcon({
  name,
  isDir,
  isExpanded = false,
  muted = false,
}: {
  name: string;
  isDir: boolean;
  isExpanded?: boolean;
  muted?: boolean;
}) {
  const href = getFileTypeIconHref(name, isDir, isExpanded);

  return (
    <svg
      className={cn(
        "file-type-icon size-4 shrink-0",
        muted && "file-type-icon--muted",
      )}
      aria-hidden="true"
      focusable="false"
    >
      <use href={href} xlinkHref={href} />
    </svg>
  );
}
