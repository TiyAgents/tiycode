import type { ComponentProps } from "react";
import { useEffect, useMemo } from "react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import { useLanguage } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import { useTheme } from "@/app/providers/theme-provider";
import { useT } from "@/i18n";
import type { ExtensionScope } from "@/modules/extensions-center/model/use-extensions-controller";
import { ExtensionsCenterOverlay } from "@/modules/extensions-center/ui/extensions-center-overlay";
import { OnboardingWizard } from "@/modules/onboarding/ui/onboarding-wizard";
import { SettingsCenterOverlay } from "@/modules/settings-center/ui/settings-center-overlay";
import type { AppUpdater } from "@/modules/workbench-shell/hooks/use-app-updater";
import { UpdateAvailableDialog } from "@/modules/workbench-shell/ui/update-available-dialog";
import {
  GitDiffPreviewPanel,
} from "@/modules/workbench-shell/ui/source-control-panels";
import {
  NewWorktreeDialog,
} from "@/modules/workbench-shell/ui/new-worktree-dialog";
import type { MarketplaceRemoveSourcePlan, MarketplaceSourceInput, McpServerConfigInput } from "@/shared/types/extensions";
import { useSystemMetadata } from "@/features/system-info/model/use-system-metadata";
import {
  uiLayoutStore,
  closeOverlay,
  setActiveSettingsCategory,
  setOpenSettingsSection,
  setSelectedDiffSelection,
  setShowOnboarding,
} from "@/modules/workbench-shell/model/ui-layout-store";
import { useStore, shallowEqual } from "@/shared/lib/create-store";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
import { projectStore } from "@/modules/workbench-shell/model/project-store";
import {
  addAgentProfile,
  addAllowEntry,
  addCommand,
  addDenyEntry,
  addProvider,
  addWorkspace,
  addWritableRoot,
  duplicateAgentProfile,
  removeAgentProfile,
  removeAllowEntry,
  removeCommand,
  removeDenyEntry,
  removeProvider,
  removeWorkspace,
  removeWritableRoot,
  setActiveAgentProfile,
  setDefaultWorkspace,
  updateAgentProfile,
  updateAllowEntry,
  updateCommand,
  updateDenyEntry,
  updateGeneralPreference,
  updatePolicySetting,
  updateProvider,
  updateTerminalSetting,
  fetchProviderModels,
  testProviderModelConnection,
  updateWritableRoot,
} from "@/modules/settings-center/model/settings-ipc-actions";
import {
  buildProjectOptionFromWorkspace,
} from "@/modules/workbench-shell/ui/dashboard-workbench-logic";
import {
  activateWorkspace,
} from "@/modules/workbench-shell/model/workbench-actions";
import {
  getWorkspaceBindingId,
} from "@/modules/workbench-shell/model/workspace-path-bindings";

type SettingsOverlayProps = ComponentProps<typeof SettingsCenterOverlay>;
type ExtensionsOverlayProps = ComponentProps<typeof ExtensionsCenterOverlay>;

/**
 * Props that still need to flow through from DashboardWorkbench because they
 * originate from hooks / controllers that must be instantiated at the top
 * level (useExtensionsController, useAppUpdater).
 */
type DashboardOverlaysProps = {
  /** DOM ref for overlay content selection (Cmd+A). */
  overlayContentRef: SettingsOverlayProps["contentRef"];
  /** Extension/marketplace data from useExtensionsController. */
  extensionDetailById: ExtensionsOverlayProps["detailById"];
  extensionsError: ExtensionsOverlayProps["error"];
  extensions: ExtensionsOverlayProps["extensions"];
  areExtensionsLoading: ExtensionsOverlayProps["isLoading"];
  marketplaceItems: ExtensionsOverlayProps["marketplaceItems"];
  marketplaceSources: ExtensionsOverlayProps["marketplaceSources"];
  mcpServers: ExtensionsOverlayProps["mcpServers"];
  configDiagnostics: SettingsOverlayProps["configDiagnostics"];
  refreshExtensions: (scope: ExtensionScope) => Promise<void>;
  currentExtensionScope: ExtensionScope;
  loadExtensionDetail: (id: string, scope: ExtensionScope) => ReturnType<ExtensionsOverlayProps["onLoadDetail"]>;
  resolveItemScope: (id: string) => ExtensionScope;
  loadSkillPreview: (id: string, scope: ExtensionScope) => ReturnType<ExtensionsOverlayProps["onLoadSkillPreview"]>;
  enableExtension: (id: string, scope: ExtensionScope) => Promise<void>;
  disableExtension: (id: string, scope: ExtensionScope) => Promise<void>;
  uninstallExtension: (id: string, scope: ExtensionScope) => Promise<void>;
  addMarketplaceSource: (input: MarketplaceSourceInput) => Promise<void>;
  getMarketplaceSourceRemovePlan: (id: string) => Promise<MarketplaceRemoveSourcePlan>;
  removeMarketplaceSource: (id: string) => Promise<void>;
  refreshMarketplaceSource: (id: string) => Promise<void>;
  installMarketplaceItem: (id: string) => Promise<void>;
  addMcpServer: (input: McpServerConfigInput, scope: ExtensionScope) => Promise<void>;
  updateMcpServer: (id: string, input: McpServerConfigInput, scope: ExtensionScope) => Promise<void>;
  removeMcpServer: (id: string, scope: ExtensionScope) => Promise<void>;
  restartMcpServer: (id: string, scope: ExtensionScope) => Promise<void>;
  rescanSkills: (scope: ExtensionScope) => Promise<void>;
  enableSkill: (id: string, scope: ExtensionScope) => Promise<void>;
  disableSkill: (id: string, scope: ExtensionScope) => Promise<void>;
  skillPreviewById: ExtensionsOverlayProps["skillPreviewById"];
  extensionSkills: ExtensionsOverlayProps["skills"];
  /** App updater instance (must be a singleton from useAppUpdater). */
  appUpdater: AppUpdater;
  /** Coalesced sidebar sync (ref-stable from DashboardWorkbench). */
  syncWorkspaceSidebar: () => Promise<void>;
};

export function DashboardOverlays(props: DashboardOverlaysProps) {
  const {
    overlayContentRef,
    extensionDetailById,
    extensionsError,
    extensions,
    areExtensionsLoading,
    marketplaceItems,
    marketplaceSources,
    mcpServers,
    configDiagnostics,
    refreshExtensions,
    currentExtensionScope,
    loadExtensionDetail,
    resolveItemScope,
    loadSkillPreview,
    enableExtension,
    disableExtension,
    uninstallExtension,
    addMarketplaceSource,
    getMarketplaceSourceRemovePlan,
    removeMarketplaceSource,
    refreshMarketplaceSource,
    installMarketplaceItem,
    addMcpServer,
    updateMcpServer,
    removeMcpServer,
    restartMcpServer,
    rescanSkills,
    enableSkill,
    disableSkill,
    skillPreviewById,
    extensionSkills,
    appUpdater,
    syncWorkspaceSidebar,
  } = props;

  // ── Global hooks (replaces language/theme/systemMetadata/t props) ──
  const { language, setLanguage } = useLanguage();
  const { theme, setTheme } = useTheme();
  const { data } = useSystemMetadata();
  const t = useT();

  // ── Derived from appUpdater (replaces isCheckingUpdates + updateStatus props) ──
  const isCheckingUpdates = appUpdater.phase === "checking";
  const updateStatus = useMemo(() => {
    if (appUpdater.phase !== "upToDate") return null;
    return t("dashboard.upToDate", { version: data?.version ?? "0.1.0" });
  }, [appUpdater.phase, data?.version, t]);

  // ── Derived workspace ID (replaces resolvedWorkspaceId prop) ──
  const selectedProject = useStore(projectStore, (s) => s.selectedProject);
  const terminalWorkspaceBindings = useStore(projectStore, (s) => s.terminalWorkspaceBindings, shallowEqual);
  const resolvedWorkspaceId = getWorkspaceBindingId(
    terminalWorkspaceBindings,
    selectedProject?.path ?? null,
  );

  // ── Subscribe to uiLayoutStore ──────────────────────────────────
  const activeOverlay = useStore(uiLayoutStore, (s) => s.activeOverlay);
  const activeSettingsCategory = useStore(uiLayoutStore, (s) => s.activeSettingsCategory);
  const selectedDiffSelection = useStore(uiLayoutStore, (s) => s.selectedDiffSelection);
  const showOnboarding = useStore(uiLayoutStore, (s) => s.showOnboarding);
  const worktreeDialogContext = useStore(uiLayoutStore, (s) => s.worktreeDialogContext);

  // ── Subscribe to settingsStore (replaces ~30 props) ─────────────
  const agentProfiles = useStore(settingsStore, (s) => s.agentProfiles, shallowEqual);
  const activeAgentProfileId = useStore(settingsStore, (s) => s.activeAgentProfileId);
  const generalPreferences = useStore(settingsStore, (s) => s.general, shallowEqual);
  const policy = useStore(settingsStore, (s) => s.policy, shallowEqual);
  const terminal = useStore(settingsStore, (s) => s.terminal, shallowEqual);
  const availableShells = useStore(settingsStore, (s) => s.availableShells, shallowEqual);
  const commandEntries = useStore(settingsStore, (s) => s.commands, shallowEqual);
  const providerCatalog = useStore(settingsStore, (s) => s.providerCatalog, shallowEqual);
  const providers = useStore(settingsStore, (s) => s.providers, shallowEqual);
  const settingsWorkspaces = useStore(settingsStore, (s) => s.workspaces, shallowEqual);
  const hydrationPhase = useStore(settingsStore, (s) => s.hydrationPhase);
  const settingsHydrated =
    hydrationPhase === "hydrated" || hydrationPhase === "phase1_ready";

  const isSettingsOpen = activeOverlay === "settings";
  const isMarketplaceOpen = activeOverlay === "marketplace";

  // ── Handlers that were previously props ─────────────────────────
  const handleCheckUpdates = appUpdater.checkForUpdates;
  const handleLanguageSelect = (nextLanguage: LanguagePreference) => {
    setLanguage(nextLanguage);
    setOpenSettingsSection("language");
  };
  const handleThemeSelect = (nextTheme: ThemePreference) => {
    setTheme(nextTheme);
    setOpenSettingsSection("theme");
  };

  // ── Escape key: close overlay or diff selection ────────────────
  useEffect(() => {
    if (typeof window === "undefined") return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;

      if (activeOverlay) {
        closeOverlay();
        return;
      }
      if (selectedDiffSelection) {
        setSelectedDiffSelection(null);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeOverlay, selectedDiffSelection]);

  return (
    <>
      {selectedDiffSelection ? (
        <GitDiffPreviewPanel
          workspaceId={resolvedWorkspaceId}
          selection={selectedDiffSelection}
          onClose={() => setSelectedDiffSelection(null)}
        />
      ) : null}

      {isSettingsOpen ? (
        <SettingsCenterOverlay
          activeCategory={activeSettingsCategory}
          agentProfiles={agentProfiles}
          activeAgentProfileId={activeAgentProfileId}
          contentRef={overlayContentRef}
          configDiagnostics={configDiagnostics}
          generalPreferences={generalPreferences}
          isCheckingUpdates={isCheckingUpdates}
          language={language}
          policy={policy}
          terminal={terminal}
          availableShells={availableShells}
          commands={{ commands: commandEntries }}
          providerCatalog={providerCatalog}
          providers={providers}
          systemMetadata={data}
          theme={theme}
          updateStatus={updateStatus}
          workspaces={settingsWorkspaces}
          onAddAgentProfile={addAgentProfile}
          onAddAllowEntry={addAllowEntry}
          onAddCommand={addCommand}
          onAddDenyEntry={addDenyEntry}
          onAddProvider={addProvider}
          onAddWorkspace={addWorkspace}
          onAddWritableRoot={addWritableRoot}
          onCheckUpdates={handleCheckUpdates}
          onClose={closeOverlay}
          onDuplicateAgentProfile={duplicateAgentProfile}
          onRemoveAgentProfile={removeAgentProfile}
          onRemoveAllowEntry={removeAllowEntry}
          onRemoveCommand={removeCommand}
          onRemoveDenyEntry={removeDenyEntry}
          onRemoveProvider={removeProvider}
          onRemoveWorkspace={removeWorkspace}
          onRemoveWritableRoot={removeWritableRoot}
          onSelectCategory={setActiveSettingsCategory}
          onSelectLanguage={handleLanguageSelect}
          onSelectTheme={handleThemeSelect}
          onSetActiveAgentProfile={setActiveAgentProfile}
          onSetDefaultWorkspace={setDefaultWorkspace}
          onUpdateAgentProfile={updateAgentProfile}
          onUpdateAllowEntry={updateAllowEntry}
          onUpdateCommand={updateCommand}
          onUpdateDenyEntry={updateDenyEntry}
          onUpdateGeneralPreference={updateGeneralPreference}
          onUpdatePolicySetting={updatePolicySetting}
          onUpdateProvider={updateProvider}
          onUpdateTerminalSetting={updateTerminalSetting}
          onFetchProviderModels={fetchProviderModels}
          onTestProviderModelConnection={testProviderModelConnection}
          onUpdateWritableRoot={updateWritableRoot}
        />
      ) : null}

      {isMarketplaceOpen ? (
        <ExtensionsCenterOverlay
          contentRef={overlayContentRef}
          detailById={extensionDetailById}
          error={extensionsError}
          extensions={extensions}
          configDiagnostics={configDiagnostics}
          isLoading={areExtensionsLoading}
          marketplaceItems={marketplaceItems}
          marketplaceSources={marketplaceSources}
          mcpServers={mcpServers}
          onClose={closeOverlay}
          onRefresh={() => void refreshExtensions(currentExtensionScope)}
          onLoadDetail={(id) => loadExtensionDetail(id, resolveItemScope(id))}
          onLoadSkillPreview={(id) => loadSkillPreview(id, resolveItemScope(id))}
          onEnableExtension={(id) => enableExtension(id, resolveItemScope(id))}
          onDisableExtension={(id) => disableExtension(id, resolveItemScope(id))}
          onUninstallExtension={(id) => uninstallExtension(id, resolveItemScope(id))}
          onAddMarketplaceSource={addMarketplaceSource}
          onGetMarketplaceSourceRemovePlan={getMarketplaceSourceRemovePlan}
          onRemoveMarketplaceSource={removeMarketplaceSource}
          onRefreshMarketplaceSource={refreshMarketplaceSource}
          onInstallMarketplaceItem={installMarketplaceItem}
          onAddMcpServer={(input) => addMcpServer(input, "global")}
          onUpdateMcpServer={(id, input) => updateMcpServer(id, input, resolveItemScope(id))}
          onRemoveMcpServer={(id) => removeMcpServer(id, resolveItemScope(id))}
          onRestartMcpServer={(id) => restartMcpServer(id, resolveItemScope(id))}
          onRescanSkills={() => rescanSkills(currentExtensionScope)}
          onEnableSkill={(id) => enableSkill(id, resolveItemScope(id))}
          onDisableSkill={(id) => disableSkill(id, resolveItemScope(id))}
          skillPreviewById={skillPreviewById}
          skills={extensionSkills}
        />
      ) : null}

      <UpdateAvailableDialog
        phase={appUpdater.phase}
        updateInfo={appUpdater.updateInfo}
        downloadProgress={appUpdater.downloadProgress}
        errorMessage={appUpdater.errorMessage}
        onDownloadAndInstall={appUpdater.downloadAndInstall}
        onRestart={appUpdater.restartApp}
        onRetry={appUpdater.checkForUpdates}
        onDismiss={appUpdater.dismiss}
      />

      {showOnboarding && settingsHydrated ? (
        <OnboardingWizard
          language={language}
          theme={theme}
          providerCatalog={providerCatalog}
          providers={providers}
          agentProfiles={agentProfiles}
          activeAgentProfileId={activeAgentProfileId}
          onSelectLanguage={setLanguage}
          onSelectTheme={setTheme}
          onAddProvider={addProvider}
          onUpdateProvider={updateProvider}
          onFetchProviderModels={fetchProviderModels}
          onUpdateAgentProfile={updateAgentProfile}
          onDismiss={() => setShowOnboarding(false)}
        />
      ) : null}

      <NewWorktreeDialog
        context={worktreeDialogContext}
        onClose={() => uiLayoutStore.setState({ worktreeDialogContext: null })}
        onCreated={(workspace) => {
          const nextProject =
            buildProjectOptionFromWorkspace(workspace, language) ?? {
              id: workspace.id,
              name: workspace.name,
              path: workspace.canonicalPath || workspace.path,
              lastOpenedLabel: t("time.justNow"),
              kind: workspace.kind,
              parentWorkspaceId: workspace.parentWorkspaceId,
              worktreeHash: workspace.worktreeName
                ? workspace.worktreeName.slice(0, 6)
                : null,
              branch: workspace.branch,
            };
          activateWorkspace(workspace.id, nextProject);
          void syncWorkspaceSidebar().catch(() => {});
        }}
      />
    </>
  );
}
