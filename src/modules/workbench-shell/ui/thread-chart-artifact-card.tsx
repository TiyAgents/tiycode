import { Component, useEffect, useRef, useState } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { AlertCircleIcon, BarChart3Icon, CodeIcon, EyeIcon } from "lucide-react";
import { MessageResponse } from "@/components/ai-elements/message";
import type { SurfaceChartMessagePart } from "@/modules/workbench-shell/ui/runtime-thread-surface-state";
import { cn } from "@/shared/lib/utils";

type ThreadChartArtifactCardProps = {
  part: SurfaceChartMessagePart;
};

const MAX_SPEC_SIZE_BYTES = 512_000;
const MAX_DATA_POINTS = 50_000;

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

function validateSpec(spec: unknown): string | null {
  if (!spec || typeof spec !== "object") {
    return "Spec must be a non-null object";
  }

  const specStr = JSON.stringify(spec);
  if (specStr.length > MAX_SPEC_SIZE_BYTES) {
    return `Spec exceeds maximum size (${Math.round(specStr.length / 1024)}KB > ${MAX_SPEC_SIZE_BYTES / 1024}KB)`;
  }

  const record = spec as Record<string, unknown>;
  if (!record.mark && !record.layer && !record.concat && !record.hconcat && !record.vconcat && !record.facet && !record.repeat) {
    return "Spec must include 'mark', 'layer', or a composition operator";
  }

  const data = record.data as Record<string, unknown> | undefined;
  if (data && "values" in data && Array.isArray(data.values)) {
    if (data.values.length > MAX_DATA_POINTS) {
      return `Data exceeds maximum points (${data.values.length} > ${MAX_DATA_POINTS})`;
    }
  }

  return null;
}

class ChartErrorBoundary extends Component<
  { children: ReactNode; fallback: (error: string) => ReactNode },
  { error: string | null }
> {
  state: { error: string | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error: error.message };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[ChartErrorBoundary]", error, info);
  }

  render() {
    if (this.state.error) {
      return this.props.fallback(this.state.error);
    }
    return this.props.children;
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
          width: containerRef.current.clientWidth - 32,
          config: {
            autosize: { type: "fit", contains: "padding" },
          },
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

function ChartErrorFallback({ message }: { message: string }) {
  return (
    <div className="flex items-start gap-2 rounded-xl border border-app-danger/25 bg-app-danger/8 px-3 py-2 text-sm text-app-danger">
      <AlertCircleIcon className="mt-0.5 size-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}

export function ThreadChartArtifactCard({ part }: ThreadChartArtifactCardProps) {
  const [showSpec, setShowSpec] = useState(false);
  const specText = JSON.stringify(part.spec, null, 2);
  const validationError = part.status !== "loading" ? validateSpec(part.spec) : null;

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
          <ChartErrorFallback message={part.error} />
        ) : null}

        {validationError && !part.error ? (
          <ChartErrorFallback message={`Validation: ${validationError}`} />
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
        ) : validationError ? (
          <div className="rounded-xl bg-app-surface/45 px-3 py-3 text-sm text-app-muted">
            <MessageResponse>{`\`\`\`json\n${specText}\n\`\`\``}</MessageResponse>
          </div>
        ) : (
          <ChartErrorBoundary fallback={(err) => <ChartErrorFallback message={`Render crash: ${err}`} />}>
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
          </ChartErrorBoundary>
        )}
      </div>
    </div>
  );
}
