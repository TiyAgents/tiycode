import { PartyPopper, Lightbulb } from "lucide-react";
import { useT } from "@/i18n";

export function CompleteStep() {
  const t = useT();

  return (
    <div className="flex flex-col items-center gap-6 py-4 text-center">
      <div className="flex size-16 items-center justify-center rounded-2xl bg-app-success/12 shadow-[0_10px_28px_rgba(15,23,42,0.08)] dark:shadow-[0_14px_30px_rgba(0,0,0,0.24)]">
        <PartyPopper className="size-7 text-app-success" />
      </div>

      <div className="space-y-2">
        <h2 className="text-xl font-semibold tracking-[-0.02em] text-app-foreground">
          {t("onboarding.complete.title")}
        </h2>
        <p className="mx-auto max-w-sm text-[14px] leading-6 text-app-muted">
          {t("onboarding.complete.desc")}
        </p>
      </div>

      <div className="flex items-start gap-2.5 rounded-xl border border-app-border bg-app-surface-muted/50 px-4 py-3 text-left">
        <Lightbulb className="mt-0.5 size-4 shrink-0 text-app-warning" />
        <p className="text-[13px] leading-5 text-app-muted">
          {t("onboarding.complete.tip")}
        </p>
      </div>
    </div>
  );
}
