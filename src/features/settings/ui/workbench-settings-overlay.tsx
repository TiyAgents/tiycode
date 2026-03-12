import type { ReactNode, RefObject } from "react";
import {
  ArrowLeft,
  CircleUserRound,
  Monitor,
  RefreshCw,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import type { SystemMetadata } from "@/shared/types/system";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Separator } from "@/shared/ui/separator";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import type {
  AccessPolicy,
  ApprovalPolicySettings,
  CommandExecutionPolicy,
  PromptResponseStyle,
  PromptSettings,
  RiskyCommandConfirmationPolicy,
  SettingsCategory,
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
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  systemMetadata: SystemMetadata | null;
  theme: ThemePreference;
  updateStatus: string | null;
  userSession: UserSession | null;
  onCheckUpdates: () => void;
  onClose: () => void;
  onLogin: () => void;
  onLogout: () => void;
  onSelectCategory: (category: SettingsCategory) => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onUpdateApprovalPolicySetting: <Key extends keyof ApprovalPolicySettings>(
    key: Key,
    value: ApprovalPolicySettings[Key],
  ) => void;
  onUpdatePromptSetting: <Key extends keyof PromptSettings>(key: Key, value: PromptSettings[Key]) => void;
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
  selectedLanguageLabel,
  selectedThemeSummary,
  systemMetadata,
  theme,
  updateStatus,
  userSession,
  onCheckUpdates,
  onClose,
  onLogin,
  onLogout,
  onSelectCategory,
  onSelectLanguage,
  onSelectTheme,
  onUpdateApprovalPolicySetting,
  onUpdatePromptSetting,
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
              className="border-app-border shadow-none data-[state=checked]:bg-app-surface-active data-[state=unchecked]:bg-app-surface-muted"
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
  children,
  title,
}: {
  children: ReactNode;
  title: string;
}) {
  return (
    <section>
      <h2 className="mb-2 px-1 text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">{title}</h2>
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
            "mt-3 rounded-lg border-app-border bg-app-surface-muted px-3 py-2 text-[13px] leading-6 text-app-foreground placeholder:text-app-subtle focus-visible:border-app-border-strong focus-visible:ring-0",
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
  return <Separator className="bg-app-border" />;
}
