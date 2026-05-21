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
  }, [profileId]);

  const toggleAccess = async (subagentId: string) => {
    const next = accessIds.includes(subagentId)
      ? accessIds.filter((id) => id !== subagentId)
      : [...accessIds, subagentId];
    setAccessIds(next);
    try {
      await profileSubagentAccessSet(profileId, next);
    } catch (error) {
      console.error("Failed to update profile subagent access", error);
      // Revert optimistic update
      setAccessIds(accessIds);
    }
  };

  if (customSubagents.length === 0) {
    return null;
  }

  return (
    <div className="mt-6 border-t border-app-border pt-4">
      <h4 className="mb-2 flex items-center gap-1.5 text-sm font-medium">
        <Bot className="size-4" />
        Available Agents
      </h4>
      <p className="mb-3 text-xs text-app-foreground-muted">
        Select which custom agents this profile can use. Built-in agents (Explore, Review) are always available.
      </p>

      {isLoading ? (
        <p className="text-xs text-app-foreground-muted">Loading...</p>
      ) : (
        <div className="space-y-1.5">
          {/* Built-in agents (always on) */}
          <label className="flex items-center gap-2 text-sm opacity-60">
            <input type="checkbox" checked disabled className="size-3.5 rounded" />
            <span className="font-medium">Explore</span>
            <span className="text-xs text-app-foreground-muted">(built-in, always available)</span>
          </label>
          <label className="flex items-center gap-2 text-sm opacity-60">
            <input type="checkbox" checked disabled className="size-3.5 rounded" />
            <span className="font-medium">Review</span>
            <span className="text-xs text-app-foreground-muted">(built-in, always available)</span>
          </label>

          {/* Custom agents */}
          {customSubagents.map((agent) => (
            <label key={agent.id} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={accessIds.includes(agent.id)}
                onChange={() => toggleAccess(agent.id)}
                className="size-3.5 rounded border-app-border"
              />
              <span className="font-medium">{agent.name}</span>
              <code className="text-xs text-app-foreground-muted">agent_{agent.slug}</code>
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
