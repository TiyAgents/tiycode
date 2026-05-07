import { Component, useEffect, useMemo, useRef, useState } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { AlertCircleIcon, BarChart3Icon, CircleXIcon, CodeIcon, EyeIcon } from "lucide-react";
import { useTheme } from "@/app/providers/theme-provider";
import { CodeBlock } from "@/components/ai-elements/code-block";
import { MessageResponse } from "@/components/ai-elements/message";
import type { SurfaceChartMessagePart } from "@/modules/workbench-shell/ui/runtime-thread-surface-state";
import { cn } from "@/shared/lib/utils";
import { validateSpec } from "@/modules/workbench-shell/ui/chart-spec-validation";

type ThreadChartArtifactCardProps = {
  part: SurfaceChartMessagePart;
};

function getStatusLabel(status: SurfaceChartMessagePart["status"], library: string) {
  if (status === "loading") return "Preparing…";
  if (status === "error") return "Unavailable";
  switch (library) {
    case "html":
      return "HTML artifact";
    case "svg":
      return "SVG artifact";
    default:
      return "Chart artifact";
  }
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

const vegaEmbedPromise = import("vega-embed");

function VegaLiteRenderer({ spec }: { spec: unknown }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const { resolvedTheme } = useTheme();

  useEffect(() => {
    let cancelled = false;

    async function render() {
      if (!containerRef.current) return;
      try {
        const vegaEmbed = (await vegaEmbedPromise).default;
        if (cancelled) return;
        const result = await vegaEmbed(containerRef.current, spec as object, {
          actions: { export: true, source: false, compiled: false, editor: false },
          theme: resolvedTheme === "dark" ? "dark" : undefined,
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
  }, [spec, resolvedTheme]);

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

function HtmlSvgRenderer({ source }: { source: string; library: string }) {
  return (
    <CodeBlock
      code={source}
      contentClassName="max-h-[320px]"
      language="html"
    />
  );
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
  const isHtmlSvg = part.library === "html" || part.library === "svg";
  const [showSpec, setShowSpec] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const specText = useMemo(() => JSON.stringify(part.spec, null, 2), [part.spec]);
  const validationError = !isHtmlSvg && part.status !== "loading" ? validateSpec(part.spec) : null;

  return (
    <>
      <div className="overflow-hidden rounded-2xl border border-app-border/30 bg-app-surface/18 shadow-sm">
        <div className="flex items-start justify-between gap-3 border-b border-app-border/20 px-4 py-3">
          <div className="min-w-0 space-y-1">
            <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.08em] text-app-subtle">
              <BarChart3Icon className="size-3.5" />
              <span>{getStatusLabel(part.status, part.library)}</span>
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
          <div className="flex shrink-0 items-center gap-1.5">
            {isHtmlSvg && part.source ? (
              <button
                className="shrink-0 rounded-lg p-1.5 text-app-subtle transition-colors hover:bg-app-surface/50 hover:text-app-foreground"
                onClick={() => setPreviewOpen(true)}
                title="Preview"
                type="button"
              >
                <EyeIcon className="size-4" />
              </button>
            ) : null}
            {!isHtmlSvg && (
              <button
                className="shrink-0 rounded-lg p-1.5 text-app-subtle transition-colors hover:bg-app-surface/50 hover:text-app-foreground"
                onClick={() => setShowSpec((v) => !v)}
                title={showSpec ? "Show chart" : "Show spec"}
                type="button"
              >
                {showSpec ? <EyeIcon className="size-4" /> : <CodeIcon className="size-4" />}
              </button>
            )}
          </div>
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
              <span className="text-sm text-app-subtle animate-pulse">Generating…</span>
            </div>
          ) : isHtmlSvg ? (
            part.source ? (
              <HtmlSvgRenderer source={part.source} library={part.library} />
            ) : (
              <ChartErrorFallback message="No source content available" />
            )
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

      {isHtmlSvg && part.source && previewOpen ? (
          <div className="fixed inset-0 z-[80] flex items-center justify-center bg-app-chrome/50 px-6 py-12 backdrop-blur-sm">
            <div className="flex h-[min(82vh,860px)] w-full max-w-7xl flex-col overflow-hidden rounded-[24px] border border-app-border bg-app-surface shadow-[0_32px_96px_rgba(15,23,42,0.28)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.56)]">
              <div className="shrink-0 border-b border-app-border/30 px-5 py-4 text-left">
                <div className="flex items-center justify-between gap-4">
                  <h2 className="text-sm font-medium text-app-foreground">
                    {part.library.toUpperCase()} Preview
                  </h2>
                  <button
                    type="button"
                    aria-label="Close preview"
                    title="Close preview"
                    className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                    onClick={() => setPreviewOpen(false)}
                  >
                    <CircleXIcon className="size-4" />
                  </button>
                </div>
              </div>
              <div className="min-h-0 flex-1 overflow-auto bg-app-canvas/70 p-0">
                {part.library === "svg" ? (
                  <div
                    className="flex min-h-full w-full items-center justify-center p-6"
                    dangerouslySetInnerHTML={{ __html: part.source }}
                  />
                ) : (
                  <iframe
                    srcDoc={part.source}
                    sandbox="allow-scripts"
                    className="h-full min-h-full w-full border-0 bg-white"
                    title="HTML Preview"
                  />
                )}
              </div>
            </div>
          </div>
      ) : null}
    </>
  );
}
