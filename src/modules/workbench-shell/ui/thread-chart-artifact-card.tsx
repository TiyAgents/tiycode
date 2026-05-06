import { useEffect, useRef, useState } from "react";
import { AlertCircleIcon, BarChart3Icon, CodeIcon, EyeIcon } from "lucide-react";
import { MessageResponse } from "@/components/ai-elements/message";
import type { SurfaceChartMessagePart } from "@/modules/workbench-shell/ui/runtime-thread-surface-state";
import { cn } from "@/shared/lib/utils";

type ThreadChartArtifactCardProps = {
  part: SurfaceChartMessagePart;
};

function getStatusLabel(status: SurfaceChartMessagePart["status"]) {
  switch (status) {
    case "loading":
      return "Preparing chart";
    case "error":
      return "Chart unavailable";
    default:
      return "Chart artifact";
  }
}

function VegaLiteRenderer({ spec }: { spec: unknown }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function render() {
      if (!containerRef.current) return;
      try {
        const vegaEmbed = (await import("vega-embed")).default;
        if (cancelled) return;
        const result = await vegaEmbed(containerRef.current, spec as object, {
          actions: { export: true, source: false, compiled: false, editor: false },
          theme: "dark",
          renderer: "svg",
        });
        if (cancelled) {
          result.finalize();
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    }

    void render();
    return () => { cancelled = true; };
  }, [spec]);

  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-xl border border-app-danger/25 bg-app-danger/8 px-3 py-2 text-sm text-app-danger">
        <AlertCircleIcon className="mt-0.5 size-4 shrink-0" />
        <span>Failed to render chart: {error}</span>
      </div>
    );
  }

  return <div ref={containerRef} className="w-full overflow-x-auto [&_.vega-embed]:!w-full" />;
}

export function ThreadChartArtifactCard({ part }: ThreadChartArtifactCardProps) {
  const [showSpec, setShowSpec] = useState(false);
  const specText = JSON.stringify(part.spec, null, 2);

  return (
    <div className="overflow-hidden rounded-2xl border border-app-border/30 bg-app-surface/18 shadow-sm">
      <div className="flex items-start justify-between gap-3 border-b border-app-border/20 px-4 py-3">
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">
            <BarChart3Icon className="size-3.5" />
            <span>{getStatusLabel(part.status)}</span>
            <span className="rounded-full border border-app-border/30 px-2 py-0.5 normal-case tracking-normal text-app-muted">
              {part.library}
            </span>
          </div>
          {part.title ? (
            <div className="text-sm font-medium text-app-foreground">{part.title}</div>
          ) : null}
          {part.caption ? (
            <p className="text-sm leading-6 text-app-muted">{part.caption}</p>
          ) : null}
        </div>
        <button
          className="shrink-0 rounded-lg p-1.5 text-app-subtle transition-colors hover:bg-app-surface/50 hover:text-app-foreground"
          onClick={() => setShowSpec((v) => !v)}
          title={showSpec ? "Show chart" : "Show spec"}
          type="button"
        >
          {showSpec ? <EyeIcon className="size-4" /> : <CodeIcon className="size-4" />}
        </button>
      </div>

      <div className="space-y-3 px-4 py-4">
        {part.error ? (
          <div className="flex items-start gap-2 rounded-xl border border-app-danger/25 bg-app-danger/8 px-3 py-2 text-sm text-app-danger">
            <AlertCircleIcon className="mt-0.5 size-4 shrink-0" />
            <span>{part.error}</span>
          </div>
        ) : null}

        {part.status === "loading" ? (
          <div className="flex h-48 items-center justify-center rounded-xl border border-dashed border-app-border/30 bg-app-surface/35">
            <span className="text-sm text-app-subtle animate-pulse">Generating chart…</span>
          </div>
        ) : showSpec ? (
          <div className="rounded-xl bg-app-surface/45 px-3 py-3 text-sm text-app-muted">
            <div className="mb-2 text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">
              Spec preview
            </div>
            <MessageResponse>{`\`\`\`json\n${specText}\n\`\`\``}</MessageResponse>
          </div>
        ) : (
          <div
            className={cn(
              "rounded-xl border px-2 py-2",
              part.status === "error"
                ? "border-app-danger/25 bg-app-danger/5"
                : "border-app-border/20 bg-app-surface/35",
            )}
          >
            <VegaLiteRenderer spec={part.spec} />
          </div>
        )}
      </div>
    </div>
  );
}
