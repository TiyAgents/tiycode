import { type ReactNode, useEffect, useState } from "react";
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  Code2,
  FileSearch,
  Plus,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { useT, type TranslationKey } from "@/i18n";
import type {
  CustomSubagent,
  CustomSubagentModelRole,
} from "@/modules/settings-center/model/types";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import {
  customSubagentCreate,
  customSubagentDelete,
  customSubagentUpdate,
  type CustomSubagentInput,
} from "@/services/bridge/subagent-commands";
import { getInvokeErrorMessage, formatInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Separator } from "@/shared/ui/separator";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";

const TOOL_CATEGORIES: Array<{ labelKey: TranslationKey; tools: string[] }> = [
  {
    labelKey: "settings.agents.toolCategory.fileRead",
    tools: ["read", "list", "search", "find"],
  },
  {
    labelKey: "settings.agents.toolCategory.web",
    tools: ["web_search"],
  },
  {
    labelKey: "settings.agents.toolCategory.fileWrite",
    tools: ["edit", "write"],
  },
  {
    labelKey: "settings.agents.toolCategory.terminal",
    tools: ["shell", "term_status", "term_output", "term_write", "term_restart", "term_close"],
  },
  {
    labelKey: "settings.agents.toolCategory.git",
    tools: ["git_status", "git_diff", "git_log"],
  },
];

const BUILT_IN_AGENTS: Array<{
  nameKey: TranslationKey;
  descriptionKey: TranslationKey;
  icon: typeof FileSearch;
}> = [
  {
    nameKey: "settings.agents.builtIn.explore.name",
    descriptionKey: "settings.agents.builtIn.explore.desc",
    icon: FileSearch,
  },
  {
    nameKey: "settings.agents.builtIn.review.name",
    descriptionKey: "settings.agents.builtIn.review.desc",
    icon: CheckCircle2,
  },
];

const MODEL_ROLE_OPTIONS: Array<{
  value: CustomSubagentModelRole;
  labelKey: TranslationKey;
  descriptionKey: TranslationKey;
}> = [
  {
    value: "primary",
    labelKey: "settings.agents.modelRole.primary",
    descriptionKey: "settings.agents.modelRole.primaryDesc",
  },
  {
    value: "auxiliary",
    labelKey: "settings.agents.modelRole.auxiliary",
    descriptionKey: "settings.agents.modelRole.auxiliaryDesc",
  },
  {
    value: "lightweight",
    labelKey: "settings.agents.modelRole.lightweight",
    descriptionKey: "settings.agents.modelRole.lightweightDesc",
  },
];

function SettingsPanelSection({
  action,
  children,
  title,
}: {
  action?: ReactNode;
  children: ReactNode;
  title: string;
}) {
  return (
    <section>
      <div className="mb-2 flex items-center justify-between px-1">
        <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">{title}</h2>
        {action ?? null}
      </div>
      <div className="overflow-hidden rounded-2xl border border-app-border bg-app-surface">{children}</div>
    </section>
  );
}

function FieldGroup({
  children,
  description,
  title,
}: {
  children: ReactNode;
  description?: string;
  title: string;
}) {
  return (
    <div className="space-y-3">
      <div>
        <h3 className="text-[13px] font-medium text-app-foreground">{title}</h3>
        {description ? (
          <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
        ) : null}
      </div>
      {children}
    </div>
  );
}

type AgentsSettingsPanelProps = {
  customSubagents: CustomSubagent[];
  description?: string;
  onUnsavedChangesChange?: (dirty: boolean) => void;
};

type AgentPendingAction =
  | { type: "select"; id: string }
  | { type: "collapse" }
  | { type: "create" }
  | { type: "delete"; id: string };

export function AgentsSettingsPanel({
  customSubagents,
  description,
  onUnsavedChangesChange,
}: AgentsSettingsPanelProps) {
  const t = useT();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editState, setEditState] = useState<Partial<CustomSubagentInput> | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [pendingAction, setPendingAction] = useState<AgentPendingAction | null>(null);
  const [pageErrorMessage, setPageErrorMessage] = useState<string | null>(null);
  const [editorErrorMessage, setEditorErrorMessage] = useState<string | null>(null);

  const selectedAgent = customSubagents.find((a) => a.id === selectedId);
  const hasUnsavedChanges = editState !== null;

  // Notify parent when unsaved-changes state changes so it can block navigation.
  useEffect(() => {
    onUnsavedChangesChange?.(hasUnsavedChanges);
  }, [hasUnsavedChanges, onUnsavedChangesChange]);

  const modelRoleLabel = (role: CustomSubagentModelRole | undefined) => {
    const option = MODEL_ROLE_OPTIONS.find((entry) => entry.value === (role ?? "auxiliary"));
    return t(option?.labelKey ?? "settings.agents.modelRole.auxiliary");
  };

  const getSubagentErrorMessage = (error: unknown, fallback: string, slug?: string) => {
    const errorCode =
      typeof error === "object" && error !== null ? Reflect.get(error, "errorCode") : null;
    if (errorCode === "custom_subagent.slug_conflict") {
      return t("settings.agents.errorSlugConflict", { slug: slug ?? "" });
    }
    return formatInvokeErrorMessage(error) ?? fallback;
  };

  const createAgent = async () => {
    const input: CustomSubagentInput = {
      name: t("settings.agents.newAgentName"),
      slug: `agent-${Date.now().toString(36)}`,
      systemPrompt: t("settings.agents.defaultSystemPrompt"),
      invocationDescription: t("settings.agents.defaultInvocationDescription"),
      allowedTools: ["read", "list", "search", "find"],
      modelRole: "auxiliary",
      isEnabled: true,
    };
    try {
      const created = await customSubagentCreate(input);
      settingsStore.setState({
        customSubagents: [...settingsStore.getState().customSubagents, created],
      });
      setSelectedId(created.id);
      setEditState(null);
    } catch (error) {
      console.error("Failed to create subagent", error);
      setPageErrorMessage(getSubagentErrorMessage(error, t("settings.agents.errorCreate"), input.slug));
    }
  };

  const deleteAgent = async (id: string) => {
    try {
      await customSubagentDelete(id);
      const updated = settingsStore.getState().customSubagents.filter((a) => a.id !== id);
      settingsStore.setState({ customSubagents: updated });
      if (selectedId === id) {
        setSelectedId(null);
        setEditState(null);
      }
    } catch (error) {
      console.error("Failed to delete subagent", error);
      setPageErrorMessage(getInvokeErrorMessage(error, t("settings.agents.errorDelete")));
    }
  };

  const performAction = (action: AgentPendingAction) => {
    setPageErrorMessage(null);
    setEditorErrorMessage(null);
    switch (action.type) {
      case "select":
        setSelectedId(action.id);
        setEditState(null);
        break;
      case "collapse":
        setSelectedId(null);
        setEditState(null);
        break;
      case "create":
        void createAgent();
        break;
      case "delete":
        void deleteAgent(action.id);
        break;
    }
  };

  const requestAction = (action: AgentPendingAction) => {
    if (hasUnsavedChanges) {
      setPendingAction(action);
      return;
    }
    performAction(action);
  };

  const handleCreate = () => {
    requestAction({ type: "create" });
  };

  const handleDelete = (id: string) => {
    requestAction({ type: "delete", id });
  };

  const handleAgentToggle = (id: string) => {
    requestAction(selectedId === id ? { type: "collapse" } : { type: "select", id });
  };

  const handleDiscardChanges = () => {
    const action = pendingAction;
    if (!action) return;
    setPendingAction(null);
    setEditState(null);
    performAction(action);
  };

  const handleSave = async () => {
    if (!selectedAgent || !editState) return;
    const input: CustomSubagentInput = {
      name: editState.name !== undefined ? editState.name : selectedAgent.name,
      slug: editState.slug !== undefined ? editState.slug : selectedAgent.slug,
      systemPrompt: editState.systemPrompt !== undefined ? editState.systemPrompt : selectedAgent.systemPrompt,
      invocationDescription: editState.invocationDescription !== undefined ? editState.invocationDescription : selectedAgent.invocationDescription,
      allowedTools: editState.allowedTools ?? selectedAgent.allowedTools,
      modelRole: editState.modelRole ?? selectedAgent.modelRole ?? "auxiliary",
      isEnabled: editState.isEnabled ?? selectedAgent.isEnabled,
    };
    setEditorErrorMessage(null);
    setIsSaving(true);
    try {
      const updated = await customSubagentUpdate(selectedAgent.id, input);
      const agents = settingsStore.getState().customSubagents.map((a) =>
        a.id === updated.id ? updated : a,
      );
      settingsStore.setState({ customSubagents: agents });
      setEditState(null);
    } catch (error) {
      console.error("Failed to update subagent", error);
      setEditorErrorMessage(getSubagentErrorMessage(error, t("settings.agents.errorSave"), input.slug));
    } finally {
      setIsSaving(false);
    }
  };

  const currentValue = (key: keyof CustomSubagentInput) => {
    if (editState && key in editState) return editState[key];
    if (selectedAgent) return selectedAgent[key as keyof CustomSubagent];
    return "";
  };

  const updateField = (key: keyof CustomSubagentInput, value: unknown) => {
    setEditorErrorMessage(null);
    setEditState((prev) => ({ ...prev, [key]: value }));
  };

  const toggleTool = (toolName: string) => {
    const current = (currentValue("allowedTools") as string[]) ?? [];
    const next = current.includes(toolName)
      ? current.filter((tool) => tool !== toolName)
      : [...current, toolName];
    updateField("allowedTools", next);
  };

  const renderAgentEditor = () => {
    if (!selectedAgent) return null;
    const selectedModelRole = ((currentValue("modelRole") as CustomSubagentModelRole | undefined) ?? "auxiliary");

    return (
      <div
        id={`agent-editor-${selectedAgent.id}`}
        className="border-t border-app-border bg-app-surface-muted/40 px-4 py-4"
      >
        <div className="space-y-5 rounded-xl border border-app-border bg-app-surface/85 p-4">
          <div className="space-y-2">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="flex min-w-0 flex-1 flex-wrap items-start gap-3">
                <div className="min-w-0 shrink-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-[15px] font-semibold text-app-foreground">{selectedAgent.name}</h3>
                    {hasUnsavedChanges ? (
                      <span className="inline-flex items-center gap-1 rounded-full border border-app-warning/30 bg-app-warning/10 px-2 py-0.5 text-[11px] font-medium text-app-warning">
                        <AlertTriangle className="size-3" />
                        {t("settings.agents.unsavedChanges")}
                      </span>
                    ) : (
                      <span className="rounded-full border border-app-border bg-app-surface-muted px-2 py-0.5 text-[11px] font-medium text-app-subtle">
                        {t("settings.agents.saved")}
                      </span>
                    )}
                  </div>
                </div>
                {editorErrorMessage ? (
                  <div className="flex min-w-[240px] flex-1 items-center gap-2 py-0.5 text-[14px] font-semibold text-app-error">
                    <AlertTriangle className="size-4 shrink-0" />
                    <span className="min-w-0 truncate">{editorErrorMessage}</span>
                    <button
                      type="button"
                      aria-label={t("topBar.close")}
                      className="ml-auto shrink-0 text-app-error/70 hover:text-app-error"
                      onClick={() => setEditorErrorMessage(null)}
                    >
                      <X className="size-3.5" />
                    </button>
                  </div>
                ) : null}
              </div>
              <div className="flex shrink-0 flex-wrap items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  onClick={handleSave}
                  disabled={!hasUnsavedChanges || isSaving}
                >
                  {isSaving ? t("settings.agents.saving") : t("settings.agents.saveChanges")}
                </Button>
              </div>
            </div>
            <p className="text-[12px] leading-5 text-app-muted">
              {t("settings.agents.editorDescription")}
            </p>
          </div>

          <FieldGroup
            title={t("settings.agents.identity")}
            description={t("settings.agents.identityDesc")}
          >
            <div className="grid gap-3 md:grid-cols-2">
              <label className="space-y-1.5">
                <span className="text-[12px] font-medium text-app-foreground">{t("settings.agents.name")}</span>
                <Input
                  type="text"
                  value={(currentValue("name") as string) ?? ""}
                  onChange={(event) => updateField("name", event.target.value)}
                />
              </label>
              <label className="space-y-1.5">
                <span className="text-[12px] font-medium text-app-foreground">{t("settings.agents.slug")}</span>
                <Input
                  type="text"
                  value={(currentValue("slug") as string) ?? ""}
                  onChange={(event) =>
                    updateField(
                      "slug",
                      event.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
                    )
                  }
                  className="font-mono"
                />
                <span className="block truncate text-[11px] text-app-subtle">
                  {t("settings.agents.toolName", { slug: (currentValue("slug") as string) ?? "" })}
                </span>
              </label>
            </div>
            <div className="flex items-center justify-between gap-3 rounded-xl border border-app-border bg-app-surface-muted px-3 py-2.5">
              <label
                htmlFor={`agent-enabled-${selectedAgent.id}`}
                className="min-w-0 text-[13px] font-medium text-app-foreground"
              >
                {t("settings.agents.enabled")}
                <span className="mt-0.5 block text-[12px] font-normal leading-5 text-app-muted">
                  {t("settings.agents.enabledDesc")}
                </span>
              </label>
              <Switch
                id={`agent-enabled-${selectedAgent.id}`}
                size="sm"
                checked={(currentValue("isEnabled") as boolean) ?? true}
                onCheckedChange={(checked) => updateField("isEnabled", checked)}
              />
            </div>
          </FieldGroup>

          <Separator />

          <FieldGroup
            title={t("settings.agents.modelLevel")}
            description={t("settings.agents.modelLevelDesc")}
          >
            <div className="grid gap-2 md:grid-cols-3">
              {MODEL_ROLE_OPTIONS.map((option) => {
                const isSelected = selectedModelRole === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => updateField("modelRole", option.value)}
                    className={cn(
                      "rounded-xl border p-3 text-left transition-colors",
                      isSelected
                        ? "border-app-info/40 bg-app-info/10 text-app-foreground"
                        : "border-app-border bg-app-surface-muted text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                    )}
                  >
                    <span className="block text-[12px] font-medium">{t(option.labelKey)}</span>
                    <span className="mt-1 block text-[11px] leading-5 text-app-muted">
                      {t(option.descriptionKey)}
                    </span>
                  </button>
                );
              })}
            </div>
          </FieldGroup>

          <Separator />

          <FieldGroup
            title={t("settings.agents.behavior")}
            description={t("settings.agents.behaviorDesc")}
          >
            <label className="block space-y-1.5">
              <span className="text-[12px] font-medium text-app-foreground">{t("settings.agents.invocationDescription")}</span>
              <Textarea
                value={(currentValue("invocationDescription") as string) ?? ""}
                onChange={(event) => updateField("invocationDescription", event.target.value)}
                className="min-h-24"
              />
              <span className="block text-[11px] text-app-subtle">
                {t("settings.agents.invocationDescriptionHint")}
              </span>
            </label>
            <label className="block space-y-1.5">
              <span className="text-[12px] font-medium text-app-foreground">{t("settings.agents.systemPrompt")}</span>
              <Textarea
                value={(currentValue("systemPrompt") as string) ?? ""}
                onChange={(event) => updateField("systemPrompt", event.target.value)}
                className="h-56 min-h-56 resize-none overflow-y-auto font-mono [field-sizing:fixed]"
              />
            </label>
          </FieldGroup>

          <Separator />

          <FieldGroup
            title={t("settings.agents.allowedTools")}
            description={t("settings.agents.allowedToolsDesc")}
          >
            <div className="space-y-4">
              {TOOL_CATEGORIES.map((category) => (
                <div key={category.labelKey} className="space-y-2">
                  <div className="flex items-center gap-2 text-[12px] font-medium text-app-muted">
                    <Wrench className="size-3.5" />
                    {t(category.labelKey)}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {category.tools.map((tool) => {
                      const checked = ((currentValue("allowedTools") as string[]) ?? []).includes(tool);
                      return (
                        <label key={tool} className="cursor-pointer">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => toggleTool(tool)}
                            className="peer sr-only"
                          />
                          <span
                            className={cn(
                              "inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-[12px] font-medium transition-colors",
                              checked
                                ? "border-app-info/40 bg-app-info/10 text-app-foreground"
                                : "border-app-border bg-app-surface-muted text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                              "peer-focus-visible:ring-2 peer-focus-visible:ring-app-info/50",
                            )}
                          >
                            <Code2 className="size-3.5" />
                            <code>{tool}</code>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          </FieldGroup>
        </div>
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-[19px] font-semibold text-app-foreground">{t("settings.category.agents")}</h1>
        <p className="mt-1 text-[12px] leading-5 text-app-muted">
          {description ?? t("settings.category.agentsDesc")}
        </p>
      </div>

      {pageErrorMessage && (
        <div className="flex items-center gap-2 rounded-lg border border-app-error/30 bg-app-error/10 px-3 py-2 text-[12px] text-app-error">
          <AlertTriangle className="size-3.5 shrink-0" />
          <span className="min-w-0">{pageErrorMessage}</span>
          <button
            type="button"
            aria-label={t("topBar.close")}
            className="ml-auto shrink-0 text-app-muted hover:text-app-foreground"
            onClick={() => setPageErrorMessage(null)}
          >
            <X className="size-3.5" />
          </button>
        </div>
      )}

      <SettingsPanelSection title={t("settings.agents.builtInAgents")}>
        <div className="grid gap-3 p-3 md:grid-cols-2">
          {BUILT_IN_AGENTS.map((agent) => {
            const Icon = agent.icon;
            const name = t(agent.nameKey);
            return (
              <div
                key={agent.nameKey}
                className="rounded-xl border border-app-border bg-app-surface-muted p-3"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface text-app-info">
                      <Icon className="size-4" />
                    </span>
                    <div className="min-w-0">
                      <p className="text-[13px] font-medium text-app-foreground">{name}</p>
                      <p className="mt-0.5 text-[11px] text-app-subtle">{t("settings.agents.alwaysAvailable")}</p>
                    </div>
                  </div>
                  <span className="rounded-full border border-app-border bg-app-surface px-2 py-0.5 text-[11px] font-medium text-app-muted">
                    {t("settings.agents.builtInBadge")}
                  </span>
                </div>
                <p className="mt-3 text-[12px] leading-5 text-app-muted">{t(agent.descriptionKey)}</p>
              </div>
            );
          })}
        </div>
      </SettingsPanelSection>

      <SettingsPanelSection
        title={t("settings.agents.customAgents")}
        action={
          <Button type="button" variant="outline" size="sm" onClick={handleCreate}>
            <Plus className="size-3.5" />
            {t("settings.agents.newAgent")}
          </Button>
        }
      >
        <div className="divide-y divide-app-border">
          {customSubagents.length > 0 ? (
            customSubagents.map((agent) => {
              const isSelected = agent.id === selectedId;
              const actionLabel = isSelected ? t("settings.agents.collapse") : t("settings.agents.edit");
              return (
                <div
                  key={agent.id}
                  className={cn(
                    "bg-app-surface transition-colors",
                    isSelected && "bg-app-surface-active",
                  )}
                >
                  <div className="group flex items-stretch transition-colors hover:bg-app-surface-hover">
                    <button
                      type="button"
                      onClick={() => handleAgentToggle(agent.id)}
                      className="flex min-w-0 flex-1 items-center gap-3 px-4 py-3 text-left outline-none transition-colors focus-visible:bg-app-surface-hover"
                      aria-expanded={isSelected}
                      aria-controls={isSelected ? `agent-editor-${agent.id}` : undefined}
                      aria-label={`${actionLabel} ${agent.name}`}
                    >
                      <span
                        className={cn(
                          "flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-muted",
                          isSelected && "border-app-info/40 bg-app-info/10 text-app-info",
                        )}
                      >
                        <Bot className="size-4" />
                      </span>
                      <span className="min-w-0">
                        <span className="flex flex-wrap items-center gap-2">
                          <span
                            className={cn(
                              "truncate text-[13px] font-medium text-app-foreground",
                              !agent.isEnabled && "text-app-muted line-through",
                            )}
                          >
                            {agent.name}
                          </span>
                          <span
                            className={cn(
                              "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                              agent.isEnabled
                                ? "border-app-info/30 bg-app-info/10 text-app-info"
                                : "border-app-border bg-app-surface-muted text-app-subtle",
                            )}
                          >
                            {agent.isEnabled ? t("settings.agents.enabled") : t("settings.agents.disabled")}
                          </span>
                          <span className="rounded-full border border-app-border bg-app-surface-muted px-2 py-0.5 text-[11px] font-medium text-app-subtle">
                            {modelRoleLabel(agent.modelRole)}
                          </span>
                          {isSelected && hasUnsavedChanges ? (
                            <span className="rounded-full border border-app-warning/30 bg-app-warning/10 px-2 py-0.5 text-[11px] font-medium text-app-warning">
                              {t("settings.agents.unsaved")}
                            </span>
                          ) : null}
                        </span>
                        <code className="mt-1 block truncate text-[12px] text-app-subtle">
                          agent_{agent.slug}
                        </code>
                        {agent.invocationDescription ? (
                          <p className="mt-1 truncate text-[12px] text-app-muted">
                            {agent.invocationDescription}
                          </p>
                        ) : null}
                      </span>
                    </button>

                    <div className="flex shrink-0 items-center gap-1.5 px-4 py-3 pl-0">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        onClick={() => handleDelete(agent.id)}
                        className="shrink-0 text-app-muted opacity-0 hover:bg-app-danger/10 hover:text-app-danger focus-visible:opacity-100 group-hover:opacity-100"
                        title={t("settings.agents.deleteAgent", { name: agent.name })}
                        aria-label={t("settings.agents.deleteAgent", { name: agent.name })}
                      >
                        <Trash2 className="size-4" />
                      </Button>
                    </div>
                  </div>

                  {isSelected ? renderAgentEditor() : null}
                </div>
              );
            })
          ) : (
            <div className="flex flex-col items-center justify-center px-4 py-10 text-center">
              <span className="flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted text-app-muted">
                <Bot className="size-5" />
              </span>
              <h3 className="mt-3 text-[13px] font-medium text-app-foreground">{t("settings.agents.emptyTitle")}</h3>
              <p className="mt-1 max-w-sm text-[12px] leading-5 text-app-muted">
                {t("settings.agents.emptyDesc")}
              </p>
              <Button type="button" variant="outline" size="sm" onClick={handleCreate} className="mt-4">
                <Plus className="size-3.5" />
                {t("settings.agents.createAgent")}
              </Button>
            </div>
          )}
        </div>
      </SettingsPanelSection>

      <Dialog
        open={pendingAction !== null}
        onOpenChange={(open) => {
          if (!open) setPendingAction(null);
        }}
      >
        <DialogContent className="border-app-border bg-app-surface text-app-foreground sm:max-w-md">
          <DialogHeader>
            <div className="flex items-start gap-3">
              <span className="flex size-9 shrink-0 items-center justify-center rounded-xl border border-app-warning/30 bg-app-warning/10 text-app-warning">
                <AlertTriangle className="size-4" />
              </span>
              <div className="min-w-0">
                <DialogTitle>{t("settings.agents.discardTitle")}</DialogTitle>
                <DialogDescription className="mt-2 text-[12px] leading-5 text-app-muted">
                  {t("settings.agents.discardDesc")}
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setPendingAction(null)}>
              {t("settings.agents.continueEditing")}
            </Button>
            <Button type="button" variant="destructive" onClick={handleDiscardChanges}>
              {t("settings.agents.discardChanges")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
