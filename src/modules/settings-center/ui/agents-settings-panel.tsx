import { useState } from "react";
import { Bot, Plus, Trash2 } from "lucide-react";
import type { CustomSubagent } from "@/modules/settings-center/model/types";
import {
  customSubagentCreate,
  customSubagentDelete,
  customSubagentUpdate,
  type CustomSubagentInput,
} from "@/services/bridge/subagent-commands";
import { settingsStore } from "@/modules/settings-center/model/settings-store";

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
    <div className="flex h-full min-h-0 flex-col">
      {description && (
        <p className="mb-4 text-sm text-app-foreground-muted">{description}</p>
      )}

      {/* Built-in agents info */}
      <div className="mb-4 rounded-md border border-app-border bg-app-sidebar/50 p-3">
        <h4 className="mb-1 text-xs font-medium uppercase tracking-wide text-app-foreground-muted">
          Built-in Agents (always available)
        </h4>
        <div className="flex gap-4 text-sm">
          <span className="flex items-center gap-1.5">
            <Bot className="size-3.5 text-app-accent" />
            <span className="font-medium">Explore</span>
            <span className="text-app-foreground-muted">— read-only code investigation</span>
          </span>
          <span className="flex items-center gap-1.5">
            <Bot className="size-3.5 text-app-accent" />
            <span className="font-medium">Review</span>
            <span className="text-app-foreground-muted">— code review + verification</span>
          </span>
        </div>
      </div>

      <div className="flex flex-1 gap-4 overflow-hidden">
        {/* Left: agent list */}
        <div className="flex w-56 shrink-0 flex-col rounded-md border border-app-border">
          <div className="flex items-center justify-between border-b border-app-border px-3 py-2">
            <span className="text-xs font-medium uppercase tracking-wide text-app-foreground-muted">
              Custom Agents
            </span>
            <button
              type="button"
              onClick={handleCreate}
              className="rounded p-1 hover:bg-app-surface-hover"
              title="Add agent"
            >
              <Plus className="size-3.5" />
            </button>
          </div>
          <div className="flex-1 overflow-y-auto">
            {customSubagents.map((agent) => (
              <button
                key={agent.id}
                type="button"
                onClick={() => {
                  setSelectedId(agent.id);
                  setEditState(null);
                }}
                className={`flex w-full items-center justify-between px-3 py-2 text-left text-sm hover:bg-app-surface-hover ${
                  agent.id === selectedId ? "bg-app-surface-active" : ""
                }`}
              >
                <span className={agent.isEnabled ? "" : "text-app-foreground-muted line-through"}>
                  {agent.name}
                </span>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDelete(agent.id);
                  }}
                  className="rounded p-0.5 opacity-0 hover:bg-app-danger/10 group-hover:opacity-100 [button:hover>&]:opacity-100"
                  title="Delete"
                >
                  <Trash2 className="size-3 text-app-danger" />
                </button>
              </button>
            ))}
            {customSubagents.length === 0 && (
              <p className="px-3 py-4 text-center text-xs text-app-foreground-muted">
                No custom agents yet
              </p>
            )}
          </div>
        </div>

        {/* Right: detail editor */}
        <div className="flex-1 overflow-y-auto">
          {selectedAgent ? (
            <div className="space-y-4">
              {/* Name */}
              <div>
                <label className="mb-1 block text-xs font-medium">Name</label>
                <input
                  type="text"
                  value={(currentValue("name") as string) ?? ""}
                  onChange={(e) => updateField("name", e.target.value)}
                  className="w-full rounded-md border border-app-border bg-app-surface px-3 py-1.5 text-sm"
                />
              </div>

              {/* Slug */}
              <div>
                <label className="mb-1 block text-xs font-medium">
                  Slug{" "}
                  <span className="font-normal text-app-foreground-muted">
                    (tool name: agent_{(currentValue("slug") as string) ?? ""})
                  </span>
                </label>
                <input
                  type="text"
                  value={(currentValue("slug") as string) ?? ""}
                  onChange={(e) => updateField("slug", e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""))}
                  className="w-full rounded-md border border-app-border bg-app-surface px-3 py-1.5 text-sm font-mono"
                />
              </div>

              {/* Enabled toggle */}
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={(currentValue("isEnabled") as boolean) ?? true}
                  onChange={(e) => updateField("isEnabled", e.target.checked)}
                  className="size-4 rounded border-app-border"
                  id="agent-enabled"
                />
                <label htmlFor="agent-enabled" className="text-sm">Enabled</label>
              </div>

              {/* Invocation Description */}
              <div>
                <label className="mb-1 block text-xs font-medium">
                  Invocation Description
                  <span className="ml-1 font-normal text-app-foreground-muted">
                    (tells the main agent when to use this)
                  </span>
                </label>
                <textarea
                  value={(currentValue("invocationDescription") as string) ?? ""}
                  onChange={(e) => updateField("invocationDescription", e.target.value)}
                  rows={3}
                  className="w-full rounded-md border border-app-border bg-app-surface px-3 py-1.5 text-sm"
                />
              </div>

              {/* System Prompt */}
              <div>
                <label className="mb-1 block text-xs font-medium">System Prompt</label>
                <textarea
                  value={(currentValue("systemPrompt") as string) ?? ""}
                  onChange={(e) => updateField("systemPrompt", e.target.value)}
                  rows={8}
                  className="w-full rounded-md border border-app-border bg-app-surface px-3 py-1.5 text-sm font-mono"
                />
              </div>

              {/* Allowed Tools */}
              <div>
                <label className="mb-2 block text-xs font-medium">Allowed Tools</label>
                <div className="space-y-3">
                  {TOOL_CATEGORIES.map((category) => (
                    <div key={category.label}>
                      <span className="text-xs font-medium text-app-foreground-muted">
                        {category.label}
                      </span>
                      <div className="mt-1 flex flex-wrap gap-2">
                        {category.tools.map((tool) => {
                          const checked = ((currentValue("allowedTools") as string[]) ?? []).includes(tool);
                          return (
                            <label
                              key={tool}
                              className="flex items-center gap-1.5 text-xs"
                            >
                              <input
                                type="checkbox"
                                checked={checked}
                                onChange={() => toggleTool(tool)}
                                className="size-3.5 rounded border-app-border"
                              />
                              <code className="rounded bg-app-surface px-1 py-0.5">{tool}</code>
                            </label>
                          );
                        })}
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              {/* Save button */}
              {editState && (
                <button
                  type="button"
                  onClick={handleSave}
                  disabled={isSaving}
                  className="rounded-md bg-app-accent px-4 py-1.5 text-sm font-medium text-white hover:bg-app-accent/90 disabled:opacity-50"
                >
                  {isSaving ? "Saving..." : "Save Changes"}
                </button>
              )}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-app-foreground-muted">
              Select an agent or create a new one
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
