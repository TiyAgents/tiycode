import { type ReactNode, useState } from "react";
import {
  Bot,
  CheckCircle2,
  Code2,
  FileSearch,
  Plus,
  Trash2,
  Wrench,
} from "lucide-react";
import type { CustomSubagent } from "@/modules/settings-center/model/types";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import {
  customSubagentCreate,
  customSubagentDelete,
  customSubagentUpdate,
  type CustomSubagentInput,
} from "@/services/bridge/subagent-commands";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Separator } from "@/shared/ui/separator";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";

// ---------------------------------------------------------------------------
// Tool categories for the checkbox UI
// ---------------------------------------------------------------------------

const TOOL_CATEGORIES = [
  {
    label: "File System (Read)",
    tools: ["read", "list", "search", "find"],
  },
  {
    label: "File System (Write)",
    tools: ["edit", "write"],
  },
  {
    label: "Terminal",
    tools: ["shell", "term_status", "term_output", "term_write", "term_restart", "term_close"],
  },
  {
    label: "Git",
    tools: ["git_status", "git_diff", "git_log"],
  },
];

const BUILT_IN_AGENTS = [
  {
    name: "Explore",
    description: "Read-only code investigation for mapping files, dependencies, and current behavior.",
    icon: FileSearch,
  },
  {
    name: "Review",
    description: "Focused code review and verification after an implementation is complete.",
    icon: CheckCircle2,
  },
];

// ---------------------------------------------------------------------------
// Local layout helpers matching Settings Center conventions
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

type AgentsSettingsPanelProps = {
  customSubagents: CustomSubagent[];
  description?: string;
};

export function AgentsSettingsPanel({
  customSubagents,
  description,
}: AgentsSettingsPanelProps) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editState, setEditState] = useState<Partial<CustomSubagentInput> | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const selectedAgent = customSubagents.find((a) => a.id === selectedId);

  const handleCreate = async () => {
    const input: CustomSubagentInput = {
      name: "New Agent",
      slug: `agent-${Date.now().toString(36)}`,
      systemPrompt: "You are a helpful assistant.",
      invocationDescription: "Describe when the main agent should use this subagent.",
      allowedTools: ["read", "list", "search", "find"],
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
    }
  };

  const handleDelete = async (id: string) => {
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
    }
  };

  const handleSave = async () => {
    if (!selectedAgent || !editState) return;
    setIsSaving(true);
    try {
      const input: CustomSubagentInput = {
        name: editState.name ?? selectedAgent.name,
        slug: editState.slug ?? selectedAgent.slug,
        systemPrompt: editState.systemPrompt ?? selectedAgent.systemPrompt,
        invocationDescription: editState.invocationDescription ?? selectedAgent.invocationDescription,
        allowedTools: editState.allowedTools ?? selectedAgent.allowedTools,
        isEnabled: editState.isEnabled ?? selectedAgent.isEnabled,
      };
      const updated = await customSubagentUpdate(selectedAgent.id, input);
      const agents = settingsStore.getState().customSubagents.map((a) =>
        a.id === updated.id ? updated : a,
      );
      settingsStore.setState({ customSubagents: agents });
      setEditState(null);
    } catch (error) {
      console.error("Failed to update subagent", error);
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
    setEditState((prev) => ({ ...prev, [key]: value }));
  };

  const toggleTool = (toolName: string) => {
    const current = (currentValue("allowedTools") as string[]) ?? [];
    const next = current.includes(toolName)
      ? current.filter((t) => t !== toolName)
      : [...current, toolName];
    updateField("allowedTools", next);
  };

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-[19px] font-semibold text-app-foreground">Agents</h1>
        <p className="mt-1 text-[12px] leading-5 text-app-muted">
          {description ?? "Create and manage custom sub-agents"}
        </p>
      </div>

      <SettingsPanelSection title="Built-in Agents">
        <div className="grid gap-3 p-3 md:grid-cols-2">
          {BUILT_IN_AGENTS.map((agent) => {
            const Icon = agent.icon;
            return (
              <div
                key={agent.name}
                className="rounded-xl border border-app-border bg-app-surface-muted p-3"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface text-app-info">
                      <Icon className="size-4" />
                    </span>
                    <div className="min-w-0">
                      <p className="text-[13px] font-medium text-app-foreground">{agent.name}</p>
                      <p className="mt-0.5 text-[11px] text-app-subtle">Always available</p>
                    </div>
                  </div>
                  <span className="rounded-full border border-app-border bg-app-surface px-2 py-0.5 text-[11px] font-medium text-app-muted">
                    Built-in
                  </span>
                </div>
                <p className="mt-3 text-[12px] leading-5 text-app-muted">{agent.description}</p>
              </div>
            );
          })}
        </div>
      </SettingsPanelSection>

      <SettingsPanelSection
        title="Custom Agents"
        action={
          <Button type="button" variant="outline" size="sm" onClick={handleCreate}>
            <Plus className="size-3.5" />
            New Agent
          </Button>
        }
      >
        <div className="divide-y divide-app-border">
          {customSubagents.length > 0 ? (
            customSubagents.map((agent) => {
              const isSelected = agent.id === selectedId;
              return (
                <div
                  key={agent.id}
                  role="button"
                  tabIndex={0}
                  onClick={() => {
                    setSelectedId(agent.id);
                    setEditState(null);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setSelectedId(agent.id);
                      setEditState(null);
                    }
                  }}
                  className={cn(
                    "group flex cursor-pointer items-center justify-between gap-3 px-4 py-3 text-left transition-colors outline-none hover:bg-app-surface-hover focus-visible:bg-app-surface-hover",
                    isSelected && "bg-app-surface-active shadow-[inset_3px_0_0_var(--app-info)]",
                  )}
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <span
                      className={cn(
                        "flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface-muted text-app-muted",
                        isSelected && "border-app-info/40 bg-app-info/10 text-app-info",
                      )}
                    >
                      <Bot className="size-4" />
                    </span>
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <p
                          className={cn(
                            "truncate text-[13px] font-medium text-app-foreground",
                            !agent.isEnabled && "text-app-muted line-through",
                          )}
                        >
                          {agent.name}
                        </p>
                        <span
                          className={cn(
                            "rounded-full border px-2 py-0.5 text-[11px] font-medium",
                            agent.isEnabled
                              ? "border-app-info/30 bg-app-info/10 text-app-info"
                              : "border-app-border bg-app-surface-muted text-app-subtle",
                          )}
                        >
                          {agent.isEnabled ? "Enabled" : "Disabled"}
                        </span>
                      </div>
                      <code className="mt-1 block truncate text-[12px] text-app-subtle">
                        agent_{agent.slug}
                      </code>
                    </div>
                  </div>

                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    onClick={(event) => {
                      event.stopPropagation();
                      handleDelete(agent.id);
                    }}
                    onKeyDown={(event) => event.stopPropagation()}
                    className="shrink-0 text-app-muted opacity-0 hover:bg-app-danger/10 hover:text-app-danger focus-visible:opacity-100 group-hover:opacity-100"
                    title={`Delete ${agent.name}`}
                    aria-label={`Delete ${agent.name}`}
                  >
                    <Trash2 className="size-4" />
                  </Button>
                </div>
              );
            })
          ) : (
            <div className="flex flex-col items-center justify-center px-4 py-10 text-center">
              <span className="flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted text-app-muted">
                <Bot className="size-5" />
              </span>
              <h3 className="mt-3 text-[13px] font-medium text-app-foreground">No custom agents yet</h3>
              <p className="mt-1 max-w-sm text-[12px] leading-5 text-app-muted">
                Create a focused helper with its own instructions and allowed tools.
              </p>
              <Button type="button" variant="outline" size="sm" onClick={handleCreate} className="mt-4">
                <Plus className="size-3.5" />
                Create Agent
              </Button>
            </div>
          )}
        </div>

        {customSubagents.length > 0 ? (
          <>
            <Separator />
            {selectedAgent ? (
              <div className="space-y-5 px-4 py-4">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <h3 className="text-[15px] font-semibold text-app-foreground">{selectedAgent.name}</h3>
                    <p className="mt-1 text-[12px] leading-5 text-app-muted">
                      Configure how this custom agent appears, when it is used, and which tools it can call.
                    </p>
                  </div>
                  {editState ? (
                    <Button type="button" size="sm" onClick={handleSave} disabled={isSaving}>
                      {isSaving ? "Saving..." : "Save Changes"}
                    </Button>
                  ) : null}
                </div>

                <FieldGroup title="Identity" description="Name the agent and define the tool identifier exposed to the main agent.">
                  <div className="grid gap-3 md:grid-cols-2">
                    <label className="space-y-1.5">
                      <span className="text-[12px] font-medium text-app-foreground">Name</span>
                      <Input
                        type="text"
                        value={(currentValue("name") as string) ?? ""}
                        onChange={(event) => updateField("name", event.target.value)}
                      />
                    </label>
                    <label className="space-y-1.5">
                      <span className="text-[12px] font-medium text-app-foreground">Slug</span>
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
                        Tool name: agent_{(currentValue("slug") as string) ?? ""}
                      </span>
                    </label>
                  </div>
                  <div className="flex items-center justify-between gap-3 rounded-xl border border-app-border bg-app-surface-muted px-3 py-2.5">
                    <label
                      htmlFor={`agent-enabled-${selectedAgent.id}`}
                      className="min-w-0 text-[13px] font-medium text-app-foreground"
                    >
                      Enabled
                      <span className="mt-0.5 block text-[12px] font-normal leading-5 text-app-muted">
                        Disabled agents stay configured but are hidden from delegation.
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

                <FieldGroup title="Behavior" description="Describe when to call the agent and how it should behave once delegated.">
                  <label className="block space-y-1.5">
                    <span className="text-[12px] font-medium text-app-foreground">Invocation Description</span>
                    <Textarea
                      value={(currentValue("invocationDescription") as string) ?? ""}
                      onChange={(event) => updateField("invocationDescription", event.target.value)}
                      className="min-h-24"
                    />
                    <span className="block text-[11px] text-app-subtle">
                      This tells the main agent when this specialist should be used.
                    </span>
                  </label>
                  <label className="block space-y-1.5">
                    <span className="text-[12px] font-medium text-app-foreground">System Prompt</span>
                    <Textarea
                      value={(currentValue("systemPrompt") as string) ?? ""}
                      onChange={(event) => updateField("systemPrompt", event.target.value)}
                      className="min-h-44 font-mono"
                    />
                  </label>
                </FieldGroup>

                <Separator />

                <FieldGroup title="Allowed Tools" description="Choose the tools this agent may call during delegated work.">
                  <div className="space-y-4">
                    {TOOL_CATEGORIES.map((category) => (
                      <div key={category.label} className="space-y-2">
                        <div className="flex items-center gap-2 text-[12px] font-medium text-app-muted">
                          <Wrench className="size-3.5" />
                          {category.label}
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
            ) : (
              <div className="flex flex-col items-center justify-center px-4 py-10 text-center">
                <span className="flex size-11 items-center justify-center rounded-2xl border border-app-border bg-app-surface-muted text-app-muted">
                  <Bot className="size-5" />
                </span>
                <h3 className="mt-3 text-[13px] font-medium text-app-foreground">Select an agent to edit</h3>
                <p className="mt-1 max-w-sm text-[12px] leading-5 text-app-muted">
                  Choose a custom agent above, or create a new one to define instructions and tool access.
                </p>
              </div>
            )}
          </>
        ) : null}
      </SettingsPanelSection>
    </div>
  );
}
