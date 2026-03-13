import { type ReactNode, type RefObject, useMemo, useState } from "react";
import {
  ArrowLeft,
  Blocks,
  ChevronDown,
  CircleUserRound,
  Download,
  Eye,
  EyeOff,
  FolderOpen,
  FolderPlus,
  GitBranch,
  Monitor,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Sparkles,
  Star,
  Trash2,
} from "lucide-react";
import AnthropicIcon from "@lobehub/icons/es/Anthropic";
import ClaudeIcon from "@lobehub/icons/es/Claude";
import DeepSeekIcon from "@lobehub/icons/es/DeepSeek";
import GeminiIcon from "@lobehub/icons/es/Gemini";
import GoogleIcon from "@lobehub/icons/es/Google";
import LlamaIcon from "@lobehub/icons/es/LlamaIndex";
import MistralIcon from "@lobehub/icons/es/Mistral";
import MoonshotIcon from "@lobehub/icons/es/Moonshot";
import OpenAIIcon from "@lobehub/icons/es/OpenAI";
import OpenRouterIcon from "@lobehub/icons/es/OpenRouter";
import QwenIcon from "@lobehub/icons/es/Qwen";
import StepfunIcon from "@lobehub/icons/es/Stepfun";
import ZenMuxIcon from "@lobehub/icons/es/ZenMux";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import type { SystemMetadata } from "@/shared/types/system";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Separator } from "@/shared/ui/separator";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import type {
  AccessPolicy,
  ApiProtocol,
  ApprovalPolicySettings,
  CommandExecutionPolicy,
  PromptResponseStyle,
  PromptSettings,
  ProviderEntry,
  ProviderModel,
  RiskyCommandConfirmationPolicy,
  SettingsCategory,
  WorkspaceEntry,
} from "@/features/settings/model/use-workbench-settings";

type UserSession = {
  name: string;
  avatar: string;
  email: string;
};

type WorkbenchSettingsOverlayProps = {
  activeCategory: SettingsCategory;
  approvalPolicy: ApprovalPolicySettings;
  contentRef: RefObject<HTMLDivElement | null>;
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  prompts: PromptSettings;
  providers: Array<ProviderEntry>;
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  systemMetadata: SystemMetadata | null;
  theme: ThemePreference;
  updateStatus: string | null;
  userSession: UserSession | null;
  workspaces: Array<WorkspaceEntry>;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onAddWorkspace: (entry: Omit<WorkspaceEntry, "id">) => void;
  onCheckUpdates: () => void;
  onClose: () => void;
  onLogin: () => void;
  onLogout: () => void;
  onRemoveProvider: (id: string) => void;
  onRemoveWorkspace: (id: string) => void;
  onSelectCategory: (category: SettingsCategory) => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onSetDefaultWorkspace: (id: string) => void;
  onUpdateApprovalPolicySetting: <Key extends keyof ApprovalPolicySettings>(
    key: Key,
    value: ApprovalPolicySettings[Key],
  ) => void;
  onUpdatePromptSetting: <Key extends keyof PromptSettings>(key: Key, value: PromptSettings[Key]) => void;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
  onUpdateWorkspace: (id: string, patch: Partial<Omit<WorkspaceEntry, "id">>) => void;
};

const CATEGORY_META: ReadonlyArray<{
  description: string;
  icon: typeof CircleUserRound;
  key: SettingsCategory;
  title: string;
}> = [
  {
    key: "account",
    title: "Account",
    description: "Identity, local session state, and desktop operator details.",
    icon: CircleUserRound,
  },
  {
    key: "general",
    title: "General",
    description: "Core app preferences for language, theme, and desktop runtime.",
    icon: Monitor,
  },
  {
    key: "providers",
    title: "Providers",
    description: "Configure AI model providers, API keys, and available models.",
    icon: Blocks,
  },
  {
    key: "prompts",
    title: "Prompts",
    description: "Default response posture, standing instructions, and project notes.",
    icon: Sparkles,
  },
  {
    key: "approval-policy",
    title: "Approval Policy",
    description: "How much the agent may act before it has to stop and ask.",
    icon: ShieldCheck,
  },
  {
    key: "workspace",
    title: "Workspace",
    description: "Manage project workspaces. New conversations will use these directories instead of creating temporary ones.",
    icon: FolderOpen,
  },
] as const;

const THEME_OPTIONS: ReadonlyArray<{ label: string; value: ThemePreference }> = [
  { label: "System", value: "system" },
  { label: "Light", value: "light" },
  { label: "Dark", value: "dark" },
];

const LANGUAGE_OPTIONS: ReadonlyArray<{ label: string; value: LanguagePreference }> = [
  { label: "English", value: "en" },
  { label: "简体中文", value: "zh-CN" },
];

const RESPONSE_STYLE_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: PromptResponseStyle;
}> = [
  { value: "balanced", label: "Balanced", description: "Clear by default, detailed when needed." },
  { value: "concise", label: "Concise", description: "Short, direct, and low-friction." },
  { value: "guide", label: "Guided", description: "More explanation around tradeoffs and choices." },
] as const;

const COMMAND_EXECUTION_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: CommandExecutionPolicy;
}> = [
  { value: "ask-every-time", label: "Ask", description: "Confirm every command before it runs." },
  { value: "auto-safe", label: "Safe Auto", description: "Run low-risk commands automatically." },
  { value: "full-auto", label: "Full Auto", description: "Let the agent execute commands freely." },
] as const;

const ACCESS_POLICY_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: AccessPolicy;
}> = [
  { value: "ask-first", label: "Ask", description: "Require confirmation the first time." },
  { value: "block", label: "Block", description: "Prevent this class of action." },
  { value: "allow", label: "Allow", description: "Permit it without extra prompts." },
] as const;

const RISKY_COMMAND_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: RiskyCommandConfirmationPolicy;
}> = [
  { value: "always-confirm", label: "Always Confirm", description: "High-risk commands always require explicit approval." },
  { value: "block", label: "Block", description: "Disallow dangerous commands outright." },
] as const;

export function WorkbenchSettingsOverlay({
  activeCategory,
  approvalPolicy,
  contentRef,
  isCheckingUpdates,
  language,
  prompts,
  providers,
  selectedLanguageLabel,
  selectedThemeSummary,
  systemMetadata,
  theme,
  updateStatus,
  userSession,
  workspaces,
  onAddProvider,
  onAddWorkspace,
  onCheckUpdates,
  onClose,
  onLogin,
  onLogout,
  onRemoveProvider,
  onRemoveWorkspace,
  onSelectCategory,
  onSelectLanguage,
  onSelectTheme,
  onSetDefaultWorkspace,
  onUpdateApprovalPolicySetting,
  onUpdatePromptSetting,
  onUpdateProvider,
  onUpdateWorkspace,
}: WorkbenchSettingsOverlayProps) {
  const activeMeta = CATEGORY_META.find((category) => category.key === activeCategory) ?? CATEGORY_META[1];

  return (
    <div className="fixed inset-x-0 bottom-0 top-9 z-20 bg-app-canvas text-app-foreground">
      <div className="flex h-full min-h-0">
        <aside className="hidden w-[320px] shrink-0 overflow-hidden border-r border-app-border bg-app-sidebar md:flex md:flex-col">
          <div className="flex h-full min-h-0 flex-col px-3 pb-3 pt-4">
            <button
              type="button"
              className="inline-flex items-center gap-2 px-3 py-1 text-[12px] text-app-muted transition-colors hover:text-app-foreground"
              onClick={onClose}
            >
              <ArrowLeft className="size-3.5" />
              <span>Back to app</span>
            </button>

            <div className="mt-4 space-y-1">
              {CATEGORY_META.map((category) => {
                const Icon = category.icon;
                const isActive = category.key === activeCategory;

                return (
                  <button
                    key={category.key}
                    type="button"
                    className={cn(
                      "group flex w-full items-center gap-2.5 rounded-xl border px-3 py-2.5 text-left transition-[transform,box-shadow,background-color,border-color,color] duration-200 active:scale-[0.99]",
                      isActive
                        ? "border-app-border-strong bg-app-surface-active text-app-foreground shadow-[0_4px_14px_rgba(15,23,42,0.08)]"
                        : "border-transparent bg-transparent text-app-muted hover:border-app-border hover:bg-app-surface-hover hover:text-app-foreground hover:shadow-[0_4px_14px_rgba(15,23,42,0.08)]",
                    )}
                    onClick={() => onSelectCategory(category.key)}
                  >
                    <Icon
                      className={cn(
                        "size-4 shrink-0 transition-colors duration-200",
                        isActive ? "text-app-foreground" : "text-app-subtle group-hover:text-app-foreground",
                      )}
                    />
                    <span className="truncate text-sm font-medium">{category.title}</span>
                  </button>
                );
              })}
            </div>
          </div>
        </aside>

        <section className="min-w-0 flex-1 min-h-0 select-text bg-app-canvas">
          <div className="flex h-full min-h-0 flex-col">
          <div className="flex items-center justify-between border-b border-app-border px-4 py-3 md:hidden">
            <button
              type="button"
              className="inline-flex items-center gap-2 rounded-lg px-2 py-1.5 text-[12px] text-app-muted transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onClose}
            >
              <ArrowLeft className="size-3.5" />
              <span>Back to app</span>
            </button>
            <p className="text-sm font-medium text-app-foreground">{activeMeta.title}</p>
          </div>

          <div className="overflow-x-auto border-b border-app-border px-4 py-2 md:hidden [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className="flex w-max items-center gap-4">
              {CATEGORY_META.map((category) => {
                const isActive = category.key === activeCategory;

                return (
                  <button
                    key={category.key}
                    type="button"
                    className={cn(
                      "border-b px-0.5 py-1 text-[13px] transition-colors",
                      isActive
                        ? "border-app-border-strong text-app-foreground"
                        : "border-transparent text-app-muted hover:text-app-foreground",
                    )}
                    onClick={() => onSelectCategory(category.key)}
                  >
                    {category.title}
                  </button>
                );
              })}
            </div>
          </div>

          <div
            ref={contentRef}
            className="relative min-h-0 flex-1"
          >
            <div className="h-full overflow-y-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              <div className="mx-auto flex max-w-4xl flex-col gap-6 px-6 pb-28 pt-6">
                {activeCategory === "account" ? (
                  <AccountSettingsPanel
                    appVersion={systemMetadata?.version ?? "0.1.0"}
                    description={activeMeta.description}
                    userSession={userSession}
                    onLogin={onLogin}
                    onLogout={onLogout}
                  />
                ) : null}

                {activeCategory === "general" ? (
                  <GeneralSettingsPanel
                    description={activeMeta.description}
                    isCheckingUpdates={isCheckingUpdates}
                    language={language}
                    runtime={systemMetadata}
                    selectedLanguageLabel={selectedLanguageLabel}
                    selectedThemeSummary={selectedThemeSummary}
                    theme={theme}
                    updateStatus={updateStatus}
                    onCheckUpdates={onCheckUpdates}
                    onSelectLanguage={onSelectLanguage}
                    onSelectTheme={onSelectTheme}
                  />
                ) : null}

                {activeCategory === "workspace" ? (
                  <WorkspaceSettingsPanel
                    description={activeMeta.description}
                    workspaces={workspaces}
                    onAddWorkspace={onAddWorkspace}
                    onRemoveWorkspace={onRemoveWorkspace}
                    onSetDefaultWorkspace={onSetDefaultWorkspace}
                    onUpdateWorkspace={onUpdateWorkspace}
                  />
                ) : null}

                {activeCategory === "providers" ? (
                  <ProviderSettingsPanel
                    description={activeMeta.description}
                    providers={providers}
                    onAddProvider={onAddProvider}
                    onRemoveProvider={onRemoveProvider}
                    onUpdateProvider={onUpdateProvider}
                  />
                ) : null}

                {activeCategory === "prompts" ? (
                  <PromptSettingsPanel
                    description={activeMeta.description}
                    prompts={prompts}
                    onUpdatePromptSetting={onUpdatePromptSetting}
                  />
                ) : null}

                {activeCategory === "approval-policy" ? (
                  <ApprovalSettingsPanel
                    approvalPolicy={approvalPolicy}
                    description={activeMeta.description}
                    onUpdateApprovalPolicySetting={onUpdateApprovalPolicySetting}
                  />
                ) : null}
              </div>
            </div>
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-14 bg-gradient-to-b from-transparent via-app-overlay via-55% to-app-canvas" />
          </div>
        </div>
        </section>
      </div>
    </div>
  );
}

function AccountSettingsPanel({
  appVersion,
  description,
  userSession,
  onLogin,
  onLogout,
}: {
  appVersion: string;
  description: string;
  userSession: UserSession | null;
  onLogin: () => void;
  onLogout: () => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Account" description={description} />

      <SettingsSection title="Session">
        <SettingsRow
          label="Status"
          description={userSession ? "This desktop session is already signed in." : "Use a guest session or sign in for persistent identity."}
          control={<SettingValue value={userSession ? "Authenticated" : "Guest"} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Name"
          description="The local operator profile used in the workbench."
          control={<SettingValue value={userSession?.name ?? "Local desktop user"} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Email"
          description="Currently attached email for this desktop session."
          control={<SettingValue value={userSession?.email ?? "Not connected"} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Authentication"
          description="Open or clear the current account session without leaving the app."
          control={
            userSession ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="border-app-border bg-app-surface-muted text-app-foreground shadow-none hover:bg-app-surface-hover"
                onClick={onLogout}
              >
                Sign out
              </Button>
            ) : (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="border-app-border bg-app-surface-muted text-app-foreground shadow-none hover:bg-app-surface-hover"
                onClick={onLogin}
              >
                Sign in
              </Button>
            )
          }
        />
      </SettingsSection>

      <SettingsSection title="Desktop">
        <SettingsRow
          label="App version"
          description="The current local build of Tiy Agent."
          control={<SettingValue value={`v${appVersion}`} />}
        />
      </SettingsSection>
    </div>
  );
}

function GeneralSettingsPanel({
  description,
  isCheckingUpdates,
  language,
  runtime,
  selectedLanguageLabel,
  selectedThemeSummary,
  theme,
  updateStatus,
  onCheckUpdates,
  onSelectLanguage,
  onSelectTheme,
}: {
  description: string;
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  runtime: SystemMetadata | null;
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  theme: ThemePreference;
  updateStatus: string | null;
  onCheckUpdates: () => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="General" description={description} />

      <SettingsSection title="Preferences">
        <SettingsRow
          label="Theme"
          description="Use the system appearance or lock the app to light or dark mode."
          control={
            <ChoiceGroup
              options={THEME_OPTIONS}
              value={theme}
              onValueChange={(value) => onSelectTheme(value as ThemePreference)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Language"
          description="Choose which language the app interface should use."
          control={
            <ChoiceGroup
              options={LANGUAGE_OPTIONS}
              value={language}
              onValueChange={(value) => onSelectLanguage(value as LanguagePreference)}
            />
          }
        />
      </SettingsSection>

      <SettingsSection title="Runtime">
        <SettingsRow
          label="Current summary"
          description="Quick view of your active appearance and language settings."
          control={<SettingValue value={`${selectedThemeSummary} • ${selectedLanguageLabel}`} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Platform"
          description="The active runtime platform reported by the desktop bridge."
          control={<SettingValue value={runtime?.platform ?? "Unknown"} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Version"
          description="Current local application version."
          control={<SettingValue value={runtime?.version ?? "0.1.0"} />}
        />
        <SectionDivider />
        <SettingsRow
          label="Updates"
          description={updateStatus ?? "Check the current desktop build without leaving the active workspace."}
          control={
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="border-app-border bg-app-surface-muted text-app-foreground shadow-none hover:bg-app-surface-hover"
              onClick={onCheckUpdates}
            >
              <RefreshCw data-icon="inline-start" className={cn(isCheckingUpdates && "animate-spin")} />
              {isCheckingUpdates ? "Checking..." : "Check"}
            </Button>
          }
        />
      </SettingsSection>
    </div>
  );
}

function PromptSettingsPanel({
  description,
  prompts,
  onUpdatePromptSetting,
}: {
  description: string;
  prompts: PromptSettings;
  onUpdatePromptSetting: <Key extends keyof PromptSettings>(key: Key, value: PromptSettings[Key]) => void;
}) {
  const selectedStyle = RESPONSE_STYLE_OPTIONS.find((option) => option.value === prompts.responseStyle) ?? RESPONSE_STYLE_OPTIONS[0];

  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Prompts" description={description} />

      <SettingsSection title="Defaults">
        <SettingsRow
          label="Response style"
          description={selectedStyle.description}
          control={
            <ChoiceGroup
              options={RESPONSE_STYLE_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={prompts.responseStyle}
              onValueChange={(value) => onUpdatePromptSetting("responseStyle", value as PromptResponseStyle)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Project context"
          description="Include workspace context by default when a new thread starts."
          control={
            <Switch
              checked={prompts.includeProjectContext}
              size="sm"
              aria-label="Toggle project context inclusion"
              onCheckedChange={(checked) => onUpdatePromptSetting("includeProjectContext", checked)}
            />
          }
        />
      </SettingsSection>

      <TextAreaSection
        title="System prompt"
        description="Standing instruction applied before any project-specific context is injected."
        value={prompts.systemPrompt}
        minHeightClassName="min-h-36"
        onChange={(value) => onUpdatePromptSetting("systemPrompt", value)}
      />

      <TextAreaSection
        title="Project notes"
        description="Reusable notes about conventions, review posture, or collaboration habits."
        value={prompts.promptNotes}
        minHeightClassName="min-h-28"
        onChange={(value) => onUpdatePromptSetting("promptNotes", value)}
      />
    </div>
  );
}

function ApprovalSettingsPanel({
  approvalPolicy,
  description,
  onUpdateApprovalPolicySetting,
}: {
  approvalPolicy: ApprovalPolicySettings;
  description: string;
  onUpdateApprovalPolicySetting: <Key extends keyof ApprovalPolicySettings>(
    key: Key,
    value: ApprovalPolicySettings[Key],
  ) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Approval Policy" description={description} />

      <SettingsSection title="Execution">
        <SettingsRow
          label="Command execution"
          description={COMMAND_EXECUTION_OPTIONS.find((option) => option.value === approvalPolicy.commandExecution)?.description ?? ""}
          control={
            <ChoiceGroup
              options={COMMAND_EXECUTION_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={approvalPolicy.commandExecution}
              onValueChange={(value) => onUpdateApprovalPolicySetting("commandExecution", value as CommandExecutionPolicy)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="File writes outside workspace"
          description={ACCESS_POLICY_OPTIONS.find((option) => option.value === approvalPolicy.fileWriteOutsideWorkspace)?.description ?? ""}
          control={
            <ChoiceGroup
              options={ACCESS_POLICY_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={approvalPolicy.fileWriteOutsideWorkspace}
              onValueChange={(value) => onUpdateApprovalPolicySetting("fileWriteOutsideWorkspace", value as AccessPolicy)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Network access"
          description={ACCESS_POLICY_OPTIONS.find((option) => option.value === approvalPolicy.networkAccess)?.description ?? ""}
          control={
            <ChoiceGroup
              options={ACCESS_POLICY_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={approvalPolicy.networkAccess}
              onValueChange={(value) => onUpdateApprovalPolicySetting("networkAccess", value as AccessPolicy)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Risky commands"
          description={RISKY_COMMAND_OPTIONS.find((option) => option.value === approvalPolicy.riskyCommandConfirmation)?.description ?? ""}
          control={
            <ChoiceGroup
              options={RISKY_COMMAND_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={approvalPolicy.riskyCommandConfirmation}
              onValueChange={(value) =>
                onUpdateApprovalPolicySetting("riskyCommandConfirmation", value as RiskyCommandConfirmationPolicy)
              }
            />
          }
        />
      </SettingsSection>
    </div>
  );
}

const API_PROTOCOL_OPTIONS: ReadonlyArray<{ label: string; value: ApiProtocol }> = [
  { value: "chat-completions", label: "Chat Completions (/chat/completions)" },
  { value: "responses", label: "Responses (/responses)" },
  { value: "anthropic", label: "Anthropic (/messages)" },
  { value: "gemini", label: "Gemini (/generateContent)" },
  { value: "ollama", label: "Ollama (/api/chat)" },
];

function ProviderSettingsPanel({
  description,
  providers,
  onAddProvider,
  onRemoveProvider,
  onUpdateProvider,
}: {
  description: string;
  providers: Array<ProviderEntry>;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onRemoveProvider: (id: string) => void;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
}) {
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    () => providers[0]?.id ?? null,
  );
  const [providerSearch, setProviderSearch] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [modelSearch, setModelSearch] = useState("");
  const [newModelId, setNewModelId] = useState("");
  const [newModelDisplayName, setNewModelDisplayName] = useState("");

  const selectedProvider = providers.find((provider) => provider.id === selectedProviderId) ?? null;

  const filteredProviders = useMemo(() => {
    if (!providerSearch.trim()) return providers;
    const query = providerSearch.toLowerCase();
    return providers.filter((provider) => provider.name.toLowerCase().includes(query));
  }, [providers, providerSearch]);

  const filteredModels = useMemo(() => {
    if (!selectedProvider) return [];
    if (!modelSearch.trim()) return selectedProvider.models;
    const query = modelSearch.toLowerCase();
    return selectedProvider.models.filter(
      (model) =>
        model.modelId.toLowerCase().includes(query) ||
        model.displayName.toLowerCase().includes(query),
    );
  }, [selectedProvider, modelSearch]);

  const handleAddCustomProvider = () => {
    const newProvider: Omit<ProviderEntry, "id"> = {
      name: "Custom Provider",
      baseUrl: "https://api.example.com/v1",
      apiKey: "",
      apiProtocol: "chat-completions",
      enabled: false,
      isCustom: true,
      models: [],
    };
    onAddProvider(newProvider);
  };

  const handleToggleModel = (modelId: string, enabled: boolean) => {
    if (!selectedProvider) return;
    onUpdateProvider(selectedProvider.id, {
      models: selectedProvider.models.map((model) =>
        model.id === modelId ? { ...model, enabled } : model,
      ),
    });
  };

  const handleRemoveModel = (modelId: string) => {
    if (!selectedProvider) return;
    onUpdateProvider(selectedProvider.id, {
      models: selectedProvider.models.filter((model) => model.id !== modelId),
    });
  };

  const handleAddModel = () => {
    if (!selectedProvider || !newModelId.trim()) return;
    const newModel: ProviderModel = {
      id: crypto.randomUUID(),
      modelId: newModelId.trim(),
      displayName: newModelDisplayName.trim() || newModelId.trim(),
      enabled: true,
      isManual: true,
    };
    onUpdateProvider(selectedProvider.id, {
      models: [newModel, ...selectedProvider.models],
    });
    setNewModelId("");
    setNewModelDisplayName("");
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between">
        <PageHeading title="Providers" description={description} />
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
          onClick={handleAddCustomProvider}
        >
          <Plus className="size-3.5" />
          <span>Add Custom Provider</span>
        </button>
      </div>

      <div className="flex min-h-[520px] gap-4" style={{ height: "calc(100vh - 220px)" }}>
        {/* Provider sidebar list */}
        <div className="flex w-[220px] shrink-0 flex-col overflow-hidden rounded-2xl border border-app-border bg-app-surface">
          <div className="border-b border-app-border p-2">
            <div className="flex items-center gap-2 rounded-lg bg-app-surface-muted px-2.5 py-1.5">
              <Search className="size-3.5 shrink-0 text-app-subtle" />
              <input
                type="text"
                placeholder="Search providers..."
                value={providerSearch}
                onChange={(event) => setProviderSearch(event.target.value)}
                className="min-w-0 flex-1 bg-transparent text-[12px] text-app-foreground placeholder:text-app-subtle outline-none"
              />
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-1.5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <div className="space-y-0.5">
              {filteredProviders.map((provider) => {
                const isSelected = provider.id === selectedProviderId;
                return (
                  <button
                    key={provider.id}
                    type="button"
                    className={cn(
                      "flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors",
                      isSelected
                        ? "bg-app-surface-active text-app-foreground"
                        : "text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                    )}
                    onClick={() => {
                      setSelectedProviderId(provider.id);
                      setShowApiKey(false);
                      setModelSearch("");
                    }}
                  >
                    <ProviderIcon name={provider.name} className="size-5 shrink-0" />
                    <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{provider.name}</span>
                    {provider.isCustom ? (
                      <span className="shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide text-app-subtle">
                        custom
                      </span>
                    ) : null}
                    <div
                      className={cn(
                        "size-2 shrink-0 rounded-full",
                        provider.enabled ? "bg-app-success" : "bg-app-border",
                      )}
                    />
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        {/* Provider detail */}
        {selectedProvider ? (
          <div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-app-border bg-app-surface">
            <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
              {/* Header */}
              <div className="flex items-center justify-between border-b border-app-border px-5 py-4">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2.5">
                    <h3 className="text-[15px] font-semibold text-app-foreground">{selectedProvider.name}</h3>
                    {selectedProvider.isCustom ? (
                      <span className="rounded-md border border-app-border bg-app-surface-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-app-muted">
                        Custom
                      </span>
                    ) : null}
                    {selectedProvider.enabled ? (
                      <span className="rounded-md bg-app-success/15 px-1.5 py-0.5 text-[10px] font-medium text-app-success">
                        Active
                      </span>
                    ) : null}
                  </div>
                  <p className="mt-0.5 truncate text-[12px] text-app-subtle">{selectedProvider.baseUrl}</p>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {selectedProvider.isCustom ? (
                    <button
                      type="button"
                      title="Delete provider"
                      aria-label="Delete provider"
                      className="flex size-8 items-center justify-center rounded-lg border border-app-danger/30 text-app-danger transition-colors hover:bg-app-danger/10"
                      onClick={() => {
                        onRemoveProvider(selectedProvider.id);
                        setSelectedProviderId(providers.find((p) => p.id !== selectedProvider.id)?.id ?? null);
                      }}
                    >
                      <Trash2 className="size-3.5" />
                    </button>
                  ) : null}
                  <button
                    type="button"
                    title="Provider settings"
                    aria-label="Provider settings"
                    className="flex size-8 items-center justify-center rounded-lg border border-app-border text-app-muted transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                  >
                    <Settings2 className="size-3.5" />
                  </button>
                  <Switch
                    checked={selectedProvider.enabled}
                    aria-label="Toggle provider"
                    onCheckedChange={(checked) => onUpdateProvider(selectedProvider.id, { enabled: checked })}
                  />
                </div>
              </div>

              {/* Form fields */}
              <div className="space-y-5 px-5 py-4">
                <ProviderField label="Provider Name">
                  <Input
                    value={selectedProvider.name}
                    onChange={(event) => onUpdateProvider(selectedProvider.id, { name: event.target.value })}
                  />
                </ProviderField>

                <ProviderField label="Base URL">
                  <Input
                    value={selectedProvider.baseUrl}
                    onChange={(event) => onUpdateProvider(selectedProvider.id, { baseUrl: event.target.value })}
                  />
                </ProviderField>

                <ProviderField label="API Key">
                  <div className="relative">
                    <Input
                      type={showApiKey ? "text" : "password"}
                      value={selectedProvider.apiKey}
                      onChange={(event) => onUpdateProvider(selectedProvider.id, { apiKey: event.target.value })}
                      className="pr-10"
                    />
                    <button
                      type="button"
                      className="absolute right-2 top-1/2 flex size-6 -translate-y-1/2 items-center justify-center rounded-md text-app-subtle transition-colors hover:text-app-foreground"
                      onClick={() => setShowApiKey((current) => !current)}
                      aria-label={showApiKey ? "Hide API key" : "Show API key"}
                    >
                      {showApiKey ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
                    </button>
                  </div>
                </ProviderField>

                <ProviderField label="API Protocol">
                  <div className="relative">
                    <select
                      value={selectedProvider.apiProtocol}
                      onChange={(event) =>
                        onUpdateProvider(selectedProvider.id, { apiProtocol: event.target.value as ApiProtocol })
                      }
                      className="h-9 w-full appearance-none rounded-lg border border-app-border bg-app-surface-muted px-3 pr-8 text-[13px] text-app-foreground outline-none transition-colors focus-visible:border-app-border-strong"
                    >
                      {API_PROTOCOL_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-app-subtle" />
                  </div>
                  <p className="mt-1.5 text-[11px] text-app-subtle">
                    Choose the API protocol your provider uses
                  </p>
                </ProviderField>

                {/* Models section */}
                <div>
                  <div className="mb-3 flex items-center justify-between">
                    <h4 className="text-[13px] font-medium text-app-foreground">Models</h4>
                    <button
                      type="button"
                      className="inline-flex items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-2.5 py-1 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
                    >
                      <Download className="size-3" />
                      <span>Fetch</span>
                    </button>
                  </div>

                  {/* Add model row */}
                  <div className="mb-3 flex items-center gap-2">
                    <Input
                      placeholder="Model ID (e.g., gpt-4o)"
                      value={newModelId}
                      onChange={(event) => setNewModelId(event.target.value)}
                      className="min-w-0 flex-1"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") handleAddModel();
                      }}
                    />
                    <Input
                      placeholder="Display Name (optional)"
                      value={newModelDisplayName}
                      onChange={(event) => setNewModelDisplayName(event.target.value)}
                      className="min-w-0 flex-1"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") handleAddModel();
                      }}
                    />
                    <button
                      type="button"
                      className="inline-flex shrink-0 items-center gap-1 rounded-lg border border-app-border bg-app-surface px-2.5 py-2 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
                      onClick={handleAddModel}
                    >
                      <Plus className="size-3" />
                      <span>Add</span>
                    </button>
                  </div>
                  <p className="mb-3 text-[11px] text-app-subtle">
                    Add models manually or use Fetch to load from API
                  </p>

                  {/* Model search */}
                  <div className="mb-3 flex items-center gap-2 rounded-lg border border-app-border bg-app-surface-muted px-3 py-1.5">
                    <Search className="size-3.5 shrink-0 text-app-subtle" />
                    <input
                      type="text"
                      placeholder="Search models..."
                      value={modelSearch}
                      onChange={(event) => setModelSearch(event.target.value)}
                      className="min-w-0 flex-1 bg-transparent text-[12px] text-app-foreground placeholder:text-app-subtle outline-none"
                    />
                  </div>

                  <p className="mb-2 text-[11px] text-app-subtle">
                    Showing {filteredModels.length} model{filteredModels.length !== 1 ? "s" : ""} (enabled models shown first)
                  </p>

                  {/* Model list */}
                  <div className="space-y-1">
                    {[...filteredModels]
                      .sort((a, b) => (a.enabled === b.enabled ? 0 : a.enabled ? -1 : 1))
                      .map((model) => (
                        <div
                          key={model.id}
                          className="group flex items-center justify-between rounded-xl border border-app-border bg-app-surface-muted px-3.5 py-2.5 transition-colors"
                        >
                          <div className="flex min-w-0 flex-1 items-center gap-3">
                            <ModelIcon modelId={model.modelId} className="size-5 text-[16px]" />
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span className="font-mono text-[13px] font-medium text-app-foreground">
                                  {model.displayName || model.modelId}
                                </span>
                                {model.isManual ? (
                                  <span className="rounded px-1 py-0.5 text-[10px] font-semibold text-app-muted">
                                    Manual
                                  </span>
                                ) : null}
                              </div>
                              <div className="mt-0.5 flex items-center gap-2">
                                {model.contextWindow ? (
                                  <span className="text-[11px] text-app-subtle">{model.contextWindow}</span>
                                ) : null}
                                <span className="truncate font-mono text-[11px] text-app-subtle">
                                  {model.modelId}
                                </span>
                              </div>
                            </div>
                          </div>
                          <div className="flex shrink-0 items-center gap-2">
                            <button
                              type="button"
                              title="Settings"
                              aria-label="Model settings"
                              className="flex size-6 items-center justify-center rounded-md text-app-subtle opacity-0 transition-all hover:bg-app-surface-hover hover:text-app-foreground group-hover:opacity-100"
                            >
                              <Settings2 className="size-3" />
                            </button>
                            <button
                              type="button"
                              title="Remove model"
                              aria-label="Remove model"
                              className="flex size-6 items-center justify-center rounded-md text-app-subtle opacity-0 transition-all hover:bg-app-surface-hover hover:text-app-danger group-hover:opacity-100"
                              onClick={() => handleRemoveModel(model.id)}
                            >
                              <Trash2 className="size-3" />
                            </button>
                            <Switch
                              size="sm"
                              checked={model.enabled}
                              aria-label={`Toggle ${model.displayName || model.modelId}`}
                              onCheckedChange={(checked) => handleToggleModel(model.id, checked)}
                            />
                          </div>
                        </div>
                      ))}
                  </div>
                </div>
              </div>
            </div>
          </div>
        ) : (
          <div className="flex min-w-0 flex-1 items-center justify-center rounded-2xl border border-app-border bg-app-surface">
            <p className="text-[13px] text-app-subtle">Select a provider to configure</p>
          </div>
        )}
      </div>
    </div>
  );
}

function ProviderField({
  children,
  label,
}: {
  children: ReactNode;
  label: string;
}) {
  return (
    <div>
      <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">{label}</label>
      {children}
    </div>
  );
}

const PROVIDER_ICON_MAP: ReadonlyArray<{ match: (name: string) => boolean; icon: React.ComponentType<{ size?: number | string; className?: string }> }> = [
  { match: (n) => /\bopenai\b/i.test(n), icon: OpenAIIcon },
  { match: (n) => /\banthropic\b/i.test(n), icon: AnthropicIcon },
  { match: (n) => /\bgemini\b/i.test(n) || /\bgoogle\b/i.test(n), icon: GoogleIcon },
  { match: (n) => /\bdeepseek\b/i.test(n), icon: DeepSeekIcon },
  { match: (n) => /\bmoonshot\b/i.test(n), icon: MoonshotIcon },
  { match: (n) => /\bopenrouter\b/i.test(n), icon: OpenRouterIcon },
  { match: (n) => /\bzenmux\b/i.test(n), icon: ZenMuxIcon },
  { match: (n) => /\bstepfun\b/i.test(n), icon: StepfunIcon },
  { match: (n) => /\bmistral\b/i.test(n), icon: MistralIcon },
  { match: (n) => /\bqwen\b/i.test(n), icon: QwenIcon },
];

function ProviderIcon({ className, name }: { className?: string; name: string }) {
  const entry = PROVIDER_ICON_MAP.find((item) => item.match(name));
  if (entry) {
    const Icon = entry.icon;
    return <Icon className={cn("shrink-0 text-app-muted", className)} size="1em" />;
  }
  const initial = name.charAt(0).toUpperCase();
  return (
    <div
      className={cn(
        "flex items-center justify-center rounded-lg bg-app-surface-muted text-[11px] font-semibold text-app-muted",
        className,
      )}
    >
      {initial}
    </div>
  );
}

const MODEL_ICON_MAP: ReadonlyArray<{ match: (id: string) => boolean; icon: React.ComponentType<{ size?: number | string; className?: string }> }> = [
  { match: (id) => /\bclaude\b/i.test(id), icon: ClaudeIcon },
  { match: (id) => /\bgpt\b/i.test(id) || id.startsWith("openai/"), icon: OpenAIIcon },
  { match: (id) => /\bgemini\b/i.test(id) || id.startsWith("google/"), icon: GeminiIcon },
  { match: (id) => id.startsWith("deepseek/") || /\bdeepseek\b/i.test(id), icon: DeepSeekIcon },
  { match: (id) => id.startsWith("anthropic/"), icon: AnthropicIcon },
  { match: (id) => id.startsWith("stepfun/") || /\bstep-/i.test(id), icon: StepfunIcon },
  { match: (id) => id.startsWith("moonshot/") || /\bmoonshot\b/i.test(id), icon: MoonshotIcon },
  { match: (id) => /\bmistral\b/i.test(id), icon: MistralIcon },
  { match: (id) => /\bqwen\b/i.test(id), icon: QwenIcon },
  { match: (id) => /\bllama\b/i.test(id), icon: LlamaIcon },
];

function ModelIcon({ className, modelId }: { className?: string; modelId: string }) {
  const entry = MODEL_ICON_MAP.find((item) => item.match(modelId));
  if (entry) {
    const Icon = entry.icon;
    return <Icon className={cn("shrink-0 text-app-muted", className)} size="1em" />;
  }
  const initial = modelId.charAt(0).toUpperCase();
  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center rounded text-[10px] font-semibold text-app-muted",
        className,
      )}
    >
      {initial}
    </div>
  );
}

function WorkspaceSettingsPanel({
  description,
  workspaces,
  onAddWorkspace,
  onRemoveWorkspace,
  onSetDefaultWorkspace,
  onUpdateWorkspace,
}: {
  description: string;
  workspaces: Array<WorkspaceEntry>;
  onAddWorkspace: (entry: Omit<WorkspaceEntry, "id">) => void;
  onRemoveWorkspace: (id: string) => void;
  onSetDefaultWorkspace: (id: string) => void;
  onUpdateWorkspace: (id: string, patch: Partial<Omit<WorkspaceEntry, "id">>) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");

  const handleStartEdit = (workspace: WorkspaceEntry) => {
    setEditingId(workspace.id);
    setEditName(workspace.name);
  };

  const handleConfirmEdit = () => {
    if (editingId && editName.trim()) {
      onUpdateWorkspace(editingId, { name: editName.trim() });
    }
    setEditingId(null);
    setEditName("");
  };

  const handleAddWorkspace = () => {
    onAddWorkspace({
      name: "New Workspace",
      path: "/Users/jorben/Documents/Codespace/new-project",
      isDefault: false,
      isGit: false,
      autoWorkTree: false,
    });
  };

  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Workspace" description={description} />

      <SettingsSection
        title="Workspace"
        action={
          <button
            type="button"
            className="inline-flex items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
            onClick={handleAddWorkspace}
          >
            <FolderPlus className="size-3.5" />
            <span>Add workspace</span>
          </button>
        }
      >
        {workspaces.length === 0 ? (
          <div className="px-4 py-8 text-center">
            <p className="text-[13px] text-app-muted">No workspaces configured.</p>
            <p className="mt-1 text-[12px] text-app-subtle">Add a workspace to get started.</p>
          </div>
        ) : (
          <div className="divide-y divide-app-border">
            {workspaces.map((workspace) => (
              <div
                key={workspace.id}
                className="group flex items-center gap-3 px-4 py-3 transition-colors"
              >
                <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-app-surface-muted text-app-subtle">
                  <FolderOpen className="size-4" />
                </div>

                <div className="min-w-0 flex-1">
                  {editingId === workspace.id ? (
                    <input
                      type="text"
                      value={editName}
                      onChange={(event) => setEditName(event.target.value)}
                      onBlur={handleConfirmEdit}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") handleConfirmEdit();
                        if (event.key === "Escape") {
                          setEditingId(null);
                          setEditName("");
                        }
                      }}
                      autoFocus
                      className="h-7 w-full rounded-md border border-app-border-strong bg-app-surface-muted px-2 text-[13px] font-medium text-app-foreground outline-none"
                    />
                  ) : (
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium text-app-foreground">{workspace.name}</span>
                      {workspace.isDefault ? (
                        <span className="inline-flex items-center gap-1 rounded-md border border-app-border bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-muted">
                          <Star className="size-2.5 fill-current" />
                          Default
                        </span>
                      ) : null}
                      {workspace.isGit ? (
                        <span className="inline-flex items-center gap-1 rounded-md border border-app-border bg-app-surface-muted px-1.5 py-0.5 text-[11px] text-app-muted">
                          <GitBranch className="size-2.5" />
                          Git
                        </span>
                      ) : null}
                    </div>
                  )}
                  <p className="mt-0.5 truncate text-[12px] text-app-subtle" title={workspace.path}>
                    {workspace.path}
                  </p>
                </div>

                <div className="flex shrink-0 items-center gap-1">
                  <WorkspaceActionButton
                    icon={Star}
                    label="Set as default"
                    active={workspace.isDefault}
                    className="invisible group-hover:visible"
                    onClick={() => onSetDefaultWorkspace(workspace.id)}
                  />
                  <WorkspaceActionButton
                    icon={FolderOpen}
                    label="Open in finder"
                    onClick={() => {}}
                  />
                  <WorkspaceActionButton
                    icon={Pencil}
                    label="Rename"
                    onClick={() => handleStartEdit(workspace)}
                  />
                  <WorkspaceActionButton
                    icon={Trash2}
                    label="Remove workspace"
                    onClick={() => onRemoveWorkspace(workspace.id)}
                  />
                </div>
              </div>
            ))}
          </div>
        )}
      </SettingsSection>
    </div>
  );
}

function WorkspaceActionButton({
  active,
  className,
  icon: Icon,
  label,
  onClick,
}: {
  active?: boolean;
  className?: string;
  icon: typeof Star;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      className={cn(
        "flex size-7 items-center justify-center rounded-md transition-colors",
        active
          ? "text-app-foreground"
          : "text-app-subtle hover:bg-app-surface-hover hover:text-app-foreground",
        className,
      )}
      onClick={onClick}
    >
      <Icon className={cn("size-3.5", active && "fill-current")} />
    </button>
  );
}

function PageHeading({
  description,
  title,
}: {
  description: string;
  title: string;
}) {
  return (
    <div>
      <h1 className="text-[19px] font-semibold text-app-foreground">{title}</h1>
      <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
    </div>
  );
}

function SettingsSection({
  action,
  children,
  title,
}: {
  action?: ReactNode;
  children: ReactNode;
  title: string;
}) {
  return (
    <section>
      <div className="mb-2 flex items-center justify-between px-1">
        <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">{title}</h2>
        {action ?? null}
      </div>
      <div className="overflow-hidden rounded-2xl border border-app-border bg-app-surface">{children}</div>
    </section>
  );
}

function TextAreaSection({
  description,
  minHeightClassName,
  onChange,
  title,
  value,
}: {
  description: string;
  minHeightClassName: string;
  onChange: (value: string) => void;
  title: string;
  value: string;
}) {
  return (
    <SettingsSection title={title}>
      <div className="px-4 py-3">
        <p className="text-[12px] leading-5 text-app-muted">{description}</p>
        <Textarea
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className={cn(
            "mt-3",
            minHeightClassName,
          )}
        />
      </div>
    </SettingsSection>
  );
}

function SettingsRow({
  control,
  description,
  label,
}: {
  control: ReactNode;
  description: string;
  label: string;
}) {
  return (
    <div className="grid gap-3 bg-app-surface px-4 py-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-app-foreground">{label}</p>
        <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
      </div>
      <div className="min-w-0 md:justify-self-end">{control}</div>
    </div>
  );
}

function ChoiceGroup<TValue extends string>({
  onValueChange,
  options,
  value,
}: {
  onValueChange: (value: TValue) => void;
  options: ReadonlyArray<{ label: string; value: TValue }>;
  value: TValue;
}) {
  return (
    <WorkbenchSegmentedControl
      value={value}
      options={options}
      className="w-full md:w-auto"
      onValueChange={onValueChange}
    />
  );
}

function SettingValue({ value }: { value: string }) {
  return (
    <div className="inline-flex min-h-8 items-center rounded-lg border border-app-border bg-app-surface-muted px-3 text-[12px] text-app-foreground">
      {value}
    </div>
  );
}

function SectionDivider() {
  return <Separator />;
}
