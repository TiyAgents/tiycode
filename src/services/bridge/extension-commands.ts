import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  ExtensionActivityEvent,
  ExtensionCommand,
  ExtensionDetail,
  ExtensionSummary,
  MarketplaceItem,
  MarketplaceSource,
  MarketplaceSourceInput,
  McpServerConfigInput,
  McpServerState,
  PluginDetail,
  SkillPreview,
  SkillRecord,
} from "@/shared/types/extensions";

const requireTauri = (cmd: string) => {
  if (!isTauri()) throw new Error(`${cmd} requires Tauri runtime`);
};

export async function extensionsList(options?: { scope?: string; workspacePath?: string | null }): Promise<ExtensionSummary[]> {
  if (!isTauri()) return [];
  return invoke<ExtensionSummary[]>("extensions_list", {
    scope: options?.scope,
    workspacePath: options?.workspacePath ?? undefined,
  });
}

export async function extensionGetDetail(
  id: string,
  options?: { scope?: string; workspacePath?: string | null },
): Promise<ExtensionDetail> {
  requireTauri("extension_get_detail");
  return invoke<ExtensionDetail>("extension_get_detail", {
    id,
    scope: options?.scope,
    workspacePath: options?.workspacePath ?? undefined,
  });
}

export async function extensionEnable(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("extension_enable");
  return invoke("extension_enable", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function extensionDisable(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("extension_disable");
  return invoke("extension_disable", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function extensionUninstall(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("extension_uninstall");
  return invoke("extension_uninstall", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function extensionsListCommands(): Promise<ExtensionCommand[]> {
  if (!isTauri()) return [];
  return invoke<ExtensionCommand[]>("extensions_list_commands");
}

export async function extensionsListActivity(limit = 50): Promise<ExtensionActivityEvent[]> {
  if (!isTauri()) return [];
  return invoke<ExtensionActivityEvent[]>("extensions_list_activity", { limit });
}

export async function pluginValidateDir(path: string): Promise<PluginDetail> {
  requireTauri("plugin_validate_dir");
  return invoke<PluginDetail>("plugin_validate_dir", { path });
}

export async function pluginInstallFromDir(path: string): Promise<PluginDetail> {
  requireTauri("plugin_install_from_dir");
  return invoke<PluginDetail>("plugin_install_from_dir", { path });
}

export async function pluginUpdateConfig(id: string, config: unknown): Promise<void> {
  requireTauri("plugin_update_config");
  return invoke("plugin_update_config", { id, config });
}

export async function mcpListServers(options?: { scope?: string; workspacePath?: string | null }): Promise<McpServerState[]> {
  if (!isTauri()) return [];
  return invoke<McpServerState[]>("mcp_list_servers", {
    scope: options?.scope,
    workspacePath: options?.workspacePath ?? undefined,
  });
}

export async function mcpAddServer(
  input: McpServerConfigInput,
  options?: { scope?: string; workspacePath?: string | null },
): Promise<McpServerState> {
  requireTauri("mcp_add_server");
  return invoke<McpServerState>("mcp_add_server", { input, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function mcpUpdateServer(
  id: string,
  input: McpServerConfigInput,
  options?: { scope?: string; workspacePath?: string | null },
): Promise<McpServerState> {
  requireTauri("mcp_update_server");
  return invoke<McpServerState>("mcp_update_server", { id, input, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function mcpRemoveServer(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("mcp_remove_server");
  return invoke("mcp_remove_server", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function mcpRestartServer(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<McpServerState> {
  requireTauri("mcp_restart_server");
  return invoke<McpServerState>("mcp_restart_server", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function mcpGetServerState(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<McpServerState> {
  requireTauri("mcp_get_server_state");
  return invoke<McpServerState>("mcp_get_server_state", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillList(options?: { scope?: string; workspacePath?: string | null }): Promise<SkillRecord[]> {
  if (!isTauri()) return [];
  return invoke<SkillRecord[]>("skill_list", { scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillRescan(options?: { scope?: string; workspacePath?: string | null }): Promise<SkillRecord[]> {
  requireTauri("skill_rescan");
  return invoke<SkillRecord[]>("skill_rescan", { scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillEnable(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("skill_enable");
  return invoke("skill_enable", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillDisable(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<void> {
  requireTauri("skill_disable");
  return invoke("skill_disable", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillPin(
  id: string,
  pinned: boolean,
  options?: { scope?: string; workspacePath?: string | null },
): Promise<void> {
  requireTauri("skill_pin");
  return invoke("skill_pin", { id, pinned, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function skillPreview(id: string, options?: { scope?: string; workspacePath?: string | null }): Promise<SkillPreview> {
  requireTauri("skill_preview");
  return invoke<SkillPreview>("skill_preview", { id, scope: options?.scope, workspacePath: options?.workspacePath ?? undefined });
}

export async function marketplaceListSources(): Promise<MarketplaceSource[]> {
  if (!isTauri()) return [];
  return invoke<MarketplaceSource[]>("marketplace_list_sources");
}

export async function marketplaceAddSource(input: MarketplaceSourceInput): Promise<MarketplaceSource> {
  requireTauri("marketplace_add_source");
  return invoke<MarketplaceSource>("marketplace_add_source", { input });
}

export async function marketplaceRemoveSource(id: string): Promise<void> {
  requireTauri("marketplace_remove_source");
  return invoke("marketplace_remove_source", { id });
}

export async function marketplaceRefreshSource(id: string): Promise<MarketplaceSource> {
  requireTauri("marketplace_refresh_source");
  return invoke<MarketplaceSource>("marketplace_refresh_source", { id });
}

export async function marketplaceListItems(): Promise<MarketplaceItem[]> {
  if (!isTauri()) return [];
  return invoke<MarketplaceItem[]>("marketplace_list_items");
}

export async function marketplaceInstallItem(id: string): Promise<PluginDetail> {
  requireTauri("marketplace_install_item");
  return invoke<PluginDetail>("marketplace_install_item", { id });
}
