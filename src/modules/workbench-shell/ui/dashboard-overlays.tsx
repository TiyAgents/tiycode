import type { ComponentProps } from "react";
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
  type GitDiffSelection,
} from "@/modules/workbench-shell/ui/source-control-panels";
import {
  NewWorktreeDialog,
  type NewWorktreeDialogContext,
} from "@/modules/workbench-shell/ui/new-worktree-dialog";
import type { WorkbenchOverlay, ProjectOption } from "@/modules/workbench-shell/model/types";
import type { WorkspaceDto } from "@/shared/types/api";
import type {
  MarketplaceRemoveSourcePlan,
  MarketplaceSourceInput,
  McpServerConfigInput,
} from "@/shared/types/extensions";

type SettingsOverlayProps = ComponentProps<typeof SettingsCenterOverlay>;
type ExtensionsOverlayProps = ComponentProps<typeof ExtensionsCenterOverlay>;
type OnboardingWizardProps = ComponentProps<typeof OnboardingWizard>;

type DashboardOverlaysProps = {
  selectedDiffSelection: GitDiffSelection | null;
  resolvedWorkspaceId: string | null;
  setSelectedDiffSelection: (selection: GitDiffSelection | null) => void;
  isSettingsOpen: boolean;
  activeSettingsCategory: SettingsOverlayProps["activeCategory"];
  agentProfiles: SettingsOverlayProps["agentProfiles"];
  activeAgentProfileId: string;
  overlayContentRef: SettingsOverlayProps["contentRef"];
  configDiagnostics: SettingsOverlayProps["configDiagnostics"];
  generalPreferences: SettingsOverlayProps["generalPreferences"];
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  policy: SettingsOverlayProps["policy"];
  terminal: SettingsOverlayProps["terminal"];
  availableShells: SettingsOverlayProps["availableShells"];
  commands: SettingsOverlayProps["commands"];
  providerCatalog: SettingsOverlayProps["providerCatalog"];
  providers: SettingsOverlayProps["providers"];
  data: SettingsOverlayProps["systemMetadata"];
  theme: ThemePreference;
  updateStatus: string | null;
  settingsWorkspaces: SettingsOverlayProps["workspaces"];
  addAgentProfile: SettingsOverlayProps["onAddAgentProfile"];
  addAllowEntry: SettingsOverlayProps["onAddAllowEntry"];
  addCommand: SettingsOverlayProps["onAddCommand"];
  addDenyEntry: SettingsOverlayProps["onAddDenyEntry"];
  addProvider: SettingsOverlayProps["onAddProvider"];
  addWorkspace: SettingsOverlayProps["onAddWorkspace"];
  addWritableRoot: SettingsOverlayProps["onAddWritableRoot"];
  handleCheckUpdates: SettingsOverlayProps["onCheckUpdates"];
  setActiveOverlay: (overlay: WorkbenchOverlay) => void;
  duplicateAgentProfile: SettingsOverlayProps["onDuplicateAgentProfile"];
  removeAgentProfile: SettingsOverlayProps["onRemoveAgentProfile"];
  removeAllowEntry: SettingsOverlayProps["onRemoveAllowEntry"];
  removeCommand: SettingsOverlayProps["onRemoveCommand"];
  removeDenyEntry: SettingsOverlayProps["onRemoveDenyEntry"];
  removeProvider: SettingsOverlayProps["onRemoveProvider"];
  removeWorkspace: SettingsOverlayProps["onRemoveWorkspace"];
  removeWritableRoot: SettingsOverlayProps["onRemoveWritableRoot"];
  setActiveSettingsCategory: SettingsOverlayProps["onSelectCategory"];
  handleLanguageSelect: SettingsOverlayProps["onSelectLanguage"];
  handleThemeSelect: SettingsOverlayProps["onSelectTheme"];
  setActiveAgentProfile: SettingsOverlayProps["onSetActiveAgentProfile"];
  setDefaultWorkspace: SettingsOverlayProps["onSetDefaultWorkspace"];
  updateAgentProfile: SettingsOverlayProps["onUpdateAgentProfile"];
  updateAllowEntry: SettingsOverlayProps["onUpdateAllowEntry"];
  updateCommand: SettingsOverlayProps["onUpdateCommand"];
  updateDenyEntry: SettingsOverlayProps["onUpdateDenyEntry"];
  updateGeneralPreference: SettingsOverlayProps["onUpdateGeneralPreference"];
  updatePolicySetting: SettingsOverlayProps["onUpdatePolicySetting"];
  updateProvider: SettingsOverlayProps["onUpdateProvider"];
  updateTerminalSetting: SettingsOverlayProps["onUpdateTerminalSetting"];
  fetchProviderModels: SettingsOverlayProps["onFetchProviderModels"];
  testProviderModelConnection: SettingsOverlayProps["onTestProviderModelConnection"];
  updateWritableRoot: SettingsOverlayProps["onUpdateWritableRoot"];
  isMarketplaceOpen: boolean;
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
  showOnboarding: boolean;
  settingsHydrated: boolean;
  setLanguage: OnboardingWizardProps["onSelectLanguage"];
  setTheme: OnboardingWizardProps["onSelectTheme"];
  setShowOnboarding: (show: boolean) => void;
  worktreeDialogContext: NewWorktreeDialogContext | null;
  setWorktreeDialogContext: (context: NewWorktreeDialogContext | null) => void;
  buildProjectOptionFromWorkspace: (workspace: WorkspaceDto, language: LanguagePreference) => ProjectOption | null;
  activateWorkspaceAsNewThreadTarget: (workspaceId: string, project: ProjectOption) => void;
  syncWorkspaceSidebar: () => Promise<void>;
  t: (key: TranslationKey) => string;
};

export function DashboardOverlays(props: DashboardOverlaysProps) {
  const {
    selectedDiffSelection,
    resolvedWorkspaceId,
    setSelectedDiffSelection,
    isSettingsOpen,
    activeSettingsCategory,
    agentProfiles,
    activeAgentProfileId,
    overlayContentRef,
    configDiagnostics,
    generalPreferences,
    isCheckingUpdates,
    language,
    policy,
    terminal,
    availableShells,
    commands,
    providerCatalog,
    providers,
    data,
    theme,
    updateStatus,
    settingsWorkspaces,
    addAgentProfile,
    addAllowEntry,
    addCommand,
    addDenyEntry,
    addProvider,
    addWorkspace,
    addWritableRoot,
    handleCheckUpdates,
    setActiveOverlay,
    duplicateAgentProfile,
    removeAgentProfile,
    removeAllowEntry,
    removeCommand,
    removeDenyEntry,
    removeProvider,
    removeWorkspace,
    removeWritableRoot,
    setActiveSettingsCategory,
    handleLanguageSelect,
    handleThemeSelect,
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
    isMarketplaceOpen,
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
    showOnboarding,
    settingsHydrated,
    setLanguage,
    setTheme,
    setShowOnboarding,
    worktreeDialogContext,
    setWorktreeDialogContext,
    buildProjectOptionFromWorkspace,
    activateWorkspaceAsNewThreadTarget,
    syncWorkspaceSidebar,
    t,
  } = props;

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
                commands={commands}
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
                onClose={() => setActiveOverlay(null)}
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
                onClose={() => setActiveOverlay(null)}
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
            />    </>
  );
}
