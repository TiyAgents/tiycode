import type { AgentProfile } from "@/modules/settings-center/model/types";

export function compareAgentProfilesByName(left: AgentProfile, right: AgentProfile) {
  const nameCompare = left.name.localeCompare(right.name, undefined, { sensitivity: "base" });
  if (nameCompare !== 0) return nameCompare;
  return left.id.localeCompare(right.id, undefined, { sensitivity: "base" });
}

export function sortAgentProfilesByName(profiles: ReadonlyArray<AgentProfile>) {
  return [...profiles].sort(compareAgentProfilesByName);
}
