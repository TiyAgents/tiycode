import type { CSSProperties } from "react";
import { cn } from "@/shared/lib/utils";

type LocalLlmIconProps = {
  className?: string;
  slug: string;
  title?: string;
};

export function LocalLlmIcon({ className, slug, title }: LocalLlmIconProps) {
  const accessibilityProps = title
    ? ({ "aria-label": title, role: "img" } as const)
    : ({ "aria-hidden": true } as const);

  return (
    <span
      {...accessibilityProps}
      className={cn("inline-flex shrink-0 bg-current", className)}
      style={getLocalLlmIconStyle(`/llm-icons/${slug}.svg`)}
    />
  );
}

function getLocalLlmIconStyle(iconUrl: string): CSSProperties {
  return {
    WebkitMaskImage: `url("${iconUrl}")`,
    WebkitMaskPosition: "center",
    WebkitMaskRepeat: "no-repeat",
    WebkitMaskSize: "contain",
    backgroundColor: "currentColor",
    lineHeight: 0,
    maskImage: `url("${iconUrl}")`,
    maskPosition: "center",
    maskRepeat: "no-repeat",
    maskSize: "contain",
  };
}
