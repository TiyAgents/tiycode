import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

import * as extensionCommands from "./extension-commands";

describe("extension-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // extensionsList
  // ---------------------------------------------------------------------------
  describe("extensionsList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.extensionsList();
      expect(result).toEqual([]);
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls extensions_list with options when in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const exts = [{ id: "e1", name: "Test" }] as any;
      vi.mocked(invoke).mockResolvedValue(exts);

      const result = await extensionCommands.extensionsList({ scope: "user", workspacePath: "/p" });
      expect(result).toEqual(exts);
      expect(invoke).toHaveBeenCalledWith("extensions_list", {
        scope: "user",
        workspacePath: "/p",
      });
    });

    it("calls with no options", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue([]);

      const result = await extensionCommands.extensionsList();
      expect(invoke).toHaveBeenCalledWith("extensions_list", {});
    });
  });

  // ---------------------------------------------------------------------------
  // configListDiagnostics
  // ---------------------------------------------------------------------------
  describe("configListDiagnostics", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.configListDiagnostics();
      expect(result).toEqual([]);
    });

    it("calls config_list_diagnostics", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const diags = [{ level: "error", message: "bad" }] as any;
      vi.mocked(invoke).mockResolvedValue(diags);

      const result = await extensionCommands.configListDiagnostics();
      expect(result).toEqual(diags);
      expect(invoke).toHaveBeenCalledWith("config_list_diagnostics");
    });
  });

  // ---------------------------------------------------------------------------
  // extensionGetDetail
  // ---------------------------------------------------------------------------
  describe("extensionGetDetail", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.extensionGetDetail("e1")).rejects.toThrow(
        "extension_get_detail requires Tauri runtime",
      );
    });

    it("calls extension_get_detail with id and options", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const detail = { id: "e1", name: "Ext" } as any;
      vi.mocked(invoke).mockResolvedValue(detail);

      const result = await extensionCommands.extensionGetDetail("e1", { scope: "workspace", workspacePath: "/p" });
      expect(result).toEqual(detail);
      expect(invoke).toHaveBeenCalledWith("extension_get_detail", {
        id: "e1",
        scope: "workspace",
        workspacePath: "/p",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // extensionEnable
  // ---------------------------------------------------------------------------
  describe("extensionEnable", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.extensionEnable("e1")).rejects.toThrow(
        "extension_enable requires Tauri runtime",
      );
    });

    it("calls extension_enable", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await extensionCommands.extensionEnable("e1");
      expect(invoke).toHaveBeenCalledWith("extension_enable", {
        id: "e1",
        scope: undefined,
        workspacePath: undefined,
      });
    });
  });

  // ---------------------------------------------------------------------------
  // extensionDisable
  // ---------------------------------------------------------------------------
  describe("extensionDisable", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.extensionDisable("e1")).rejects.toThrow(
        "extension_disable requires Tauri runtime",
      );
    });

    it("calls extension_disable", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await extensionCommands.extensionDisable("e1");
      expect(invoke).toHaveBeenCalledWith("extension_disable", { id: "e1" });
    });
  });

  // ---------------------------------------------------------------------------
  // extensionUninstall
  // ---------------------------------------------------------------------------
  describe("extensionUninstall", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.extensionUninstall("e1")).rejects.toThrow(
        "extension_uninstall requires Tauri runtime",
      );
    });

    it("calls extension_uninstall", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await extensionCommands.extensionUninstall("e1");
      expect(invoke).toHaveBeenCalledWith("extension_uninstall", { id: "e1" });
    });
  });

  // ---------------------------------------------------------------------------
  // extensionsListCommands
  // ---------------------------------------------------------------------------
  describe("extensionsListCommands", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.extensionsListCommands();
      expect(result).toEqual([]);
    });

    it("calls extensions_list_commands", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const cmds = [{ name: "test-cmd" }] as any;
      vi.mocked(invoke).mockResolvedValue(cmds);

      const result = await extensionCommands.extensionsListCommands();
      expect(result).toEqual(cmds);
      expect(invoke).toHaveBeenCalledWith("extensions_list_commands");
    });
  });

  // ---------------------------------------------------------------------------
  // extensionsListActivity
  // ---------------------------------------------------------------------------
  describe("extensionsListActivity", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.extensionsListActivity();
      expect(result).toEqual([]);
    });

    it("calls extensions_list_activity with default limit 50", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const activity = [{ id: "a1" }] as any;
      vi.mocked(invoke).mockResolvedValue(activity);

      const result = await extensionCommands.extensionsListActivity();
      expect(result).toEqual(activity);
      expect(invoke).toHaveBeenCalledWith("extensions_list_activity", { limit: 50 });
    });

    it("passes custom limit", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue([] as any);

      await extensionCommands.extensionsListActivity(10);
      expect(invoke).toHaveBeenCalledWith("extensions_list_activity", { limit: 10 });
    });
  });

  // ---------------------------------------------------------------------------
  // Plugin management
  // ---------------------------------------------------------------------------
  describe("pluginValidateDir", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.pluginValidateDir("/path")).rejects.toThrow(
        "plugin_validate_dir requires Tauri runtime",
      );
    });

    it("calls plugin_validate_dir", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const plugin = { name: "MyPlugin" } as any;
      vi.mocked(invoke).mockResolvedValue(plugin);

      const result = await extensionCommands.pluginValidateDir("/ext/plugin");
      expect(result).toEqual(plugin);
      expect(invoke).toHaveBeenCalledWith("plugin_validate_dir", { path: "/ext/plugin" });
    });
  });

  describe("pluginInstallFromDir", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.pluginInstallFromDir("/path")).rejects.toThrow(
        "plugin_install_from_dir requires Tauri runtime",
      );
    });

    it("calls plugin_install_from_dir", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const plugin = { name: "InstalledPlugin" } as any;
      vi.mocked(invoke).mockResolvedValue(plugin);

      const result = await extensionCommands.pluginInstallFromDir("/ext/new-plugin");
      expect(result).toEqual(plugin);
      expect(invoke).toHaveBeenCalledWith("plugin_install_from_dir", { path: "/ext/new-plugin" });
    });
  });

  describe("pluginUpdateConfig", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.pluginUpdateConfig("p1", {})).rejects.toThrow(
        "plugin_update_config requires Tauri runtime",
      );
    });

    it("calls plugin_update_config with id and config", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      const config = { enabled: true };
      await extensionCommands.pluginUpdateConfig("p1", config);
      expect(invoke).toHaveBeenCalledWith("plugin_update_config", { id: "p1", config });
    });
  });

  // ---------------------------------------------------------------------------
  // MCP Server management
  // ---------------------------------------------------------------------------
  describe("mcpListServers", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.mcpListServers();
      expect(result).toEqual([]);
    });

    it("calls mcp_list_servers", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const servers = [{ id: "m1", name: "MCP Server" }] as any;
      vi.mocked(invoke).mockResolvedValue(servers);

      const result = await extensionCommands.mcpListServers({ scope: "user" });
      expect(result).toEqual(servers);
      expect(invoke).toHaveBeenCalledWith("mcp_list_servers", { scope: "user" });
    });
  });

  describe("mcpAddServer", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.mcpAddServer({} as any)).rejects.toThrow(
        "mcp_add_server requires Tauri runtime",
      );
    });

    it("calls mcp_add_server with input and options", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const server = { id: "m2" } as any;
      const input = { name: "New MCP" } as any;
      vi.mocked(invoke).mockResolvedValue(server);

      const result = await extensionCommands.mcpAddServer(input, { scope: "workspace" });
      expect(result).toEqual(server);
      expect(invoke).toHaveBeenCalledWith("mcp_add_server", {
        input,
        scope: "workspace",
      });
    });
  });

  describe("mcpUpdateServer", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.mcpUpdateServer("m1", {} as any)).rejects.toThrow(
        "mcp_update_server requires Tauri runtime",
      );
    });

    it("calls mcp_update_server", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const server = { id: "m1" } as any;
      const input = { name: "Updated" } as any;
      vi.mocked(invoke).mockResolvedValue(server);

      const result = await extensionCommands.mcpUpdateServer("m1", input);
      expect(result).toEqual(server);
      expect(invoke).toHaveBeenCalledWith("mcp_update_server", { id: "m1", input });
    });
  });

  describe("mcpRemoveServer", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.mcpRemoveServer("m1")).rejects.toThrow(
        "mcp_remove_server requires Tauri runtime",
      );
    });

    it("calls mcp_remove_server", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await extensionCommands.mcpRemoveServer("m1");
      expect(invoke).toHaveBeenCalledWith("mcp_remove_server", { id: "m1" });
    });
  });

  describe("mcpRestartServer", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.mcpRestartServer("m1")).rejects.toThrow(
        "mcp_restart_server requires Tauri runtime",
      );
    });

    it("calls mcp_restart_server", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const server = { id: "m1", status: "running" } as any;
      vi.mocked(invoke).mockResolvedValue(server);

      const result = await extensionCommands.mcpRestartServer("m1");
      expect(result).toEqual(server);
      expect(invoke).toHaveBeenCalledWith("mcp_restart_server", { id: "m1" });
    });
  });

  describe("mcpGetServerState", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.mcpGetServerState("m1")).rejects.toThrow(
        "mcp_get_server_state requires Tauri runtime",
      );
    });

    it("calls mcp_get_server_state", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const state = { id: "m1", status: "connected" } as any;
      vi.mocked(invoke).mockResolvedValue(state);

      const result = await extensionCommands.mcpGetServerState("m1");
      expect(result).toEqual(state);
      expect(invoke).toHaveBeenCalledWith("mcp_get_server_state", { id: "m1" });
    });
  });

  // ---------------------------------------------------------------------------
  // Skill management
  // ---------------------------------------------------------------------------
  describe("skillList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.skillList();
      expect(result).toEqual([]);
    });

    it("calls skill_list with options", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const skills = [{ id: "s1", name: "Test Skill" }] as any;
      vi.mocked(invoke).mockResolvedValue(skills);

      const result = await extensionCommands.skillList({ scope: "user" });
      expect(result).toEqual(skills);
      expect(invoke).toHaveBeenCalledWith("skill_list", { scope: "user" });
    });
  });

  describe("skillRescan", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.skillRescan()).rejects.toThrow(
        "skill_rescan requires Tauri runtime",
      );
    });

    it("calls skill_rescan", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const skills = [{ id: "s1" }] as any;
      vi.mocked(invoke).mockResolvedValue(skills);

      const result = await extensionCommands.skillRescan();
      expect(result).toEqual(skills);
      expect(invoke).toHaveBeenCalledWith("skill_rescan", expect.objectContaining({}));
    });
  });

  describe("skillEnable / skillDisable / skillPreview", () => {
    it("skillEnable throws when not in Tauri and calls correctly otherwise", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.skillEnable("s1")).rejects.toThrow(
        "skill_enable requires Tauri runtime",
      );

      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);
      await extensionCommands.skillEnable("s1");
      expect(invoke).toHaveBeenCalledWith("skill_enable", { id: "s1" });
    });

    it("skillDisable throws when not in Tauri and calls correctly otherwise", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.skillDisable("s1")).rejects.toThrow(
        "skill_disable requires Tauri runtime",
      );

      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);
      await extensionCommands.skillDisable("s1");
      expect(invoke).toHaveBeenCalledWith("skill_disable", { id: "s1" });
    });

    it("skillPreview throws when not in Tauri and calls correctly otherwise", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.skillPreview("s1")).rejects.toThrow(
        "skill_preview requires Tauri runtime",
      );

      vi.mocked(isTauri).mockReturnValue(true);
      const preview = { id: "s1", content: "preview" } as any;
      vi.mocked(invoke).mockResolvedValue(preview);

      const result = await extensionCommands.skillPreview("s1");
      expect(result).toEqual(preview);
      expect(invoke).toHaveBeenCalledWith("skill_preview", { id: "s1" });
    });
  });

  // ---------------------------------------------------------------------------
  // Marketplace
  // ---------------------------------------------------------------------------
  describe("marketplaceListSources", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.marketplaceListSources();
      expect(result).toEqual([]);
    });

    it("calls marketplace_list_sources", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const sources = [{ id: "src1", url: "https://example.com" }] as any;
      vi.mocked(invoke).mockResolvedValue(sources);

      const result = await extensionCommands.marketplaceListSources();
      expect(result).toEqual(sources);
      expect(invoke).toHaveBeenCalledWith("marketplace_list_sources");
    });
  });

  describe("marketplaceAddSource", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        extensionCommands.marketplaceAddSource({} as any),
      ).rejects.toThrow("marketplace_add_source requires Tauri runtime");
    });

    it("calls marketplace_add_source", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const source = { id: "src2" } as any;
      const input = { url: "https://registry.example.com" } as any;
      vi.mocked(invoke).mockResolvedValue(source);

      const result = await extensionCommands.marketplaceAddSource(input);
      expect(result).toEqual(source);
      expect(invoke).toHaveBeenCalledWith("marketplace_add_source", { input });
    });
  });

  describe("marketplaceRemoveSource", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.marketplaceRemoveSource("src1")).rejects.toThrow(
        "marketplace_remove_source requires Tauri runtime",
      );
    });

    it("calls marketplace_remove_source", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await extensionCommands.marketplaceRemoveSource("src1");
      expect(invoke).toHaveBeenCalledWith("marketplace_remove_source", { id: "src1" });
    });
  });

  describe("marketplaceGetRemoveSourcePlan", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.marketplaceGetRemoveSourcePlan("src1")).rejects.toThrow(
        "marketplace_get_remove_source_plan requires Tauri runtime",
      );
    });

    it("calls marketplace_get_remove_source_plan", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const plan = { affectedItems: [] } as any;
      vi.mocked(invoke).mockResolvedValue(plan);

      const result = await extensionCommands.marketplaceGetRemoveSourcePlan("src1");
      expect(result).toEqual(plan);
      expect(invoke).toHaveBeenCalledWith("marketplace_get_remove_source_plan", { id: "src1" });
    });
  });

  describe("marketplaceRefreshSource", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.marketplaceRefreshSource("src1")).rejects.toThrow(
        "marketplace_refresh_source requires Tauri runtime",
      );
    });

    it("calls marketplace_refresh_source", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const source = { id: "src1" } as any;
      vi.mocked(invoke).mockResolvedValue(source);

      const result = await extensionCommands.marketplaceRefreshSource("src1");
      expect(result).toEqual(source);
      expect(invoke).toHaveBeenCalledWith("marketplace_refresh_source", { id: "src1" });
    });
  });

  describe("marketplaceListItems", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await extensionCommands.marketplaceListItems();
      expect(result).toEqual([]);
    });

    it("calls marketplace_list_items", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const items = [{ id: "item1", name: "Cool Extension" }] as any;
      vi.mocked(invoke).mockResolvedValue(items);

      const result = await extensionCommands.marketplaceListItems();
      expect(result).toEqual(items);
      expect(invoke).toHaveBeenCalledWith("marketplace_list_items");
    });
  });

  describe("marketplaceInstallItem", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(extensionCommands.marketplaceInstallItem("item1")).rejects.toThrow(
        "marketplace_install_item requires Tauri runtime",
      );
    });

    it("calls marketplace_install_item", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const plugin = { id: "item1-installed" } as any;
      vi.mocked(invoke).mockResolvedValue(plugin);

      const result = await extensionCommands.marketplaceInstallItem("item1");
      expect(result).toEqual(plugin);
      expect(invoke).toHaveBeenCalledWith("marketplace_install_item", { id: "item1" });
    });
  });
});
