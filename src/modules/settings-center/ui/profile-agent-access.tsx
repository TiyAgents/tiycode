import { useEffect, useState } from "react";
import {
  CheckCircle2,
  FileSearch,
  LockKeyhole,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { CustomSubagent } from "@/modules/settings-center/model/types";
import {
  profileSubagentAccessGet,
  profileSubagentAccessSet,
} from "@/services/bridge/subagent-commands";
import { cn } from "@/shared/lib/utils";

type ProfileAgentAccessProps = {
  profileId: string;
  customSubagents: CustomSubagent[];
};

type BuiltInAgent = {
  name: string;
  description: string;
  icon: LucideIcon;
  scope: string;
  capabilities: string[];
};

const BUILT_IN_AGENTS: BuiltInAgent[] = [
  {
    name: "Explore",
    description: "Maps files, dependencies, and current behavior before implementation decisions.",
    icon: FileSearch,
    scope: "Read-only research",
    capabilities: ["File discovery", "Architecture notes", "Current-state summary"],
  },
  {
    name: "Review",
    description: "Reviews implemented changes, probes regressions, and reports verification status.",
    icon: CheckCircle2,
    scope: "Post-change review",
    capabilities: ["Diff review", "Risk scan", "Verification report"],
  },
];

export function ProfileAgentAccess({
  profileId,
  customSubagents,
}: ProfileAgentAccessProps) {
  const [accessIds, setAccessIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (customSubagents.length === 0) {
      setAccessIds([]);
      setIsLoading(false);
      return;
    }
    let cancelled = false;
    setIsLoading(true);
    profileSubagentAccessGet(profileId)
      .then((ids) => {
        if (!cancelled) {
          setAccessIds(ids);
          setIsLoading(false);
        }
      })
      .catch(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [profileId, customSubagents.length]);

  const toggleAccess = async (subagentId: string) => {
    const next = accessIds.includes(subagentId)
      ? accessIds.filter((id) => id !== subagentId)
      : [...accessIds, subagentId];
    setAccessIds(next);
    try {
      await profileSubagentAccessSet(profileId, next);
    } catch (error) {
      console.error("Failed to update profile subagent access", error);
      setAccessIds(accessIds);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex min-w-0 items-start">
        <div className="min-w-0">
          <h4 className="text-[13px] font-medium leading-5 text-app-foreground">Available Agents</h4>
          <p className="mt-1 max-w-2xl text-[12px] leading-5 text-app-muted">
            Choose which custom agents this profile can delegate to. Built-in agents stay enabled for every profile.
          </p>
        </div>
      </div>

      <div className="grid gap-2 md:grid-cols-2">
        {BUILT_IN_AGENTS.map((agent) => {
          const Icon = agent.icon;
          return (
            <div
              key={agent.name}
              className="rounded-xl border border-app-border bg-app-surface-muted p-3 shadow-sm"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2.5">
                  <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-app-border bg-app-surface text-app-info">
                    <Icon className="size-4" />
                  </span>
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <p className="text-[13px] font-medium text-app-foreground">{agent.name}</p>
                      <span className="rounded-full border border-app-info/30 bg-app-info/10 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em] text-app-info">
                        Always on
                      </span>
                    </div>
                    <p className="mt-0.5 text-[11px] text-app-subtle">{agent.scope}</p>
                  </div>
                </div>
                <LockKeyhole className="mt-1 size-3.5 shrink-0 text-app-subtle" />
              </div>
              <p className="mt-3 text-[12px] leading-5 text-app-muted">{agent.description}</p>
              <div className="mt-3 flex flex-wrap gap-1.5">
                {agent.capabilities.map((capability) => (
                  <span
                    key={capability}
                    className="rounded-md border border-app-border bg-app-surface px-1.5 py-0.5 text-[11px] text-app-muted"
                  >
                    {capability}
                  </span>
                ))}
              </div>
            </div>
          );
        })}
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between px-0.5">
          <p className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Custom Agents</p>
          <p className="text-[11px] text-app-subtle">Profile-level access</p>
        </div>

        {isLoading ? (
          <div className="grid gap-2 md:grid-cols-2">
            {[0, 1].map((item) => (
              <div
                key={item}
                className="h-28 animate-pulse rounded-xl border border-app-border bg-app-surface-muted"
              />
            ))}
          </div>
        ) : customSubagents.length > 0 ? (
          <div className="grid gap-2 md:grid-cols-2">
            {customSubagents.map((agent) => {
              const isSelected = accessIds.includes(agent.id);
              const visibleTools = agent.allowedTools.slice(0, 4);
              const extraToolCount = agent.allowedTools.length - visibleTools.length;

              return (
                <label
                  key={agent.id}
                  className={cn(
                    "group relative flex cursor-pointer rounded-xl border p-3 pr-12 transition-colors",
                    isSelected
                      ? "border-app-info/40 bg-app-info/10"
                      : "border-app-border bg-app-surface-muted hover:bg-app-surface-hover",
                    !agent.isEnabled && "opacity-70",
                  )}
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => toggleAccess(agent.id)}
                    aria-label={`Toggle access for ${agent.name}`}
                    className="absolute right-3 top-3 size-4 rounded border-app-border accent-app-info"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="flex flex-wrap items-center gap-1.5">
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
                          "rounded-full border px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em]",
                          agent.isEnabled
                            ? "border-app-info/30 bg-app-info/10 text-app-info"
                            : "border-app-border bg-app-surface text-app-subtle",
                        )}
                      >
                        {agent.isEnabled ? "Enabled" : "Disabled"}
                      </span>
                      <span
                        className={cn(
                          "rounded-full border px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-[0.08em]",
                          isSelected
                            ? "border-app-success/30 bg-app-success/10 text-app-success"
                            : "border-app-border bg-app-surface text-app-subtle",
                        )}
                      >
                        {isSelected ? "Allowed" : "Off"}
                      </span>
                    </span>

                    <code className="mt-1 block truncate text-[11px] text-app-subtle">agent_{agent.slug}</code>
                    <span className="mt-2 block text-[12px] leading-5 text-app-muted">
                      {agent.invocationDescription.trim() || "No delegation description provided yet."}
                    </span>

                    <span className="mt-3 flex flex-wrap items-center gap-1.5">
                      <span className="inline-flex items-center gap-1 rounded-md border border-app-border bg-app-surface px-1.5 py-0.5 text-[11px] text-app-muted">
                        <Wrench className="size-3" />
                        {agent.allowedTools.length} tools
                      </span>
                      {visibleTools.map((tool) => (
                        <code
                          key={tool}
                          className="rounded-md border border-app-border bg-app-canvas px-1.5 py-0.5 text-[11px] text-app-subtle"
                        >
                          {tool}
                        </code>
                      ))}
                      {extraToolCount > 0 ? (
                        <span className="rounded-md border border-app-border bg-app-surface px-1.5 py-0.5 text-[11px] text-app-subtle">
                          +{extraToolCount}
                        </span>
                      ) : null}
                    </span>
                  </span>
                </label>
              );
            })}
          </div>
        ) : (
          <div className="rounded-xl border border-dashed border-app-border bg-app-surface-muted px-4 py-5 text-center">
            <p className="text-[13px] font-medium text-app-foreground">No custom agents configured</p>
            <p className="mt-1 text-[12px] leading-5 text-app-muted">
              Go to Settings → Agents to create focused helpers with their own instructions and tools.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
