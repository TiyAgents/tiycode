import { useEffect, useState } from "react";
import { Bot } from "lucide-react";
import type { CustomSubagent } from "@/modules/settings-center/model/types";
import {
  profileSubagentAccessGet,
  profileSubagentAccessSet,
} from "@/services/bridge/subagent-commands";

type ProfileAgentAccessProps = {
  profileId: string;
  customSubagents: CustomSubagent[];
};

export function ProfileAgentAccess({
  profileId,
  customSubagents,
}: ProfileAgentAccessProps) {
  const [accessIds, setAccessIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (customSubagents.length === 0) {
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
    return () => { cancelled = true; };
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
    <div>
      <h4 className="mb-2 flex items-center gap-1.5 text-[13px] font-medium leading-5 text-app-foreground">
        <Bot className="size-4" />
        Available Agents
      </h4>
      <p className="mb-3 text-[12px] leading-5 text-app-muted">
        Configure which agents this profile can delegate tasks to. Built-in agents are always available.
      </p>

      <div className="space-y-1.5">
        {/* Built-in agents (always on) */}
        <label className="flex items-center gap-2 text-sm opacity-60">
          <input type="checkbox" checked disabled className="size-3.5 rounded" />
          <span className="font-medium">Explore</span>
          <span className="text-xs text-app-muted">(built-in)</span>
        </label>
        <label className="flex items-center gap-2 text-sm opacity-60">
          <input type="checkbox" checked disabled className="size-3.5 rounded" />
          <span className="font-medium">Review</span>
          <span className="text-xs text-app-muted">(built-in)</span>
        </label>

        {/* Custom agents */}
        {isLoading ? (
          <p className="py-1 text-xs text-app-muted">Loading...</p>
        ) : customSubagents.length > 0 ? (
          customSubagents.map((agent) => (
            <label key={agent.id} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={accessIds.includes(agent.id)}
                onChange={() => toggleAccess(agent.id)}
                className="size-3.5 rounded border-app-border"
              />
              <span className="font-medium">{agent.name}</span>
              <code className="text-xs text-app-subtle">agent_{agent.slug}</code>
            </label>
          ))
        ) : (
          <p className="py-1 text-xs text-app-muted">
            No custom agents configured. Go to Settings → Agents to create one.
          </p>
        )}
      </div>
    </div>
  );
}

