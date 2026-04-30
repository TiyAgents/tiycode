import type { ComponentProps } from "react";
import { useEffect } from "react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import type { TranslationKey } from "@/i18n";
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
  type NewWorktreeDialogContext,
} from "@/modules/workbench-shell/ui/new-worktree-dialog";
import type { ProjectOption } from "@/modules/workbench-shell/model/types";
import type { WorkspaceDto } from "@/shared/types/api";
import type {
  MarketplaceRemoveSourcePlan,
  MarketplaceSourceInput,
  McpServerConfigInput,
} from "@/shared/types/extensions";
import {
  uiLayoutStore,
  closeOverlay,
  setActiveSettingsCategory,
  setSelectedDiffSelection,
  setShowOnboarding,
} from "@/modules/workbench-shell/model/ui-layout-store";
import { useStore, shallowEqual } from "@/shared/lib/create-store";
import { settingsStore } from "@/modules/settings-center/model/settings-store";
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

type SettingsOverlayProps = ComponentProps<typeof SettingsCenterOverlay>;
type ExtensionsOverlayProps = ComponentProps<typeof ExtensionsCenterOverlay>;
type OnboardingWizardProps = ComponentProps<typeof OnboardingWizard>;

type DashboardOverlaysProps = {
  resolvedWorkspaceId: string | null;
  overlayContentRef: SettingsOverlayProps["contentRef"];
  configDiagnostics: SettingsOverlayProps["configDiagnostics"];
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  data: SettingsOverlayProps["systemMetadata"];
  theme: ThemePreference;
  updateStatus: string | null;
  handleCheckUpdates: SettingsOverlayProps["onCheckUpdates"];
  handleLanguageSelect: SettingsOverlayProps["onSelectLanguage"];
  handleThemeSelect: SettingsOverlayProps["onSelectTheme"];
  extensionDetailById: ExtensionsOverlayProps["detailById"];
  extensionsError: ExtensionsOverlayProps["error"];
  extensions: ExtensionsOverlayProps["extensions"];
  areExtensionsLoading: ExtensionsOverlayProps["isLoading"];
  marketplaceItems: ExtensionsOverlayProps["marketplaceItems"];
  marketplaceSources: ExtensionsOverlayProps["marketplaceSources"];
  mcpServers: ExtensionsOverlayProps["mcpServers"];
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
  appUpdater: AppUpdater;
  setLanguage: OnboardingWizardProps["onSelectLanguage"];
  setTheme: OnboardingWizardProps["onSelectTheme"];
  worktreeDialogContext: NewWorktreeDialogContext | null;
  setWorktreeDialogContext: (context: NewWorktreeDialogContext | null) => void;
  buildProjectOptionFromWorkspace: (workspace: WorkspaceDto, language: LanguagePreference) => ProjectOption | null;
  activateWorkspaceAsNewThreadTarget: (workspaceId: string, project: ProjectOption) => void;
  syncWorkspaceSidebar: () => Promise<void>;
  t: (key: TranslationKey) => string;
};

export function DashboardOverlays(props: DashboardOverlaysProps) {
  const {
    resolvedWorkspaceId,
    overlayContentRef,
    configDiagnostics,
    isCheckingUpdates,
    language,
    data,
    theme,
    updateStatus,
    handleCheckUpdates,
    handleLanguageSelect,
    handleThemeSelect,
    extensionDetailById,
    extensionsError,
    extensions,
    areExtensionsLoading,
    marketplaceItems,
    marketplaceSources,
    mcpServers,
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
    setLanguage,
    setTheme,
    worktreeDialogContext,
    setWorktreeDialogContext,
    buildProjectOptionFromWorkspace,
    activateWorkspaceAsNewThreadTarget,
    syncWorkspaceSidebar,
    t,
  } = props;

  // ── Subscribe to uiLayoutStore ──────────────────────────────────
  const activeOverlay = useStore(uiLayoutStore, (s) => s.activeOverlay);
  const activeSettingsCategory = useStore(uiLayoutStore, (s) => s.activeSettingsCategory);
  const selectedDiffSelection = useStore(uiLayoutStore, (s) => s.selectedDiffSelection);
  const showOnboarding = useStore(uiLayoutStore, (s) => s.showOnboarding);

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
        onClose={() => setWorktreeDialogContext(null)}
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
          activateWorkspaceAsNewThreadTarget(workspace.id, nextProject);
          void syncWorkspaceSidebar().catch(() => {});
        }}
      />
    </>
  );
}
