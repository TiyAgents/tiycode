// Metadata parsing types and functions extracted from runtime-thread-surface.tsx

import type { TranslationKey } from "@/i18n";

export type PlanApprovalAction = "apply_plan" | "apply_plan_with_context_reset";

export type PlanStepMetadata = {
  description?: string;
  files?: string[];
  id?: string;
  status?: string;
  title?: string;
};

export type PlanMessageMetadata = {
  approvalState?: string;
  assumptions?: string[];
  context?: string;
  design?: string;
  generatedFromRunId?: string;
  kind?: string;
  keyImplementation?: string;
  planRevision?: number;
  risks?: string[];
  runModeAtCreation?: string;
  steps?: PlanStepMetadata[];
  summary?: string;
  title?: string;
  verification?: string;
};

export type FormattedPlan = {
  approvalState: string | null;
  assumptions: string[];
  context: string;
  design: string;
  keyImplementation: string;
  planRevision: number | null;
  risks: string[];
  steps: string[];
  summary: string;
  title: string;
  verification: string;
};

export type FormattedApprovalPrompt = {
  approvedAction: PlanApprovalAction | null;
  options: Array<{ action: PlanApprovalAction; label: string }>;
  planMessageId: string | null;
  planRevision: number | null;
  state: string;
};

export type ClarifyOption = {
  description: string;
  id: string;
  label: string;
  recommended: boolean;
};

export type ClarifyPrompt = {
  header: string | null;
  options: ClarifyOption[];
  question: string;
};


export function asObjectRecord(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

export function readStringField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "string" ? value : null;
}

export function readNumberField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  return typeof value === "number" ? value : null;
}

export function readStringArrayField(record: Record<string, unknown> | null, key: string) {
  const value = record?.[key];
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0);
}

export function parsePlanMessageMetadata(value: unknown): PlanMessageMetadata | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const stepEntries = Array.isArray(record.steps)
    ? record.steps
        .map<PlanStepMetadata | null>((step) => {
          if (typeof step === "string") {
            return { title: step };
          }

          const stepRecord = asObjectRecord(step);
          if (!stepRecord) {
            return null;
          }

          return {
            description: readStringField(stepRecord, "description") ?? undefined,
            files: readStringArrayField(stepRecord, "files"),
            id: readStringField(stepRecord, "id") ?? undefined,
            status: readStringField(stepRecord, "status") ?? undefined,
            title: readStringField(stepRecord, "title") ?? undefined,
          };
        })
        .filter((step): step is PlanStepMetadata => step !== null)
    : [];

  return {
    approvalState: readStringField(record, "approvalState") ?? undefined,
    assumptions: readStringArrayField(record, "assumptions"),
    context: readStringField(record, "context") ?? undefined,
    design: readStringField(record, "design") ?? undefined,
    generatedFromRunId: readStringField(record, "generatedFromRunId") ?? undefined,
    kind: readStringField(record, "kind") ?? undefined,
    keyImplementation: readStringField(record, "keyImplementation") ?? undefined,
    planRevision: readNumberField(record, "planRevision") ?? undefined,
    risks: readStringArrayField(record, "risks"),
    runModeAtCreation: readStringField(record, "runModeAtCreation") ?? undefined,
    steps: stepEntries,
    summary: readStringField(record, "summary") ?? undefined,
    title: readStringField(record, "title") ?? undefined,
    verification: readStringField(record, "verification") ?? undefined,
  };
}

export function parseApprovalPromptMetadata(value: unknown, t: (key: TranslationKey) => string): FormattedApprovalPrompt | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const options = Array.isArray(record.options)
    ? record.options
        .map((entry) => {
          const optionRecord = asObjectRecord(entry);
          const action = readStringField(optionRecord, "action");
          const label = readStringField(optionRecord, "label");
          if (
            (action !== "apply_plan" && action !== "apply_plan_with_context_reset")
            || !label
          ) {
            return null;
          }

          return { action, label };
        })
        .filter((option): option is { action: PlanApprovalAction; label: string } => Boolean(option))
    : [];

  return {
    approvedAction:
      readStringField(record, "approvedAction") === "apply_plan"
      || readStringField(record, "approvedAction") === "apply_plan_with_context_reset"
        ? (readStringField(record, "approvedAction") as PlanApprovalAction)
        : null,
    options: options.length > 0
      ? options
      : [
          { action: "apply_plan", label: t("plan.implementAsPlan") },
          { action: "apply_plan_with_context_reset", label: t("plan.clearAndImplement") },
        ],
    planMessageId: readStringField(record, "planMessageId"),
    planRevision: readNumberField(record, "planRevision"),
    state: readStringField(record, "state") ?? "pending",
  };
}

export function parseCommandComposerMetadata(value: unknown): {
  kind: "plain" | "command";
  displayText: string | null;
  effectivePrompt: string | null;
} | null {
  const record = asObjectRecord(value);
  const composer = asObjectRecord(record?.composer);
  if (!composer) {
    return null;
  }

  const kind = readStringField(composer, "kind");
  if (kind !== "command" && kind !== "plain") {
    return null;
  }

  return {
    kind,
    displayText: readStringField(composer, "displayText"),
    effectivePrompt: readStringField(composer, "effectivePrompt"),
  };
}

export function parseSummaryMarkerMetadata(value: unknown): {
  kind: string | null;
  label: string | null;
  source: string | null;
} | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  return {
    kind: readStringField(record, "kind"),
    label: readStringField(record, "label"),
    source: readStringField(record, "source"),
  };
}

export function parseClarifyPrompt(value: unknown): ClarifyPrompt | null {
  const record = asObjectRecord(value);
  if (!record) {
    return null;
  }

  const question = readStringField(record, "question")?.trim();
  if (!question) {
    return null;
  }

  const options = Array.isArray(record.options)
    ? record.options
        .map((entry, index) => {
          const optionRecord = asObjectRecord(entry);
          const label = readStringField(optionRecord, "label")?.trim();
          const description = readStringField(optionRecord, "description")?.trim();
          if (!label || !description) {
            return null;
          }

          return {
            description,
            id: readStringField(optionRecord, "id")?.trim() || `option-${index + 1}`,
            label,
            recommended: optionRecord?.recommended === true,
          };
        })
        .filter((option): option is ClarifyOption => option !== null)
    : [];

  if (options.length < 2) {
    return null;
  }

  return {
    header: readStringField(record, "header")?.trim() || null,
    options,
    question,
  };
}

export function formatPlanMetadata(
  metadata: unknown,
  fallbackContent?: string,
): FormattedPlan {
  const parsed = parsePlanMessageMetadata(metadata);
  const record = asObjectRecord(metadata);
  const title = parsed?.title?.trim() || readStringField(record, "title")?.trim() || "Execution Plan";
  const summary =
    parsed?.summary?.trim()
    || readStringField(record, "description")?.trim()
    || readStringField(record, "overview")?.trim()
    || fallbackContent?.trim()
    || "Review the proposed implementation plan before coding.";
  const stepsSource = parsed?.steps ?? [];
  const steps = stepsSource
    .map((step) => {
      const stepTitle = step.title?.trim();
      const stepDescription = step.description?.trim();
      const files = step.files?.filter((file) => file.trim().length > 0) ?? [];
      if (!stepTitle && !stepDescription && files.length === 0) {
        return null;
      }

      const fragments = [stepTitle ?? null, stepDescription ?? null, files.length > 0 ? `(${files.join(", ")})` : null]
        .filter((fragment): fragment is string => Boolean(fragment));
      return fragments.join(" — ").replace(" — (", " (");
    })
    .filter((step): step is string => Boolean(step));

  return {
    approvalState: parsed?.approvalState ?? null,
    assumptions: parsed?.assumptions ?? [],
    context: parsed?.context?.trim() ?? "",
    design: parsed?.design?.trim() ?? "",
    keyImplementation: parsed?.keyImplementation?.trim() ?? "",
    planRevision: parsed?.planRevision ?? null,
    risks: parsed?.risks ?? [],
    steps,
    summary,
    title,
    verification: parsed?.verification?.trim() ?? "",
  };
}
