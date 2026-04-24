import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke, isTauri } from "@tauri-apps/api/core";

import * as settingsCommands from "./settings-commands";

describe("settings-commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ---------------------------------------------------------------------------
  // Settings KV
  // ---------------------------------------------------------------------------
  describe("settingsGet", () => {
    it("returns null when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.settingsGet("theme");
      expect(result).toBeNull();
      expect(invoke).not.toHaveBeenCalled();
    });

    it("calls settings_get with key when in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const mockValue = { key: "theme", value: "dark" };
      vi.mocked(invoke).mockResolvedValue(mockValue);

      const result = await settingsCommands.settingsGet("theme");
      expect(result).toEqual(mockValue);
      expect(invoke).toHaveBeenCalledWith("settings_get", { key: "theme" });
    });
  });

  describe("settingsGetAll", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.settingsGetAll();
      expect(result).toEqual([]);
    });

    it("calls settings_get_all", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const items = [{ key: "a", value: "1" }, { key: "b", value: "2" }];
      vi.mocked(invoke).mockResolvedValue(items);

      const result = await settingsCommands.settingsGetAll();
      expect(result).toEqual(items);
      expect(invoke).toHaveBeenCalledWith("settings_get_all");
    });
  });

  describe("settingsSet", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(settingsCommands.settingsSet("k", "v")).rejects.toThrow(
        "settings_set requires Tauri runtime",
      );
    });

    it("calls settings_set with key and value", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await settingsCommands.settingsSet("theme", "dark");
      expect(invoke).toHaveBeenCalledWith("settings_set", { key: "theme", value: "dark" });
    });
  });

  // ---------------------------------------------------------------------------
  // Policies KV
  // ---------------------------------------------------------------------------
  describe("policyGet", () => {
    it("returns null when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.policyGet("auto_approve");
      expect(result).toBeNull();
    });

    it("calls policy_get", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const val = { key: "auto_approve", value: "true" };
      vi.mocked(invoke).mockResolvedValue(val);

      const result = await settingsCommands.policyGet("auto_approve");
      expect(result).toEqual(val);
      expect(invoke).toHaveBeenCalledWith("policy_get", { key: "auto_approve" });
    });
  });

  describe("policyGetAll", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.policyGetAll();
      expect(result).toEqual([]);
    });

    it("calls policy_get_all", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const items = [{ key: "p1", value: "v1" }];
      vi.mocked(invoke).mockResolvedValue(items);

      const result = await settingsCommands.policyGetAll();
      expect(result).toEqual(items);
    });
  });

  describe("policySet", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(settingsCommands.policySet("k", "v")).rejects.toThrow(
        "policy_set requires Tauri runtime",
      );
    });

    it("calls policy_set", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await settingsCommands.policySet("key", "val");
      expect(invoke).toHaveBeenCalledWith("policy_set", { key: "key", value: "val" });
    });
  });

  // ---------------------------------------------------------------------------
  // Providers
  // ---------------------------------------------------------------------------
  describe("providerCatalogList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.providerCatalogList();
      expect(result).toEqual([]);
    });

    it("calls provider_catalog_list", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const catalog = [{ key: "openai", name: "OpenAI" }] as any;
      vi.mocked(invoke).mockResolvedValue(catalog);

      const result = await settingsCommands.providerCatalogList();
      expect(result).toEqual(catalog);
      expect(invoke).toHaveBeenCalledWith("provider_catalog_list");
    });
  });

  describe("providerSettingsGetAll", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.providerSettingsGetAll();
      expect(result).toEqual([]);
    });

    it("calls provider_settings_get_all", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const providers = [{ id: "p1" }] as any;
      vi.mocked(invoke).mockResolvedValue(providers);

      const result = await settingsCommands.providerSettingsGetAll();
      expect(result).toEqual(providers);
    });
  });

  describe("providerSettingsFetchModels", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.providerSettingsFetchModels("p1"),
      ).rejects.toThrow("provider_settings_fetch_models requires Tauri runtime");
    });

    it("calls provider_settings_fetch_models with id", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const updated = { id: "p1", models: ["gpt-4"] } as any;
      vi.mocked(invoke).mockResolvedValue(updated);

      const result = await settingsCommands.providerSettingsFetchModels("p1");
      expect(result).toEqual(updated);
      expect(invoke).toHaveBeenCalledWith("provider_settings_fetch_models", { id: "p1" });
    });
  });

  describe("providerSettingsUpsertBuiltin", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.providerSettingsUpsertBuiltin("openai", {} as any),
      ).rejects.toThrow("provider_settings_upsert_builtin requires Tauri runtime");
    });

    it("calls provider_settings_upsert_builtin with providerKey and input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { api_key: "sk-xxx" } as any;
      const resultDto = { key: "openai" } as any;
      vi.mocked(invoke).mockResolvedValue(resultDto);

      const result = await settingsCommands.providerSettingsUpsertBuiltin("openai", input);
      expect(result).toEqual(resultDto);
      expect(invoke).toHaveBeenCalledWith("provider_settings_upsert_builtin", {
        providerKey: "openai",
        input,
      });
    });
  });

  describe("providerSettingsCreateCustom", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.providerSettingsCreateCustom({} as any),
      ).rejects.toThrow("provider_settings_create_custom requires Tauri runtime");
    });

    it("calls provider_settings_create_custom", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { name: "custom" } as any;
      const dto = { id: "c1" } as any;
      vi.mocked(invoke).mockResolvedValue(dto);

      const result = await settingsCommands.providerSettingsCreateCustom(input);
      expect(result).toEqual(dto);
      expect(invoke).toHaveBeenCalledWith("provider_settings_create_custom", { input });
    });
  });

  describe("providerSettingsUpdateCustom", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.providerSettingsUpdateCustom("c1", {} as any),
      ).rejects.toThrow("provider_settings_update_custom requires Tauri runtime");
    });

    it("calls provider_settings_update_custom", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { name: "updated" } as any;
      const dto = { id: "c1" } as any;
      vi.mocked(invoke).mockResolvedValue(dto);

      const result = await settingsCommands.providerSettingsUpdateCustom("c1", input);
      expect(result).toEqual(dto);
      expect(invoke).toHaveBeenCalledWith("provider_settings_update_custom", { id: "c1", input });
    });
  });

  describe("providerSettingsDeleteCustom", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(settingsCommands.providerSettingsDeleteCustom("c1")).rejects.toThrow(
        "provider_settings_delete_custom requires Tauri runtime",
      );
    });

    it("calls provider_settings_delete_custom", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await settingsCommands.providerSettingsDeleteCustom("c1");
      expect(invoke).toHaveBeenCalledWith("provider_settings_delete_custom", { id: "c1" });
    });
  });

  describe("providerModelTestConnection", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.providerModelTestConnection("p1", "m1"),
      ).rejects.toThrow("provider_model_test_connection requires Tauri runtime");
    });

    it("calls provider_model_test_connection with providerId and modelId", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const testResult = { success: true, latencyMs: 120 } as any;
      vi.mocked(invoke).mockResolvedValue(testResult);

      const result = await settingsCommands.providerModelTestConnection("p1", "m1");
      expect(result).toEqual(testResult);
      expect(invoke).toHaveBeenCalledWith("provider_model_test_connection", {
        providerId: "p1",
        modelId: "m1",
      });
    });
  });

  // ---------------------------------------------------------------------------
  // Agent Profiles
  // ---------------------------------------------------------------------------
  describe("profileList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.profileList();
      expect(result).toEqual([]);
    });

    it("calls profile_list", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const profiles = [{ id: "prof1", name: "Coder" }] as any;
      vi.mocked(invoke).mockResolvedValue(profiles);

      const result = await settingsCommands.profileList();
      expect(result).toEqual(profiles);
      expect(invoke).toHaveBeenCalledWith("profile_list");
    });
  });

  describe("profileCreate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.profileCreate({ name: "New Profile" } as any),
      ).rejects.toThrow("profile_create requires Tauri runtime");
    });

    it("calls profile_create with input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { name: "Reviewer" } as any;
      const profile = { id: "p2", name: "Reviewer" } as any;
      vi.mocked(invoke).mockResolvedValue(profile);

      const result = await settingsCommands.profileCreate(input);
      expect(result).toEqual(profile);
      expect(invoke).toHaveBeenCalledWith("profile_create", { input });
    });
  });

  describe("profileUpdate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.profileUpdate("p1", {} as any),
      ).rejects.toThrow("profile_update requires Tauri runtime");
    });

    it("calls profile_update with id and input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { name: "Updated" } as any;
      const profile = { id: "p1", name: "Updated" } as any;
      vi.mocked(invoke).mockResolvedValue(profile);

      const result = await settingsCommands.profileUpdate("p1", input);
      expect(result).toEqual(profile);
      expect(invoke).toHaveBeenCalledWith("profile_update", { id: "p1", input });
    });
  });

  describe("profileDelete", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(settingsCommands.profileDelete("p1")).rejects.toThrow(
        "profile_delete requires Tauri runtime",
      );
    });

    it("calls profile_delete with id", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await settingsCommands.profileDelete("p1");
      expect(invoke).toHaveBeenCalledWith("profile_delete", { id: "p1" });
    });
  });

  // ---------------------------------------------------------------------------
  // Prompt Commands
  // ---------------------------------------------------------------------------
  describe("promptCommandList", () => {
    it("returns empty array when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      const result = await settingsCommands.promptCommandList();
      expect(result).toEqual([]);
    });

    it("calls prompt_command_list", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const commands = [{ id: "pc1", name: "/review" }] as any;
      vi.mocked(invoke).mockResolvedValue(commands);

      const result = await settingsCommands.promptCommandList();
      expect(result).toEqual(commands);
      expect(invoke).toHaveBeenCalledWith("prompt_command_list");
    });
  });

  describe("promptCommandCreate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.promptCommandCreate({} as any),
      ).rejects.toThrow("prompt_command_create requires Tauri runtime");
    });

    it("calls prompt_command_create with input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { name: "/test", prompt: "Run tests" } as any;
      const cmd = { id: "pc2", name: "/test" } as any;
      vi.mocked(invoke).mockResolvedValue(cmd);

      const result = await settingsCommands.promptCommandCreate(input);
      expect(result).toEqual(cmd);
      expect(invoke).toHaveBeenCalledWith("prompt_command_create", { input });
    });
  });

  describe("promptCommandUpdate", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(
        settingsCommands.promptCommandUpdate("pc1", {} as any),
      ).rejects.toThrow("prompt_command_update requires Tauri runtime");
    });

    it("calls prompt_command_update with id and input", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      const input = { prompt: "Updated prompt" } as any;
      const cmd = { id: "pc1" } as any;
      vi.mocked(invoke).mockResolvedValue(cmd);

      const result = await settingsCommands.promptCommandUpdate("pc1", input);
      expect(result).toEqual(cmd);
      expect(invoke).toHaveBeenCalledWith("prompt_command_update", { id: "pc1", input });
    });
  });

  describe("promptCommandDelete", () => {
    it("throws when not in Tauri", async () => {
      vi.mocked(isTauri).mockReturnValue(false);
      await expect(settingsCommands.promptCommandDelete("pc1")).rejects.toThrow(
        "prompt_command_delete requires Tauri runtime",
      );
    });

    it("calls prompt_command_delete with id", async () => {
      vi.mocked(isTauri).mockReturnValue(true);
      vi.mocked(invoke).mockResolvedValue(undefined);

      await settingsCommands.promptCommandDelete("pc1");
      expect(invoke).toHaveBeenCalledWith("prompt_command_delete", { id: "pc1" });
    });
  });
});
