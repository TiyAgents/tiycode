import { useCallback, useEffect, useMemo, useState } from "react";
import {
  configListDiagnostics,
  extensionDisable,
  extensionEnable,
  extensionGetDetail,
  extensionUninstall,
  extensionsList,
  extensionsListCommands,
  marketplaceAddSource,
  marketplaceGetRemoveSourcePlan,
  marketplaceInstallItem,
  marketplaceListItems,
  marketplaceListSources,
  marketplaceRefreshSource,
  marketplaceRemoveSource,
  mcpAddServer,
  mcpListServers,
  mcpRemoveServer,
  mcpRestartServer,
  mcpUpdateServer,
  skillDisable,
  skillEnable,
  skillList,
  skillPreview,
  skillRescan,
} from "@/services/bridge";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import type {
  ConfigDiagnostic,
  ExtensionCommand,
  ExtensionDetail,
  ExtensionSummary,
  MarketplaceItem,
  MarketplaceRemoveSourcePlan,
  MarketplaceSource,
  MarketplaceSourceInput,
  McpServerConfigInput,
  McpServerState,
  SkillPreview,
  SkillRecord,
} from "@/shared/types/extensions";

export type ExtensionScope = "global" | "workspace";

type ScopeOptions = {
  scope: ExtensionScope;
  workspacePath?: string | null;
};

export function useExtensionsController(currentWorkspacePath?: string | null) {
  const [extensions, setExtensions] = useState<ExtensionSummary[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerState[]>([]);
  const [skills, setSkills] = useState<SkillRecord[]>([]);
  const [commands, setCommands] = useState<ExtensionCommand[]>([]);
  const [marketplaceSources, setMarketplaceSources] = useState<MarketplaceSource[]>([]);
  const [marketplaceItems, setMarketplaceItems] = useState<MarketplaceItem[]>([]);
  const [configDiagnostics, setConfigDiagnostics] = useState<ConfigDiagnostic[]>([]);
  const [detailByKey, setDetailByKey] = useState<Record<string, ExtensionDetail>>({});
  const [skillPreviewByKey, setSkillPreviewByKey] = useState<Record<string, SkillPreview>>({});
  const [isLoading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const buildScopeOptions = useCallback(
    (scope: ExtensionScope): ScopeOptions => ({
      scope,
      workspacePath: scope === "workspace" ? currentWorkspacePath ?? undefined : undefined,
    }),
    [currentWorkspacePath],
  );

  const cacheKey = useCallback(
    (id: string, scope: ExtensionScope) => `${scope}:${currentWorkspacePath ?? "global"}:${id}`,
    [currentWorkspacePath],
  );

  const refresh = useCallback(
    async (scope: ExtensionScope = "global") => {
      setLoading(true);
      setError(null);
      try {
        const scopeOptions = buildScopeOptions(scope);
        const results = await Promise.allSettled([
          extensionsList(scopeOptions),
          mcpListServers(scopeOptions),
          skillList(scopeOptions),
          extensionsListCommands(),
          marketplaceListSources(),
          marketplaceListItems(),
          configListDiagnostics(),
        ]);

        const [nextExtensions, nextMcpServers, nextSkills, nextCommands, nextSources, nextMarketplaceItems, nextDiagnostics] =
          results;

        if (nextExtensions.status === "fulfilled") {
          setExtensions(nextExtensions.value);
        }
        if (nextMcpServers.status === "fulfilled") {
          setMcpServers(nextMcpServers.value);
        }
        if (nextSkills.status === "fulfilled") {
          setSkills(nextSkills.value);
        }
        if (nextCommands.status === "fulfilled") {
          setCommands(nextCommands.value);
        }
        if (nextSources.status === "fulfilled") {
          setMarketplaceSources(nextSources.value);
        }
        if (nextMarketplaceItems.status === "fulfilled") {
          setMarketplaceItems(nextMarketplaceItems.value);
        }
        if (nextDiagnostics.status === "fulfilled") {
          setConfigDiagnostics(nextDiagnostics.value);
        }

        const rejected = results
          .filter((result): result is PromiseRejectedResult => result.status === "rejected")
          .map((result) => getInvokeErrorMessage(result.reason, "Extensions refresh failed"))
          .filter(Boolean);

        if (rejected.length > 0) {
          setError(rejected.join(" | "));
        }
      } finally {
        setLoading(false);
      }
    },
    [buildScopeOptions],
  );

  useEffect(() => {
    void refresh(currentWorkspacePath ? "workspace" : "global");
  }, [currentWorkspacePath, refresh]);

  const loadDetail = useCallback(
    async (id: string, scope: ExtensionScope) => {
      const key = cacheKey(id, scope);
      const existing = detailByKey[key];
      if (existing) {
        return existing;
      }
      const detail = await extensionGetDetail(id, buildScopeOptions(scope));
      setDetailByKey((current) => ({ ...current, [key]: detail }));
      return detail;
    },
    [buildScopeOptions, cacheKey, detailByKey],
  );

  const loadSkillPreview = useCallback(
    async (id: string, scope: ExtensionScope) => {
      const key = cacheKey(id, scope);
      const existing = skillPreviewByKey[key];
      if (existing) {
        return existing;
      }
      const preview = await skillPreview(id, buildScopeOptions(scope));
      setSkillPreviewByKey((current) => ({ ...current, [key]: preview }));
      return preview;
    },
    [buildScopeOptions, cacheKey, skillPreviewByKey],
  );

  const mutateAndRefresh = useCallback(
    async (scope: ExtensionScope, mutation: () => Promise<unknown>) => {
      setError(null);
      try {
        await mutation();
        setDetailByKey({});
        setSkillPreviewByKey({});
        await refresh(scope);
      } catch (mutationError) {
        const message = getInvokeErrorMessage(mutationError, "Extension update failed");
        setError(message);
      }
    },
    [refresh],
  );

  const pluginCommandEntries = useMemo(
    () =>
      commands.map((command) => ({
        id: `${command.pluginId}:${command.name}`,
        name: command.name,
        path: `/plugin:${command.pluginId}:${command.name}`,
        argumentHint: "",
        description: command.description,
        prompt: command.promptTemplate,
      })),
    [commands],
  );

  const enabledSkills = useMemo(
    () => skills.filter((skill) => skill.enabled),
    [skills],
  );

  const enabledSkillEntries = useMemo(
    () => enabledSkills.map((skill) => ({
      id: skill.id,
      name: skill.name,
      description: skill.description ?? null,
      scope: skill.scope,
      source: skill.source,
      tags: skill.tags,
      triggers: skill.triggers,
      contentPreview: skill.contentPreview,
    })),
    [enabledSkills],
  );

  return {
    extensions,
    mcpServers,
    skills,
    enabledSkills,
    enabledSkillEntries,
    commands,
    marketplaceSources,
    marketplaceItems,
    configDiagnostics,
    pluginCommandEntries,
    detailByKey,
    skillPreviewByKey,
    isLoading,
    error,
    refresh,
    loadDetail,
    loadSkillPreview,
    enableExtension: (id: string) =>
      mutateAndRefresh(currentWorkspacePath ? "workspace" : "global", () =>
        extensionEnable(id, { workspacePath: currentWorkspacePath ?? undefined }),
      ),
    disableExtension: (id: string) =>
      mutateAndRefresh(currentWorkspacePath ? "workspace" : "global", () =>
        extensionDisable(id, { workspacePath: currentWorkspacePath ?? undefined }),
      ),
    uninstallExtension: (id: string, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => extensionUninstall(id, buildScopeOptions(scope))),
    installMarketplaceItem: (id: string) => mutateAndRefresh("global", () => marketplaceInstallItem(id)),
    addMarketplaceSource: (input: MarketplaceSourceInput) =>
      mutateAndRefresh("global", () => marketplaceAddSource(input)),
    getMarketplaceSourceRemovePlan: (id: string): Promise<MarketplaceRemoveSourcePlan> =>
      marketplaceGetRemoveSourcePlan(id),
    removeMarketplaceSource: (id: string) =>
      mutateAndRefresh("global", () => marketplaceRemoveSource(id)),
    refreshMarketplaceSource: (id: string) =>
      mutateAndRefresh("global", () => marketplaceRefreshSource(id)),
    addMcpServer: (input: McpServerConfigInput, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => mcpAddServer(input, buildScopeOptions(scope))),
    updateMcpServer: (id: string, input: McpServerConfigInput, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => mcpUpdateServer(id, input, buildScopeOptions(scope))),
    removeMcpServer: (id: string, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => mcpRemoveServer(id, buildScopeOptions(scope))),
    restartMcpServer: (id: string, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => mcpRestartServer(id, buildScopeOptions(scope))),
    rescanSkills: (scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => skillRescan(buildScopeOptions(scope))),
    enableSkill: (id: string, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => skillEnable(id, buildScopeOptions(scope))),
    disableSkill: (id: string, scope: ExtensionScope) =>
      mutateAndRefresh(scope, () => skillDisable(id, buildScopeOptions(scope))),
  };
}
