import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  Brain,
  Check,
  ChevronDown,
  CircleUserRound,
  Copy,
  Database,
  Download,
  Eye,
  EyeOff,
  FolderOpen,
  FolderPlus,
  GitBranch,
  Image,
  Info,
  Monitor,
  MousePointerClick,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Star,
  Trash2,
  Wrench,
  X,
  Zap,
} from "lucide-react";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import type { LanguagePreference } from "@/app/providers/language-provider";
import type { ThemePreference } from "@/app/providers/theme-provider";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { matchProviderIcon } from "@/shared/lib/llm-brand-matcher";
import type { ProviderModelConnectionTestResultDto } from "@/shared/types/api";
import type { SystemMetadata } from "@/shared/types/system";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { LocalLlmIcon } from "@/shared/ui/local-llm-icon";
import { ModelBrandIcon } from "@/shared/ui/model-brand-icon";
import { Separator } from "@/shared/ui/separator";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";
import { WorkbenchSegmentedControl } from "@/shared/ui/workbench-segmented-control";
import type {
  AgentProfile,
  ApprovalPolicy,
  CommandEntry,
  CommandSettings,
  CustomProviderType,
  GeneralPreferences,
  NetworkAccessPolicy,
  PatternEntry,
  PolicySettings,
  PromptResponseStyle,
  ProviderCatalogEntry,
  ProviderEntry,
  ProviderModel,
  ProviderModelCapabilities,
  SandboxPolicy,
  SettingsCategory,
  WorkspaceEntry,
  WritableRootEntry,
} from "@/modules/settings-center/model/use-settings-controller";

type UserSession = {
  name: string;
  avatar: string;
  email: string;
};

type SettingsCenterOverlayProps = {
  activeCategory: SettingsCategory;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
  contentRef: RefObject<HTMLDivElement | null>;
  generalPreferences: GeneralPreferences;
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  commands: CommandSettings;
  policy: PolicySettings;
  providerCatalog: Array<ProviderCatalogEntry>;
  providers: Array<ProviderEntry>;
  systemMetadata: SystemMetadata | null;
  theme: ThemePreference;
  updateStatus: string | null;
  userSession: UserSession | null;
  workspaces: Array<WorkspaceEntry>;
  onAddAgentProfile: (entry: Omit<AgentProfile, "id">) => void;
  onAddAllowEntry: (entry: Omit<PatternEntry, "id">) => void;
  onAddCommand: (entry: Omit<CommandEntry, "id">) => void;
  onAddDenyEntry: (entry: Omit<PatternEntry, "id">) => void;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onAddWorkspace: (entry: Omit<WorkspaceEntry, "id">) => void;
  onAddWritableRoot: (entry: Omit<WritableRootEntry, "id">) => void;
  onCheckUpdates: () => void;
  onClose: () => void;
  onDuplicateAgentProfile: (id: string) => void;
  onLogin: () => void;
  onLogout: () => void;
  onRemoveAgentProfile: (id: string) => void;
  onRemoveAllowEntry: (id: string) => void;
  onRemoveCommand: (id: string) => void;
  onRemoveDenyEntry: (id: string) => void;
  onRemoveProvider: (id: string) => void;
  onRemoveWorkspace: (id: string) => void;
  onRemoveWritableRoot: (id: string) => void;
  onSelectCategory: (category: SettingsCategory) => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onSetActiveAgentProfile: (id: string) => void;
  onSetDefaultWorkspace: (id: string) => void;
  onUpdateAgentProfile: (id: string, patch: Partial<Omit<AgentProfile, "id">>) => void;
  onUpdateAllowEntry: (id: string, patch: Partial<Omit<PatternEntry, "id">>) => void;
  onUpdateCommand: (id: string, patch: Partial<Omit<CommandEntry, "id">>) => void;
  onUpdateDenyEntry: (id: string, patch: Partial<Omit<PatternEntry, "id">>) => void;
  onUpdateGeneralPreference: <Key extends keyof GeneralPreferences>(key: Key, value: GeneralPreferences[Key]) => void;
  onUpdatePolicySetting: <Key extends keyof PolicySettings>(key: Key, value: PolicySettings[Key]) => void;
  onFetchProviderModels: (id: string) => Promise<void>;
  onTestProviderModelConnection: (
    providerId: string,
    modelId: string,
  ) => Promise<ProviderModelConnectionTestResultDto>;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
  onUpdateWritableRoot: (id: string, patch: Partial<Omit<WritableRootEntry, "id">>) => void;
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
    description: "Manage your local session, account identity, active plan, and available subscription tiers.",
    icon: CircleUserRound,
  },
  {
    key: "general",
    title: "General",
    description: "Set app preferences and default agent behavior, including theme, language, startup, models, and instructions.",
    icon: Monitor,
  },
  {
    key: "providers",
    title: "Providers",
    description: "Configure model providers, API access, request settings, and the models available to your agent.",
    icon: Server,
  },
  {
    key: "commands",
    title: "Commands",
    description: "Create reusable prompt shortcuts that appear when typing / in chat.",
    icon: Zap,
  },
  {
    key: "policy",
    title: "Permissions",
    description: "Control approval mode, allow and deny rules, sandbox access, network access, and writable paths.",
    icon: ShieldCheck,
  },
  {
    key: "workspace",
    title: "Workspace",
    description: "Manage project directories and choose which workspace new conversations should start in.",
    icon: FolderOpen,
  },
  {
    key: "about",
    title: "About",
    description: "View product details, jump to key project links, and check whether this desktop build is up to date.",
    icon: Info,
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
  { value: "concise", label: "Concise", description: "Very short, answer-first replies with minimal explanation." },
  { value: "balanced", label: "Balanced", description: "Compact by default, with extra detail only when it helps." },
  { value: "guide", label: "Guided", description: "Explanatory replies with reasoning, tradeoffs, and next steps." },
] as const;

const APPROVAL_POLICY_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: ApprovalPolicy;
}> = [
  { value: "untrusted", label: "Untrusted", description: "Approve every tool call and command individually." },
  { value: "on-request", label: "On request", description: "Auto-approve safe actions, ask for risky ones." },
  { value: "never", label: "Never", description: "Let the agent run without approval prompts." },
] as const;

const SANDBOX_POLICY_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: SandboxPolicy;
}> = [
  { value: "read-only", label: "Read only", description: "No file writes allowed anywhere." },
  { value: "workspace-write", label: "Workspace write", description: "Write only inside the active workspace." },
  { value: "full-access", label: "Full access", description: "Write anywhere on the filesystem." },
] as const;

const NETWORK_ACCESS_OPTIONS: ReadonlyArray<{
  description: string;
  label: string;
  value: NetworkAccessPolicy;
}> = [
  { value: "ask", label: "Ask", description: "Prompt before making network requests." },
  { value: "block", label: "Block", description: "Block all outbound network access." },
  { value: "allow", label: "Allow", description: "Allow network access without prompts." },
] as const;

const MINIMAX_BASE_URL_OPTIONS = [
  "https://api.minimax.io/anthropic",
  "https://api.minimaxi.com/anthropic",
] as const;

function deriveWorkspaceNameFromPath(path: string) {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : "New Workspace";
}

export function SettingsCenterOverlay({
  activeCategory,
  agentProfiles,
  activeAgentProfileId,
  contentRef,
  generalPreferences,
  isCheckingUpdates,
  language,
  commands,
  policy,
  providerCatalog,
  providers,
  systemMetadata,
  theme,
  updateStatus,
  userSession,
  workspaces,
  onAddAgentProfile,
  onAddAllowEntry,
  onAddCommand,
  onAddDenyEntry,
  onAddProvider,
  onAddWorkspace,
  onAddWritableRoot,
  onCheckUpdates,
  onClose,
  onDuplicateAgentProfile,
  onLogin,
  onLogout,
  onRemoveAgentProfile,
  onRemoveAllowEntry,
  onRemoveCommand,
  onRemoveDenyEntry,
  onRemoveProvider,
  onRemoveWorkspace,
  onRemoveWritableRoot,
  onSelectCategory,
  onSelectLanguage,
  onSelectTheme,
  onSetActiveAgentProfile,
  onSetDefaultWorkspace,
  onUpdateAgentProfile,
  onUpdateAllowEntry,
  onUpdateCommand,
  onUpdateDenyEntry,
  onUpdateGeneralPreference,
  onUpdatePolicySetting,
  onFetchProviderModels,
  onTestProviderModelConnection,
  onUpdateProvider,
  onUpdateWritableRoot,
}: SettingsCenterOverlayProps) {
  const activeMeta = CATEGORY_META.find((category) => category.key === activeCategory) ?? CATEGORY_META[1];
  const isAboutCategory = activeCategory === "about";

  return (
    <div className="fixed inset-x-0 bottom-0 top-9 z-[60] bg-app-canvas text-app-foreground">
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
              <div
                className={cn(
                  "mx-auto px-6",
                  isAboutCategory
                    ? "flex min-h-full w-full max-w-4xl items-center justify-center py-8"
                    : "flex max-w-4xl flex-col gap-6 pb-28 pt-6",
                )}
              >
                {activeCategory === "account" ? (
                  <AccountSettingsPanel
                    description={activeMeta.description}
                    userSession={userSession}
                    onLogin={onLogin}
                    onLogout={onLogout}
                  />
                ) : null}

                {activeCategory === "general" ? (
                  <GeneralSettingsPanel
                    agentProfiles={agentProfiles}
                    activeAgentProfileId={activeAgentProfileId}
                    description={activeMeta.description}
                    generalPreferences={generalPreferences}
                    language={language}
                    providers={providers}
                    theme={theme}
                    onAddAgentProfile={onAddAgentProfile}
                    onDuplicateAgentProfile={onDuplicateAgentProfile}
                    onRemoveAgentProfile={onRemoveAgentProfile}
                    onSelectLanguage={onSelectLanguage}
                    onSelectTheme={onSelectTheme}
                    onSetActiveAgentProfile={onSetActiveAgentProfile}
                    onUpdateAgentProfile={onUpdateAgentProfile}
                    onUpdateGeneralPreference={onUpdateGeneralPreference}
                  />
                ) : null}

                {activeCategory === "workspace" ? (
                  <WorkspaceSettingsPanel
                    description={activeMeta.description}
                    systemMetadata={systemMetadata}
                    workspaces={workspaces}
                    onAddWorkspace={onAddWorkspace}
                    onRemoveWorkspace={onRemoveWorkspace}
                    onSetDefaultWorkspace={onSetDefaultWorkspace}
                  />
                ) : null}

                {activeCategory === "providers" ? (
                  <ProviderSettingsPanel
                    description={activeMeta.description}
                    providers={providers}
                    providerCatalog={providerCatalog}
                    onAddProvider={onAddProvider}
                    onFetchProviderModels={onFetchProviderModels}
                    onTestProviderModelConnection={onTestProviderModelConnection}
                    onRemoveProvider={onRemoveProvider}
                    onUpdateProvider={onUpdateProvider}
                  />
                ) : null}

                {activeCategory === "commands" ? (
                  <CommandSettingsPanel
                    description={activeMeta.description}
                    commands={commands}
                    onAddCommand={onAddCommand}
                    onRemoveCommand={onRemoveCommand}
                    onUpdateCommand={onUpdateCommand}
                  />
                ) : null}

                {activeCategory === "policy" ? (
                  <PolicySettingsPanel
                    description={activeMeta.description}
                    policy={policy}
                    onAddAllowEntry={onAddAllowEntry}
                    onAddDenyEntry={onAddDenyEntry}
                    onAddWritableRoot={onAddWritableRoot}
                    onRemoveAllowEntry={onRemoveAllowEntry}
                    onRemoveDenyEntry={onRemoveDenyEntry}
                    onRemoveWritableRoot={onRemoveWritableRoot}
                    onUpdateAllowEntry={onUpdateAllowEntry}
                    onUpdateDenyEntry={onUpdateDenyEntry}
                    onUpdatePolicySetting={onUpdatePolicySetting}
                    onUpdateWritableRoot={onUpdateWritableRoot}
                  />
                ) : null}

                {activeCategory === "about" ? (
                  <AboutSettingsPanel
                    isCheckingUpdates={isCheckingUpdates}
                    runtime={systemMetadata}
                    updateStatus={updateStatus}
                    onCheckUpdates={onCheckUpdates}
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

type PlanName = "Free" | "Lite" | "Pro" | "Max";

type PlanDefinition = {
  description: string;
  monthlyCredits: number;
  name: PlanName;
  priceLabel: string;
  summary: string;
};

type CurrentPlanSnapshot = {
  creditsTotal: number;
  creditsUsed: number;
  name: PlanName;
  nextResetAt: string;
};

type BillingHistoryEntry = {
  amount: string;
  id: string;
  invoice: string;
  paymentMethod: string;
  time: string;
};

const PLAN_DEFINITIONS: ReadonlyArray<PlanDefinition> = [
  {
    name: "Free",
    priceLabel: "",
    summary: "A one-time trial with 1,000 credits for your first 31 days after signup.",
    description: "Best for evaluating the core experience, testing a few real workflows, and deciding whether you need an ongoing plan.",
    monthlyCredits: 1_000,
  },
  {
    name: "Lite",
    priceLabel: "$9.9/mo",
    summary: "For occasional personal use, quick chats, and lightweight agent workflows.",
    description: "Includes 2,000 credits each month for individuals who use the product regularly but do not need heavy daily volume.",
    monthlyCredits: 2_000,
  },
  {
    name: "Pro",
    priceLabel: "$19.9/mo",
    summary: "For consistent day-to-day work across coding, research, and repeated agent runs.",
    description: "Includes 5,000 monthly credits and is the best fit for active individual users who want reliable room for deeper sessions.",
    monthlyCredits: 5_000,
  },
  {
    name: "Max",
    priceLabel: "$199.9/mo",
    summary: "For intensive usage, long sessions, and high-volume multi-step workflows.",
    description: "Includes 80,000 monthly credits for advanced users who need substantial capacity for sustained, heavy workloads.",
    monthlyCredits: 80_000,
  },
] as const;

const CURRENT_PLAN_SNAPSHOT: CurrentPlanSnapshot = {
  name: "Pro",
  creditsUsed: 1_840,
  creditsTotal: 5_000,
  nextResetAt: "2026-04-01 00:00",
};

const BILLING_HISTORY_SNAPSHOT: ReadonlyArray<BillingHistoryEntry> = [
  {
    id: "billing-2026-03",
    time: "Mar 01, 2026",
    amount: "$19.90",
    paymentMethod: "Visa •••• 2048",
    invoice: "INV-2026-0301",
  },
  {
    id: "billing-2026-02",
    time: "Feb 01, 2026",
    amount: "$19.90",
    paymentMethod: "Visa •••• 2048",
    invoice: "INV-2026-0201",
  },
  {
    id: "billing-2026-01",
    time: "Jan 01, 2026",
    amount: "$19.90",
    paymentMethod: "Visa •••• 2048",
    invoice: "INV-2026-0101",
  },
] as const;

const PLAN_VISUAL_STYLES: Partial<
  Record<
    PlanName,
    {
      activeClass: string;
      cardClass: string;
      chipClass: string;
      currentBadgeClass: string;
    }
  >
> = {
  Lite: {
    cardClass:
      "border-emerald-400/25 bg-[radial-gradient(circle_at_top_left,rgba(52,211,153,0.16),transparent_42%),linear-gradient(180deg,rgba(255,255,255,0.82),rgba(255,255,255,0.52))] shadow-[0_18px_40px_-32px_rgba(16,185,129,0.55)] dark:border-emerald-400/18 dark:bg-[radial-gradient(circle_at_top_left,rgba(52,211,153,0.2),transparent_38%),linear-gradient(180deg,rgba(20,30,28,0.88),rgba(20,30,28,0.56))] dark:shadow-[0_18px_42px_-30px_rgba(16,185,129,0.38)]",
    activeClass:
      "border-emerald-500/35 shadow-[0_22px_48px_-30px_rgba(16,185,129,0.62)] dark:border-emerald-300/28 dark:shadow-[0_20px_46px_-28px_rgba(16,185,129,0.44)]",
    chipClass:
      "border-emerald-400/24 bg-white/72 text-emerald-700 dark:border-emerald-400/18 dark:bg-emerald-400/10 dark:text-emerald-100",
    currentBadgeClass:
      "border-emerald-500/18 bg-white/70 text-emerald-700 dark:border-emerald-400/18 dark:bg-emerald-400/10 dark:text-emerald-100",
  },
  Pro: {
    cardClass:
      "border-sky-400/28 bg-[radial-gradient(circle_at_top_right,rgba(56,189,248,0.16),transparent_42%),linear-gradient(180deg,rgba(255,255,255,0.84),rgba(255,255,255,0.54))] shadow-[0_18px_40px_-32px_rgba(59,130,246,0.5)] dark:border-sky-400/18 dark:bg-[radial-gradient(circle_at_top_right,rgba(56,189,248,0.2),transparent_38%),linear-gradient(180deg,rgba(18,24,34,0.88),rgba(18,24,34,0.56))] dark:shadow-[0_18px_42px_-30px_rgba(56,189,248,0.34)]",
    activeClass:
      "border-sky-500/38 shadow-[0_22px_48px_-30px_rgba(59,130,246,0.58)] dark:border-sky-300/28 dark:shadow-[0_20px_46px_-28px_rgba(56,189,248,0.42)]",
    chipClass:
      "border-sky-400/24 bg-white/72 text-sky-700 dark:border-sky-400/18 dark:bg-sky-400/10 dark:text-sky-100",
    currentBadgeClass:
      "border-sky-500/18 bg-white/70 text-sky-700 dark:border-sky-400/18 dark:bg-sky-400/10 dark:text-sky-100",
  },
  Max: {
    cardClass:
      "border-lime-400/25 bg-[radial-gradient(circle_at_top_center,rgba(163,230,53,0.16),transparent_42%),linear-gradient(180deg,rgba(255,255,255,0.84),rgba(255,255,255,0.54))] shadow-[0_18px_40px_-32px_rgba(132,204,22,0.45)] dark:border-lime-400/18 dark:bg-[radial-gradient(circle_at_top_center,rgba(163,230,53,0.22),transparent_38%),linear-gradient(180deg,rgba(30,32,20,0.88),rgba(30,32,20,0.56))] dark:shadow-[0_18px_42px_-30px_rgba(163,230,53,0.34)]",
    activeClass:
      "border-lime-500/34 shadow-[0_22px_48px_-30px_rgba(132,204,22,0.52)] dark:border-lime-300/24 dark:shadow-[0_20px_46px_-28px_rgba(163,230,53,0.4)]",
    chipClass:
      "border-lime-400/24 bg-white/72 text-lime-700 dark:border-lime-400/18 dark:bg-lime-400/10 dark:text-lime-100",
    currentBadgeClass:
      "border-lime-500/18 bg-white/70 text-lime-700 dark:border-lime-400/18 dark:bg-lime-400/10 dark:text-lime-100",
  },
};

function formatCreditCount(count: number): string {
  if (count >= 1_000_000) {
    return `${(count / 1_000_000).toFixed(1)}M`;
  }

  if (count >= 1_000) {
    return `${(count / 1_000).toFixed(count % 1_000 === 0 ? 0 : 1)}K`;
  }

  return count.toString();
}

function SessionIdentityCard({
  userSession,
  onLogin,
  onLogout,
}: {
  userSession: UserSession | null;
  onLogin: () => void;
  onLogout: () => void;
}) {
  return (
    <div className="flex flex-col gap-4 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex size-12 shrink-0 items-center justify-center rounded-full border border-app-border bg-app-surface-muted text-sm font-semibold text-app-foreground">
          {userSession ? userSession.avatar : <CircleUserRound className="size-5 text-app-subtle" />}
        </div>
        <div className="min-w-0">
          <p className="truncate text-[14px] font-semibold text-app-foreground">
            {userSession?.name ?? "Guest"}
          </p>
          <p className="mt-1 truncate text-[12px] text-app-muted">
            {userSession?.email ?? "Sign in to sync your account identity and plan usage."}
          </p>
        </div>
      </div>

      <Button
        type="button"
        size="sm"
        variant="outline"
        className="border-app-border bg-app-surface-muted text-app-foreground shadow-none hover:bg-app-surface-hover"
        onClick={userSession ? onLogout : onLogin}
      >
        {userSession ? "Sign out" : "Sign in"}
      </Button>
    </div>
  );
}

function CurrentPlanCard({ plan }: { plan: CurrentPlanSnapshot }) {
  const usageRatio = Math.min(plan.creditsUsed / plan.creditsTotal, 1);
  const remainingCredits = Math.max(plan.creditsTotal - plan.creditsUsed, 0);
  const planStyle = PLAN_VISUAL_STYLES[plan.name];

  return (
    <div
      className={cn(
        "rounded-2xl border px-4 py-4",
        planStyle?.cardClass ?? "border-app-border bg-app-surface-muted",
        planStyle?.activeClass,
      )}
    >
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <div
            className={cn(
              "inline-flex min-h-8 items-center rounded-full border bg-app-surface px-3 text-[12px] font-medium text-app-foreground",
              planStyle?.chipClass ?? "border-app-border",
            )}
          >
            {plan.name}
          </div>
        </div>

        <div className="sm:text-right">
          <p className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Next Reset</p>
          <p className="mt-2 text-[13px] font-medium text-app-foreground">{plan.nextResetAt}</p>
        </div>
      </div>

      <div className="mt-5">
        <div className="flex items-center justify-between gap-3 text-[12px]">
          <span className="font-medium text-app-foreground">Credits</span>
          <span className="text-app-muted">
            {formatCreditCount(plan.creditsUsed)} / {formatCreditCount(plan.creditsTotal)}
          </span>
        </div>
        <div className="mt-2 h-2 overflow-hidden rounded-full bg-app-surface">
          <div
            className="h-full rounded-full bg-app-foreground/70 transition-[width] duration-300 dark:bg-white/70"
            style={{ width: `${usageRatio * 100}%` }}
          />
        </div>
        <p className="mt-2 text-[12px] text-app-muted">
          {formatCreditCount(remainingCredits)} credits remaining in the current cycle.
        </p>
      </div>
    </div>
  );
}

function PlanCatalogCard({
  isActive,
  plan,
}: {
  isActive: boolean;
  plan: PlanDefinition;
}) {
  const planStyle = PLAN_VISUAL_STYLES[plan.name];

  return (
    <article
      className={cn(
        "rounded-2xl border px-4 py-4 transition-colors",
        planStyle?.cardClass ?? "border-app-border bg-app-surface",
        isActive
          ? planStyle?.activeClass ?? "border-app-foreground/20 bg-app-surface-muted"
          : null,
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-[15px] font-semibold text-app-foreground">{plan.name}</h3>
            {isActive ? (
              <span
                className={cn(
                  "inline-flex items-center rounded-full border bg-app-surface px-2 py-0.5 text-[10px] font-medium uppercase tracking-[0.1em] text-app-foreground",
                  planStyle?.currentBadgeClass ?? "border-app-foreground/15",
                )}
              >
                Current
              </span>
            ) : null}
          </div>
          <div className="mt-2 flex min-h-[20px] items-center">
            {plan.priceLabel ? (
              <p className="text-[20px] font-semibold leading-none text-app-foreground">{plan.priceLabel}</p>
            ) : null}
          </div>
          <p className="mt-2 text-[12px] leading-5 text-app-muted">{plan.summary}</p>
        </div>

        <div
          className={cn(
            "inline-flex min-h-8 shrink-0 items-center gap-1.5 rounded-full border bg-app-surface px-3 text-[12px] font-medium text-app-foreground",
            planStyle?.chipClass ?? "border-app-border",
          )}
        >
          <span>{formatCreditCount(plan.monthlyCredits)} credits</span>
          {plan.name === "Free" ? (
            <span className="inline-flex items-center rounded-full bg-app-surface-muted px-1.5 py-0.5 text-[10px] font-medium leading-none text-app-subtle">
              Trial
            </span>
          ) : null}
        </div>
      </div>

      <p className="mt-4 text-[12px] leading-5 text-app-muted">{plan.description}</p>
    </article>
  );
}

function BillingHistoryTable({
  entries,
}: {
  entries: ReadonlyArray<BillingHistoryEntry>;
}) {
  return (
    <div className="overflow-hidden rounded-2xl border border-app-border bg-app-surface">
      <div className="overflow-x-auto">
        <table className="min-w-full border-collapse text-left">
          <thead className="bg-app-surface-muted/80">
            <tr>
              <th className="px-4 py-3 text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Time</th>
              <th className="px-4 py-3 text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Amount</th>
              <th className="px-4 py-3 text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Payment Method</th>
              <th className="px-4 py-3 text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Invoice</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.id} className="border-t border-app-border">
                <td className="whitespace-nowrap px-4 py-3 text-[13px] text-app-foreground">{entry.time}</td>
                <td className="whitespace-nowrap px-4 py-3 text-[13px] font-medium text-app-foreground">{entry.amount}</td>
                <td className="whitespace-nowrap px-4 py-3 text-[13px] text-app-muted">{entry.paymentMethod}</td>
                <td className="whitespace-nowrap px-4 py-3">
                  <span className="inline-flex items-center rounded-md border border-app-border bg-app-surface-muted px-2 py-1 font-mono text-[11px] text-app-foreground">
                    {entry.invoice}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function AccountSettingsPanel({
  description,
  userSession,
  onLogin,
  onLogout,
}: {
  description: string;
  userSession: UserSession | null;
  onLogin: () => void;
  onLogout: () => void;
}) {
  const currentPlan = userSession ? CURRENT_PLAN_SNAPSHOT : null;

  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Account" description={description} />

      <SettingsSection title="Session">
        <SessionIdentityCard userSession={userSession} onLogin={onLogin} onLogout={onLogout} />
      </SettingsSection>

      <section>
        <div className="mb-2 flex items-center justify-between px-1">
          <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Current Plan</h2>
        </div>
        {currentPlan ? (
          <CurrentPlanCard plan={currentPlan} />
        ) : (
          <div className="rounded-2xl border border-dashed border-app-border bg-app-surface-muted px-4 py-4">
            <p className="text-[13px] font-medium text-app-foreground">Guest access</p>
            <p className="mt-1 text-[12px] leading-5 text-app-muted">
              Sign in to see your current plan, credit usage, and the exact reset time for the next billing cycle.
            </p>
          </div>
        )}
      </section>

      <section>
        <div className="mb-2 flex items-center justify-between px-1">
          <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Plan Comparison</h2>
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          {PLAN_DEFINITIONS.map((plan) => (
            <PlanCatalogCard
              key={plan.name}
              plan={plan}
              isActive={currentPlan?.name === plan.name}
            />
          ))}
        </div>
      </section>

      <section>
        <div className="mb-2 flex items-center justify-between px-1">
          <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Billing History</h2>
        </div>
        {userSession ? (
          <BillingHistoryTable entries={BILLING_HISTORY_SNAPSHOT} />
        ) : (
          <div className="rounded-2xl border border-dashed border-app-border bg-app-surface-muted px-4 py-4">
            <p className="text-[13px] font-medium text-app-foreground">Billing records are available after sign-in</p>
            <p className="mt-1 text-[12px] leading-5 text-app-muted">
              Sign in to review charge history, payment methods, and invoice references for your account.
            </p>
          </div>
        )}
      </section>
    </div>
  );
}

function ProfilePicker({
  profiles,
  activeProfileId,
  onSelect,
  onRename,
  onDuplicate,
  onDelete,
}: {
  profiles: Array<AgentProfile>;
  activeProfileId: string;
  onSelect: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onDuplicate: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const [dropdownStyle, setDropdownStyle] = useState<React.CSSProperties>({});

  const activeProfile = profiles.find((p) => p.id === activeProfileId) ?? profiles[0];

  useLayoutEffect(() => {
    if (!isOpen || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const dropdownMaxH = 300;
    const spaceBelow = window.innerHeight - rect.bottom - 8;
    const placeAbove = spaceBelow < dropdownMaxH && rect.top > spaceBelow;
    setDropdownStyle({
      position: "fixed",
      right: window.innerWidth - rect.right,
      minWidth: Math.max(rect.width, 240),
      maxHeight: dropdownMaxH,
      ...(placeAbove
        ? { bottom: window.innerHeight - rect.top + 4 }
        : { top: rect.bottom + 4 }),
    });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || dropdownRef.current?.contains(target)) return;
      setIsOpen(false);
      setEditingId(null);
    };
    document.addEventListener("pointerdown", handleClickOutside);
    return () => document.removeEventListener("pointerdown", handleClickOutside);
  }, [isOpen]);

  const handleStartRename = (profile: AgentProfile) => {
    setEditingId(profile.id);
    setEditingName(profile.name);
  };

  const handleCommitRename = () => {
    if (editingId && editingName.trim()) {
      onRename(editingId, editingName.trim());
    }
    setEditingId(null);
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className="flex h-6 items-center gap-1 rounded-md border border-app-border bg-app-surface px-2 text-[11px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
        onClick={() => setIsOpen((prev) => !prev)}
      >
        <span className="shrink-0 text-app-subtle">Agent Profile:</span>
        <span className="max-w-[120px] truncate">{activeProfile.name}</span>
        <ChevronDown className="size-3 text-app-subtle" />
      </button>
      {isOpen
        ? createPortal(
            <div
              ref={dropdownRef}
              style={dropdownStyle}
              className="z-[100] overflow-y-auto rounded-xl border border-app-border bg-app-surface p-1 shadow-lg"
            >
              {profiles.map((profile) => (
                <div
                  key={profile.id}
                  className={cn(
                    "group flex items-center gap-1.5 rounded-lg px-2 py-1.5 transition-colors",
                    profile.id === activeProfileId ? "bg-app-surface-hover" : "hover:bg-app-surface-hover",
                  )}
                >
                  {editingId === profile.id ? (
                    <input
                      autoFocus
                      className="min-w-0 flex-1 rounded bg-transparent px-0.5 text-[13px] leading-5 text-app-foreground outline-none ring-1 ring-app-border focus:ring-app-accent"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onBlur={handleCommitRename}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleCommitRename();
                        if (e.key === "Escape") setEditingId(null);
                      }}
                    />
                  ) : (
                    <button
                      type="button"
                      className="min-w-0 flex-1 truncate text-left text-[13px] leading-5 text-app-foreground"
                      onClick={() => {
                        onSelect(profile.id);
                        setIsOpen(false);
                      }}
                    >
                      {profile.name}
                    </button>
                  )}
                  {editingId !== profile.id ? (
                    <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                      <button
                        type="button"
                        className="flex size-5 items-center justify-center rounded text-app-subtle hover:bg-app-canvas hover:text-app-foreground"
                        title="Rename"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleStartRename(profile);
                        }}
                      >
                        <Pencil className="size-3" />
                      </button>
                      <button
                        type="button"
                        className="flex size-5 items-center justify-center rounded text-app-subtle hover:bg-app-canvas hover:text-app-foreground"
                        title="Duplicate"
                        onClick={(e) => {
                          e.stopPropagation();
                          onDuplicate(profile.id);
                          setIsOpen(false);
                        }}
                      >
                        <Copy className="size-3" />
                      </button>
                      <button
                        type="button"
                        className={cn(
                          "flex size-5 items-center justify-center rounded",
                          profiles.length <= 1
                            ? "cursor-not-allowed text-app-subtle/40"
                            : "text-app-subtle hover:bg-app-canvas hover:text-red-500",
                        )}
                        title="Delete"
                        disabled={profiles.length <= 1}
                        onClick={(e) => {
                          e.stopPropagation();
                          if (profiles.length > 1) {
                            onDelete(profile.id);
                          }
                        }}
                      >
                        <Trash2 className="size-3" />
                      </button>
                    </div>
                  ) : null}
                </div>
              ))}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

function GeneralSettingsPanel({
  agentProfiles,
  activeAgentProfileId,
  description,
  generalPreferences,
  language,
  providers,
  theme,
  onAddAgentProfile,
  onDuplicateAgentProfile,
  onRemoveAgentProfile,
  onSelectLanguage,
  onSelectTheme,
  onSetActiveAgentProfile,
  onUpdateAgentProfile,
  onUpdateGeneralPreference,
}: {
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
  description: string;
  generalPreferences: GeneralPreferences;
  language: LanguagePreference;
  providers: Array<ProviderEntry>;
  theme: ThemePreference;
  onAddAgentProfile: (entry: Omit<AgentProfile, "id">) => void;
  onDuplicateAgentProfile: (id: string) => void;
  onRemoveAgentProfile: (id: string) => void;
  onSelectLanguage: (language: LanguagePreference) => void;
  onSelectTheme: (theme: ThemePreference) => void;
  onSetActiveAgentProfile: (id: string) => void;
  onUpdateAgentProfile: (id: string, patch: Partial<Omit<AgentProfile, "id">>) => void;
  onUpdateGeneralPreference: <Key extends keyof GeneralPreferences>(key: Key, value: GeneralPreferences[Key]) => void;
}) {
  const availableModels = useMemo(() => {
    const models: Array<{
      providerId: string;
      providerName: string;
      modelRecordId: string;
      modelId: string;
      displayName: string;
    }> = [];
    for (const provider of providers) {
      if (!provider.enabled) continue;
      for (const model of provider.models) {
        if (!model.enabled) continue;
        models.push({
          providerId: provider.id,
          modelRecordId: model.id,
          modelId: model.modelId,
          displayName: model.displayName || model.modelId,
          providerName: provider.displayName,
        });
      }
    }
    return models;
  }, [providers]);

  const activeProfile = agentProfiles.find((p) => p.id === activeAgentProfileId) ?? agentProfiles[0];
  const selectedStyle = RESPONSE_STYLE_OPTIONS.find((option) => option.value === activeProfile.responseStyle) ?? RESPONSE_STYLE_OPTIONS[0];

  const handleAddProfile = () => {
    onAddAgentProfile({
      name: "New Profile",
      customInstructions: "",
      commitMessagePrompt: activeProfile.commitMessagePrompt,
      responseStyle: "balanced",
      responseLanguage: "English",
      commitMessageLanguage: "English",
      primaryProviderId: "",
      primaryModelId: "",
      assistantProviderId: "",
      assistantModelId: "",
      liteProviderId: "",
      liteModelId: "",
    });
  };

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
        <SectionDivider />
        <SettingsRow
          label="Startup"
          description="Automatically launch the app when you log in."
          control={
            <Switch
              size="sm"
              checked={generalPreferences.launchAtLogin}
              onCheckedChange={(checked) => onUpdateGeneralPreference("launchAtLogin", checked)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Prevent sleep while running"
          description="Keep the system awake while an agent run is active."
          control={
            <Switch
              size="sm"
              checked={generalPreferences.preventSleepWhileRunning}
              onCheckedChange={(checked) => onUpdateGeneralPreference("preventSleepWhileRunning", checked)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Close to tray"
          description="Minimize to system tray instead of quitting when the window is closed."
          control={
            <Switch
              size="sm"
              checked={generalPreferences.minimizeToTray}
              onCheckedChange={(checked) => onUpdateGeneralPreference("minimizeToTray", checked)}
            />
          }
        />
      </SettingsSection>

      <SettingsSection
        title="Agent Profiles"
        action={
          <div className="flex items-center gap-1.5">
            <ProfilePicker
              profiles={agentProfiles}
              activeProfileId={activeProfile.id}
              onSelect={onSetActiveAgentProfile}
              onRename={(id, name) => onUpdateAgentProfile(id, { name })}
              onDuplicate={onDuplicateAgentProfile}
              onDelete={onRemoveAgentProfile}
            />
            <button
              type="button"
              className="flex h-6 items-center gap-1 rounded-md px-1.5 text-[11px] font-medium text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={handleAddProfile}
            >
              <Plus className="size-3" />
              Add Profile
            </button>
          </div>
        }
      >
        <SettingsRow
          label="Response style"
          description={selectedStyle.description}
          control={
            <ChoiceGroup
              options={RESPONSE_STYLE_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={activeProfile.responseStyle}
              onValueChange={(value) => onUpdateAgentProfile(activeProfile.id, { responseStyle: value as PromptResponseStyle })}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Response language"
          description="The language used for agent responses."
          control={
            <Input
              value={activeProfile.responseLanguage}
              onChange={(event) => onUpdateAgentProfile(activeProfile.id, { responseLanguage: event.target.value })}
              className="w-40 text-[13px]"
              placeholder="English"
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Commit message language"
          description="The language used when the Git panel generates commit messages."
          control={
            <Input
              value={activeProfile.commitMessageLanguage}
              onChange={(event) => onUpdateAgentProfile(activeProfile.id, {
                commitMessageLanguage: event.target.value,
              })}
              className="w-40 text-[13px]"
              placeholder="English"
            />
          }
        />
        <SectionDivider />
        <ModelSelectRow
          label="Primary model"
          description="Handles the main task flow, deep reasoning, and the Plan Agent."
          providerId={activeProfile.primaryProviderId}
          modelRecordId={activeProfile.primaryModelId}
          availableModels={availableModels}
          onValueChange={(providerId, modelRecordId) => onUpdateAgentProfile(activeProfile.id, {
            primaryProviderId: providerId,
            primaryModelId: modelRecordId,
          })}
        />
        <SectionDivider />
        <ModelSelectRow
          label="Auxiliary model"
          description="Supports Explore and Review helper agents, with fallback to Primary when unset."
          providerId={activeProfile.assistantProviderId}
          modelRecordId={activeProfile.assistantModelId}
          availableModels={availableModels}
          onValueChange={(providerId, modelRecordId) => onUpdateAgentProfile(activeProfile.id, {
            assistantProviderId: providerId,
            assistantModelId: modelRecordId,
          })}
        />
        <SectionDivider />
        <ModelSelectRow
          label="Lightweight model"
          description="Handles title generation and quick internal summaries with a smaller, faster model."
          providerId={activeProfile.liteProviderId}
          modelRecordId={activeProfile.liteModelId}
          availableModels={availableModels}
          onValueChange={(providerId, modelRecordId) => onUpdateAgentProfile(activeProfile.id, {
            liteProviderId: providerId,
            liteModelId: modelRecordId,
          })}
        />
        <SectionDivider />
        <div className="px-4 py-3">
          <div className="mb-1 text-[13px] font-medium leading-5 text-app-foreground">Custom instructions</div>
          <p className="mb-3 text-[12px] leading-5 text-app-muted">
            Standing instruction applied to every thread. Use it to define the agent&apos;s personality, constraints, and default behavior.
          </p>
          <Textarea
            value={activeProfile.customInstructions}
            onChange={(event) => onUpdateAgentProfile(activeProfile.id, { customInstructions: event.target.value })}
            className="min-h-36"
          />
        </div>
        <SectionDivider />
        <div className="px-4 py-3">
          <div className="mb-1 text-[13px] font-medium leading-5 text-app-foreground">Commit message prompt</div>
          <p className="mb-3 text-[12px] leading-5 text-app-muted">
            Prompt used by the Git panel when generating commit messages for the current profile.
          </p>
          <Textarea
            value={activeProfile.commitMessagePrompt}
            onChange={(event) => onUpdateAgentProfile(activeProfile.id, { commitMessagePrompt: event.target.value })}
            className="h-48 min-h-48 overflow-y-auto [field-sizing:fixed]"
          />
        </div>
      </SettingsSection>
    </div>
  );
}

function ModelSelectRow({
  availableModels,
  description,
  label,
  providerId,
  modelRecordId,
  onValueChange,
}: {
  availableModels: Array<{
    providerId: string;
    providerName: string;
    modelRecordId: string;
    modelId: string;
    displayName: string;
  }>;
  description: string;
  label: string;
  providerId: string;
  modelRecordId: string;
  onValueChange: (providerId: string, modelRecordId: string) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const [dropdownStyle, setDropdownStyle] = useState<React.CSSProperties>({});

  useLayoutEffect(() => {
    if (!isOpen || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const dropdownMaxH = 256;
    const spaceBelow = window.innerHeight - rect.bottom - 8;
    const placeAbove = spaceBelow < dropdownMaxH && rect.top > spaceBelow;
    setDropdownStyle({
      position: "fixed",
      right: window.innerWidth - rect.right,
      minWidth: Math.max(rect.width, 280),
      maxHeight: dropdownMaxH,
      ...(placeAbove
        ? { bottom: window.innerHeight - rect.top + 4 }
        : { top: rect.bottom + 4 }),
    });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || dropdownRef.current?.contains(target)) return;
      setIsOpen(false);
    };
    document.addEventListener("pointerdown", handleClickOutside);
    return () => document.removeEventListener("pointerdown", handleClickOutside);
  }, [isOpen]);

  const selectedModel = availableModels.find((m) => m.providerId === providerId && m.modelRecordId === modelRecordId);
  const displayValue = selectedModel
    ? `${selectedModel.displayName}`
    : modelRecordId
      ? modelRecordId
      : "Not set";

  const grouped = useMemo(() => {
    const map = new Map<string, Array<{
      providerId: string;
      providerName: string;
      modelRecordId: string;
      modelId: string;
      displayName: string;
    }>>();
    for (const model of availableModels) {
      const list = map.get(model.providerName) ?? [];
      list.push(model);
      map.set(model.providerName, list);
    }
    return map;
  }, [availableModels]);

  return (
    <div className="grid gap-3 bg-app-surface px-4 py-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-center">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-app-foreground">{label}</p>
        <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
      </div>
      <div className="min-w-0 md:justify-self-end">
        <button
          ref={triggerRef}
          type="button"
          className={cn(
            "inline-flex min-h-8 w-full items-center justify-between gap-2 rounded-lg border border-app-border bg-app-surface-muted px-3 text-[12px] transition-colors hover:bg-app-surface-hover md:w-auto md:min-w-[200px]",
            !selectedModel && !modelRecordId && "text-app-muted",
          )}
          onClick={() => setIsOpen(!isOpen)}
        >
          <span className="flex min-w-0 items-center gap-2">
            {selectedModel ? (
              <ModelBrandIcon
                className="size-4"
                displayName={selectedModel.displayName}
                modelId={selectedModel.modelId}
              />
            ) : null}
            <span className="truncate">{displayValue}</span>
          </span>
          <ChevronDown className={cn("size-3 shrink-0 text-app-subtle transition-transform", isOpen && "rotate-180")} />
        </button>
        {isOpen
          ? createPortal(
              <div
                ref={dropdownRef}
                style={dropdownStyle}
                className="z-[100] overflow-y-auto rounded-lg border border-app-border bg-app-surface shadow-lg"
              >
                <button
                  type="button"
                  className={cn(
                    "flex w-full items-center px-3 py-2 text-left text-[12px] transition-colors hover:bg-app-surface-hover",
                    !modelRecordId && "text-app-accent",
                  )}
                  onClick={() => { onValueChange("", ""); setIsOpen(false); }}
                >
                  <span className="italic text-app-muted">Not set</span>
                </button>
                {[...grouped.entries()].map(([providerName, models]) => (
                  <div key={providerName}>
                    <div className="sticky top-0 bg-app-surface px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-app-subtle">
                      {providerName}
                    </div>
                    {models.map((model) => (
                      <button
                        key={`${model.providerId}:${model.modelRecordId}`}
                        type="button"
                        className={cn(
                          "flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] transition-colors hover:bg-app-surface-hover",
                          providerId === model.providerId && modelRecordId === model.modelRecordId && "text-app-accent",
                        )}
                        onClick={() => { onValueChange(model.providerId, model.modelRecordId); setIsOpen(false); }}
                      >
                        <ModelBrandIcon className="size-4" displayName={model.displayName} modelId={model.modelId} />
                        <span className="truncate">{model.displayName}</span>
                        <span className="ml-auto shrink-0 truncate font-mono text-[10px] text-app-subtle">{model.modelId.split("/").pop()}</span>
                      </button>
                    ))}
                  </div>
                ))}
                {availableModels.length === 0 ? (
                  <div className="px-3 py-4 text-center text-[12px] text-app-muted">
                    No models available. Enable providers and models first.
                  </div>
                ) : null}
              </div>,
              document.body,
            )
          : null}
      </div>
    </div>
  );
}

function AboutSettingsPanel({
  isCheckingUpdates,
  runtime,
  updateStatus,
  onCheckUpdates,
}: {
  isCheckingUpdates: boolean;
  runtime: SystemMetadata | null;
  updateStatus: string | null;
  onCheckUpdates: () => void;
}) {
  const version = runtime?.version ?? "0.1.0";
  const appName = runtime?.appName ?? "Tiy Agent";
  const platformSummary = runtime?.platform ?? "Unknown platform";
  const architectureSummary = runtime?.arch ?? "Unknown architecture";
  const aboutActions = [
    { href: "https://tiy.ai", label: "Official Website" },
    { href: "https://github.com/TiyAgents/tiy-desktop/blob/master/LICENSE", label: "License" },
    { href: "https://github.com/TiyAgents/tiy-desktop/issues", label: "Feedback" },
    { href: "mailto:contact@tiy.ai", label: "Contact Email" },
  ] as const;

  return (
    <div className="flex w-full items-center justify-center">
      <section className="flex w-full justify-center">
        <div className="flex flex-col items-center gap-5 px-5 py-7 text-center">
          <div className="flex size-20 items-center justify-center rounded-2xl border border-app-border bg-[linear-gradient(145deg,color-mix(in_srgb,var(--color-app-surface)_92%,white),color-mix(in_srgb,var(--color-app-surface-muted)_76%,white))] shadow-[0_8px_24px_rgba(15,23,42,0.06)] dark:bg-[linear-gradient(145deg,color-mix(in_srgb,var(--color-app-surface)_94%,white_4%),color-mix(in_srgb,var(--color-app-surface-muted)_82%,black_6%))] dark:shadow-[0_10px_24px_rgba(0,0,0,0.18)]">
            <img src="/icon/tiy.png" alt={`${appName} logo`} className="size-12 object-contain" />
          </div>

          <div className="max-w-[560px] space-y-2">
            <h2 className="text-[19px] font-semibold tracking-[-0.03em] text-app-foreground">{appName}</h2>
            <p className="text-[13px] leading-6 text-app-muted">
              A desktop coding partner grounded in your workspace, tools, models, and runtime context.
            </p>
            <p className="text-[12px] leading-5 text-app-subtle">{`Version v${version} • ${platformSummary} • ${architectureSummary}`}</p>
          </div>

          <div className="flex max-w-[560px] flex-wrap items-center justify-center gap-2">
            {aboutActions.map((action) => (
              <Button
                key={action.label}
                type="button"
                variant="outline"
                className="h-9 rounded-xl border-app-border bg-app-surface-muted px-4 text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
                onClick={() => {
                  void openUrl(action.href);
                }}
              >
                {action.label}
              </Button>
            ))}
          </div>

          <Button
            type="button"
            variant="outline"
            className="h-9 rounded-xl border-app-border bg-app-surface-muted px-4 text-[13px] font-medium text-app-foreground shadow-none hover:border-app-border-strong hover:bg-app-surface-hover"
            onClick={onCheckUpdates}
          >
            <RefreshCw data-icon="inline-start" className={cn("size-3.5", isCheckingUpdates && "animate-spin")} />
            {isCheckingUpdates ? "Checking for Updates..." : "Check for Updates"}
          </Button>

          <div className="flex min-h-10 items-center justify-center">
            <p className={cn("text-[12px] leading-5 text-app-subtle", !updateStatus && "invisible")} aria-live="polite">
              {updateStatus ?? "Update status placeholder"}
            </p>
          </div>

          <div className="flex w-full max-w-[560px] flex-col items-center gap-1.5 pt-1 text-[11px] leading-5 text-app-subtle">
            <p className="text-center">Copyright © 2026 Tiy.Ai All Rights Reserved.</p>

            <div className="flex items-center justify-center gap-3">
              <button
                type="button"
                className="transition-colors hover:text-app-foreground"
                onClick={() => {
                  void openUrl("https://tiy.ai/terms");
                }}
              >
                Terms of Service
              </button>
              <button
                type="button"
                className="transition-colors hover:text-app-foreground"
                onClick={() => {
                  void openUrl("https://tiy.ai/privacy");
                }}
              >
                Privacy Policy
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function CommandSettingsPanel({
  description,
  commands,
  onAddCommand,
  onRemoveCommand,
  onUpdateCommand,
}: {
  description: string;
  commands: CommandSettings;
  onAddCommand: (entry: Omit<CommandEntry, "id">) => void;
  onRemoveCommand: (id: string) => void;
  onUpdateCommand: (id: string, patch: Partial<Omit<CommandEntry, "id">>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Commands" description={description} />

      <CommandsSection
        commands={commands.commands}
        onAddCommand={onAddCommand}
        onRemoveCommand={onRemoveCommand}
        onUpdateCommand={onUpdateCommand}
      />
    </div>
  );
}

function CommandsSection({
  commands,
  onAddCommand,
  onRemoveCommand,
  onUpdateCommand,
}: {
  commands: Array<CommandEntry>;
  onAddCommand: (entry: Omit<CommandEntry, "id">) => void;
  onRemoveCommand: (id: string) => void;
  onUpdateCommand: (id: string, patch: Partial<Omit<CommandEntry, "id">>) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);

  const handleAddCommand = () => {
    onAddCommand({
      name: "",
      path: "",
      argumentHint: "",
      description: "",
      prompt: "",
    });
  };

  return (
    <SettingsSection
      title="Commands"
      action={
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
          onClick={handleAddCommand}
        >
          <Plus className="size-3.5" />
          <span>Add Prompt</span>
        </button>
      }
    >
      <div className="px-4 py-3">
        <p className="text-[12px] leading-5 text-app-muted">
          Create quick prompts that can be triggered by typing / in the chat
        </p>
      </div>
      {commands.length > 0 ? (
        <div className="flex flex-col">
          {commands.map((command) => (
            <CommandItem
              key={command.id}
              command={command}
              isEditing={editingId === command.id}
              onEdit={() => setEditingId(editingId === command.id ? null : command.id)}
              onCancelEdit={() => setEditingId(null)}
              onRemove={() => onRemoveCommand(command.id)}
              onUpdate={(patch) => onUpdateCommand(command.id, patch)}
            />
          ))}
        </div>
      ) : null}
    </SettingsSection>
  );
}

function CommandItem({
  command,
  isEditing,
  onEdit,
  onCancelEdit,
  onRemove,
  onUpdate,
}: {
  command: CommandEntry;
  isEditing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  onRemove: () => void;
  onUpdate: (patch: Partial<Omit<CommandEntry, "id">>) => void;
}) {
  const commandPath = command.name ? `/prompts:${command.name}` : "/prompts:unnamed";

  return (
    <div className="border-t border-app-border">
      <div className="flex items-center gap-3 px-4 py-3.5">
        <div className="shrink-0 text-app-subtle">
          <Zap className="size-4" />
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold text-app-foreground">{command.name || "Untitled"}</span>
            <span className="text-[12px] text-app-subtle">{commandPath}</span>
          </div>
          <p className="mt-1 truncate text-[12px] leading-5 text-app-muted">
            {command.description || <span className="italic">No description</span>}
          </p>
          <p className="mt-1 truncate text-[11px] leading-5 text-app-subtle">
            {command.argumentHint || <span className="italic">No argument hint</span>}
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {isEditing ? (
            <>
              <button
                type="button"
                title="Save changes"
                aria-label="Save changes"
                className="flex size-7 items-center justify-center rounded-md text-green-500 transition-colors hover:bg-app-surface-hover hover:text-green-600"
                onClick={onEdit}
              >
                <Check className="size-3.5" />
              </button>
              <button
                type="button"
                title="Cancel editing"
                aria-label="Cancel editing"
                className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                onClick={onCancelEdit}
              >
                <X className="size-3.5" />
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                title="Edit command"
                aria-label="Edit command"
                className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                onClick={onEdit}
              >
                <Pencil className="size-3.5" />
              </button>
              <button
                type="button"
                title="Remove command"
                aria-label="Remove command"
                className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-red-500"
                onClick={onRemove}
              >
                <Trash2 className="size-3.5" />
              </button>
            </>
          )}
        </div>
      </div>

      {isEditing ? (
        <div className="border-t border-dashed border-app-border bg-app-surface-muted px-4 py-3">
          <div>
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-app-subtle">Name</label>
            <Input
              value={command.name}
              onChange={(event) => onUpdate({ name: event.target.value })}
              placeholder="review"
              className="text-[13px]"
            />
            <p className="mt-1 text-[11px] text-app-subtle">Command path: {command.name ? `/prompts:${command.name}` : "/prompts:..."}</p>
          </div>
          <div className="mt-3">
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-app-subtle">Description</label>
            <Input
              value={command.description}
              onChange={(event) => onUpdate({ description: event.target.value })}
              placeholder="Describe what this command does in the picker"
              className="text-[13px]"
            />
          </div>
          <div className="mt-3">
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-app-subtle">Argument hint</label>
            <Input
              value={command.argumentHint}
              onChange={(event) => onUpdate({ argumentHint: event.target.value })}
              placeholder="[file=path] [focus=topic]"
              className="text-[13px]"
            />
          </div>
          <div className="mt-3">
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-app-subtle">Command prompt</label>
            <Textarea
              value={command.prompt}
              onChange={(event) => onUpdate({ prompt: event.target.value })}
              placeholder="Write the expanded prompt sent to the model when this command is used..."
              className="min-h-24 text-[13px]"
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function PolicySettingsPanel({
  description,
  policy,
  onAddAllowEntry,
  onAddDenyEntry,
  onAddWritableRoot,
  onRemoveAllowEntry,
  onRemoveDenyEntry,
  onRemoveWritableRoot,
  onUpdateAllowEntry,
  onUpdateDenyEntry,
  onUpdatePolicySetting,
  onUpdateWritableRoot,
}: {
  description: string;
  policy: PolicySettings;
  onAddAllowEntry: (entry: Omit<PatternEntry, "id">) => void;
  onAddDenyEntry: (entry: Omit<PatternEntry, "id">) => void;
  onAddWritableRoot: (entry: Omit<WritableRootEntry, "id">) => void;
  onRemoveAllowEntry: (id: string) => void;
  onRemoveDenyEntry: (id: string) => void;
  onRemoveWritableRoot: (id: string) => void;
  onUpdateAllowEntry: (id: string, patch: Partial<Omit<PatternEntry, "id">>) => void;
  onUpdateDenyEntry: (id: string, patch: Partial<Omit<PatternEntry, "id">>) => void;
  onUpdatePolicySetting: <Key extends keyof PolicySettings>(key: Key, value: PolicySettings[Key]) => void;
  onUpdateWritableRoot: (id: string, patch: Partial<Omit<WritableRootEntry, "id">>) => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Permissions" description={description} />

      <SettingsSection title="Execution">
        <SettingsRow
          label="Approval policy"
          description={APPROVAL_POLICY_OPTIONS.find((option) => option.value === policy.approvalPolicy)?.description ?? ""}
          control={
            <ChoiceGroup
              options={APPROVAL_POLICY_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={policy.approvalPolicy}
              onValueChange={(value) => onUpdatePolicySetting("approvalPolicy", value as ApprovalPolicy)}
            />
          }
        />
        <SectionDivider />
        <PatternSubsection
          title="Allowed"
          description="Patterns the agent can use without approval."
          entries={policy.allowList}
          onAdd={onAddAllowEntry}
          onRemove={onRemoveAllowEntry}
          onUpdate={onUpdateAllowEntry}
        />
        <SectionDivider />
        <PatternSubsection
          title="Denied"
          description="Patterns the agent must never use."
          entries={policy.denyList}
          onAdd={onAddDenyEntry}
          onRemove={onRemoveDenyEntry}
          onUpdate={onUpdateDenyEntry}
        />
      </SettingsSection>

      <SettingsSection title="Sandbox">
        <SettingsRow
          label="Sandbox policy"
          description={SANDBOX_POLICY_OPTIONS.find((option) => option.value === policy.sandboxPolicy)?.description ?? ""}
          control={
            <ChoiceGroup
              options={SANDBOX_POLICY_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={policy.sandboxPolicy}
              onValueChange={(value) => onUpdatePolicySetting("sandboxPolicy", value as SandboxPolicy)}
            />
          }
        />
        <SectionDivider />
        <SettingsRow
          label="Network access"
          description={NETWORK_ACCESS_OPTIONS.find((option) => option.value === policy.networkAccess)?.description ?? ""}
          control={
            <ChoiceGroup
              options={NETWORK_ACCESS_OPTIONS.map(({ label, value }) => ({ label, value }))}
              value={policy.networkAccess}
              onValueChange={(value) => onUpdatePolicySetting("networkAccess", value as NetworkAccessPolicy)}
            />
          }
        />
        <SectionDivider />
        <WritableRootsSubsection
          entries={policy.writableRoots}
          onAdd={onAddWritableRoot}
          onRemove={onRemoveWritableRoot}
          onUpdate={onUpdateWritableRoot}
        />
      </SettingsSection>
    </div>
  );
}

function PatternSubsection({
  description,
  entries,
  onAdd,
  onRemove,
  onUpdate,
  title,
}: {
  description: string;
  entries: Array<PatternEntry>;
  onAdd: (entry: Omit<PatternEntry, "id">) => void;
  onRemove: (id: string) => void;
  onUpdate: (id: string, patch: Partial<Omit<PatternEntry, "id">>) => void;
  title: string;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const pendingAddRef = useRef(false);

  useEffect(() => {
    if (pendingAddRef.current && entries.length > 0) {
      pendingAddRef.current = false;
      setEditingId(entries[entries.length - 1].id);
    }
  }, [entries]);

  const handleAdd = () => {
    pendingAddRef.current = true;
    onAdd({ pattern: "" });
  };

  return (
    <div>
      <div className="flex items-center justify-between px-4 py-3">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-app-foreground">{title}</p>
          <p className="mt-1 text-[12px] leading-5 text-app-muted">{description}</p>
        </div>
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
          onClick={handleAdd}
        >
          <Plus className="size-3.5" />
          <span>Add</span>
        </button>
      </div>
      {entries.length > 0 ? (
        <div className="mx-4 mb-3 mt-1 flex flex-col overflow-hidden rounded-lg border border-app-border bg-app-surface-muted">
          {entries.map((entry, index) => (
            <PatternItem
              key={entry.id}
              entry={entry}
              isEditing={editingId === entry.id}
              isFirst={index === 0}
              onCancelEdit={() => setEditingId(null)}
              onEdit={() => setEditingId(editingId === entry.id ? null : entry.id)}
              onRemove={() => onRemove(entry.id)}
              onUpdate={(patch) => onUpdate(entry.id, patch)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function PatternItem({
  entry,
  isEditing,
  isFirst,
  onCancelEdit,
  onEdit,
  onRemove,
  onUpdate,
}: {
  entry: PatternEntry;
  isEditing: boolean;
  isFirst: boolean;
  onCancelEdit: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onUpdate: (patch: Partial<Omit<PatternEntry, "id">>) => void;
}) {
  return (
    <div className={cn("flex items-center gap-3 px-3 py-2", !isFirst && "border-t border-dashed border-app-border")}>
      <div className="min-w-0 flex-1">
        {isEditing ? (
          <Input
            autoFocus
            value={entry.pattern}
            onChange={(event) => onUpdate({ pattern: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === "Enter") onEdit();
              if (event.key === "Escape") onCancelEdit();
            }}
            placeholder="e.g. rm -rf, curl *, Read"
            className="h-8 text-[13px]"
          />
        ) : (
          <span className="text-[13px] font-medium text-app-foreground">
            {entry.pattern || <span className="italic text-app-muted">Empty</span>}
          </span>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {isEditing ? (
          <>
            <button
              type="button"
              title="Confirm"
              aria-label="Confirm"
              className="flex size-7 items-center justify-center rounded-md text-green-500 transition-colors hover:bg-app-surface-hover hover:text-green-600"
              onClick={onEdit}
            >
              <Check className="size-3.5" />
            </button>
            <button
              type="button"
              title="Cancel"
              aria-label="Cancel"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onCancelEdit}
            >
              <X className="size-3.5" />
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              title="Edit pattern"
              aria-label="Edit pattern"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onEdit}
            >
              <Pencil className="size-3.5" />
            </button>
            <button
              type="button"
              title="Remove pattern"
              aria-label="Remove pattern"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-red-500"
              onClick={onRemove}
            >
              <Trash2 className="size-3.5" />
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function WritableRootsSubsection({
  entries,
  onAdd,
  onRemove,
  onUpdate,
}: {
  entries: Array<WritableRootEntry>;
  onAdd: (entry: Omit<WritableRootEntry, "id">) => void;
  onRemove: (id: string) => void;
  onUpdate: (id: string, patch: Partial<Omit<WritableRootEntry, "id">>) => void;
}) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const pendingAddRef = useRef(false);

  useEffect(() => {
    if (pendingAddRef.current && entries.length > 0) {
      pendingAddRef.current = false;
      setEditingId(entries[entries.length - 1].id);
    }
  }, [entries]);

  const handleAdd = () => {
    pendingAddRef.current = true;
    onAdd({ path: "" });
  };

  return (
    <div>
      <div className="flex items-center justify-between px-4 py-3">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-app-foreground">Writable roots</p>
          <p className="mt-1 text-[12px] leading-5 text-app-muted">Additional directories the sandbox is allowed to write to.</p>
        </div>
        <button
          type="button"
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
          onClick={handleAdd}
        >
          <Plus className="size-3.5" />
          <span>Add</span>
        </button>
      </div>
      {entries.length > 0 ? (
        <div className="mx-4 mb-3 mt-1 flex flex-col overflow-hidden rounded-lg border border-app-border bg-app-surface-muted">
          {entries.map((entry, index) => (
            <WritableRootItem
              key={entry.id}
              entry={entry}
              isEditing={editingId === entry.id}
              isFirst={index === 0}
              onCancelEdit={() => setEditingId(null)}
              onEdit={() => setEditingId(editingId === entry.id ? null : entry.id)}
              onRemove={() => onRemove(entry.id)}
              onUpdate={(patch) => onUpdate(entry.id, patch)}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function WritableRootItem({
  entry,
  isEditing,
  isFirst,
  onCancelEdit,
  onEdit,
  onRemove,
  onUpdate,
}: {
  entry: WritableRootEntry;
  isEditing: boolean;
  isFirst: boolean;
  onCancelEdit: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onUpdate: (patch: Partial<Omit<WritableRootEntry, "id">>) => void;
}) {
  return (
    <div className={cn("flex items-center gap-3 px-3 py-2", !isFirst && "border-t border-dashed border-app-border")}>
      <div className="shrink-0 text-app-subtle">
        <FolderOpen className="size-4" />
      </div>

      <div className="min-w-0 flex-1">
        {isEditing ? (
          <Input
            autoFocus
            value={entry.path}
            onChange={(event) => onUpdate({ path: event.target.value })}
            onKeyDown={(event) => {
              if (event.key === "Enter") onEdit();
              if (event.key === "Escape") onCancelEdit();
            }}
            placeholder="/path/to/directory"
            className="h-8 text-[13px]"
          />
        ) : (
          <span className="text-[13px] font-medium text-app-foreground">
            {entry.path || <span className="italic text-app-muted">Empty path</span>}
          </span>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1">
        {isEditing ? (
          <>
            <button
              type="button"
              title="Confirm"
              aria-label="Confirm"
              className="flex size-7 items-center justify-center rounded-md text-green-500 transition-colors hover:bg-app-surface-hover hover:text-green-600"
              onClick={onEdit}
            >
              <Check className="size-3.5" />
            </button>
            <button
              type="button"
              title="Cancel"
              aria-label="Cancel"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onCancelEdit}
            >
              <X className="size-3.5" />
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              title="Edit path"
              aria-label="Edit path"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
              onClick={onEdit}
            >
              <Pencil className="size-3.5" />
            </button>
            <button
              type="button"
              title="Remove path"
              aria-label="Remove path"
              className="flex size-7 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-red-500"
              onClick={onRemove}
            >
              <Trash2 className="size-3.5" />
            </button>
          </>
        )}
      </div>
    </div>
  );
}

const MODEL_CAPABILITY_META: ReadonlyArray<{
  icon: React.ComponentType<{ className?: string }>;
  key: keyof ProviderModelCapabilities;
  label: string;
}> = [
  { key: "vision", label: "Vision", icon: Eye },
  { key: "imageOutput", label: "Image Output", icon: Image },
  { key: "toolCalling", label: "Tool Calling", icon: Wrench },
  { key: "reasoning", label: "Reasoning", icon: Brain },
  { key: "embedding", label: "Embedding", icon: Database },
];

function formatCustomHeaders(customHeaders: Record<string, string> | undefined) {
  return JSON.stringify(customHeaders ?? {}, null, 2);
}

function parseCustomHeadersInput(value: string): { customHeaders: Record<string, string> | null; error: string | null } {
  if (!value.trim()) {
    return { customHeaders: {}, error: null };
  }

  try {
    const parsed = JSON.parse(value) as unknown;

    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return {
        customHeaders: null,
        error: "Custom HTTP Headers must be a JSON object.",
      };
    }

    const invalidEntry = Object.entries(parsed).find(([, headerValue]) => typeof headerValue !== "string");
    if (invalidEntry) {
      return {
        customHeaders: null,
        error: `Header "${invalidEntry[0]}" must use a string value.`,
      };
    }

    return {
      customHeaders: parsed as Record<string, string>,
      error: null,
    };
  } catch {
    return {
      customHeaders: null,
      error: "Invalid JSON. Example: {\"HTTP-Referer\": \"https://example.com\"}",
    };
  }
}

function formatProviderOptions(providerOptions: Record<string, unknown> | undefined) {
  return JSON.stringify(providerOptions ?? {}, null, 2);
}

function parseProviderOptionsInput(value: string): { error: string | null; providerOptions: Record<string, unknown> | null } {
  if (!value.trim()) {
    return { providerOptions: {}, error: null };
  }

  try {
    const parsed = JSON.parse(value) as unknown;

    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return {
        providerOptions: null,
        error: "Provider Options must be a JSON object.",
      };
    }

    return {
      providerOptions: parsed as Record<string, unknown>,
      error: null,
    };
  } catch {
    return {
      providerOptions: null,
      error: "Invalid JSON. Example: {\"thinking\": {\"type\": \"disabled\"}}",
    };
  }
}

function inferModelCapabilities(modelId: string): ProviderModelCapabilities {
  const normalized = modelId.toLowerCase();
  const embedding = /\bembed|embedding\b/.test(normalized);
  const imageOutput = /\b(image|images|gpt-image|flux|sdxl|seedream|dall-e)\b/.test(normalized);
  const vision = /\b(vision|vl|gpt-4o|gpt-4\.1|claude|gemini|pixtral|llava)\b/.test(normalized);
  const reasoning = /\b(gpt-5|o1|o3|o4|r1|reason|thinking|claude-3\.7|gemini-2\.5|step-3)\b/.test(normalized);
  const toolCalling = !embedding && !imageOutput && /\b(gpt|claude|gemini|deepseek|moonshot|qwen|llama|mistral|step|openai|anthropic|doubao)\b/.test(normalized);

  return {
    vision,
    imageOutput,
    toolCalling,
    reasoning,
    embedding,
  };
}

function getEffectiveModelCapabilities(model: ProviderModel): ProviderModelCapabilities {
  return {
    ...inferModelCapabilities(model.modelId),
    ...model.capabilityOverrides,
  };
}

function ProviderSettingsPanel({
  description,
  providerCatalog,
  providers,
  onAddProvider,
  onFetchProviderModels,
  onTestProviderModelConnection,
  onRemoveProvider,
  onUpdateProvider,
}: {
  description: string;
  providerCatalog: Array<ProviderCatalogEntry>;
  providers: Array<ProviderEntry>;
  onAddProvider: (entry: Omit<ProviderEntry, "id">) => void;
  onFetchProviderModels: (id: string) => Promise<void>;
  onTestProviderModelConnection: (
    providerId: string,
    modelId: string,
  ) => Promise<ProviderModelConnectionTestResultDto>;
  onRemoveProvider: (id: string) => void;
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
}) {
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    () => providers[0]?.id ?? null,
  );
  const [providerSearch, setProviderSearch] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [showAdvancedSettings, setShowAdvancedSettings] = useState(false);
  const [customHeadersInput, setCustomHeadersInput] = useState("{}");
  const [customHeadersError, setCustomHeadersError] = useState<string | null>(null);
  const [expandedModelId, setExpandedModelId] = useState<string | null>(null);
  const [modelSearch, setModelSearch] = useState("");
  const [newModelId, setNewModelId] = useState("");
  const [newModelDisplayName, setNewModelDisplayName] = useState("");
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [fetchFeedback, setFetchFeedback] = useState<{ kind: "success" | "error"; message: string } | null>(null);
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [modelTestFeedback, setModelTestFeedback] = useState<Record<string, ProviderModelConnectionTestResultDto>>({});
  const [isMiniMaxCustomBaseUrl, setIsMiniMaxCustomBaseUrl] = useState(false);
  const modelTestFeedbackTimeoutsRef = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  const selectedProvider = providers.find((provider) => provider.id === selectedProviderId) ?? null;
  const isMiniMaxProvider = selectedProvider?.providerKey === "minimax";
  const minimaxBaseUrlMode =
    isMiniMaxCustomBaseUrl
      ? "__custom__"
      : selectedProvider && MINIMAX_BASE_URL_OPTIONS.includes(selectedProvider.baseUrl as typeof MINIMAX_BASE_URL_OPTIONS[number])
      ? selectedProvider.baseUrl
      : "__custom__";

  const filteredProviders = useMemo(() => {
    if (!providerSearch.trim()) return providers;
    const query = providerSearch.toLowerCase();
    return providers.filter((provider) => provider.displayName.toLowerCase().includes(query));
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

  const customProviderTypeOptions = useMemo(
    () =>
      providerCatalog
        .filter((entry) => entry.supportsCustom)
        .map((entry) => ({
          value: entry.providerType as CustomProviderType,
          label: entry.displayName,
          defaultBaseUrl: entry.defaultBaseUrl,
        })),
    [providerCatalog],
  );

  useEffect(() => {
    if (!providers.some((provider) => provider.id === selectedProviderId)) {
      setSelectedProviderId(providers[0]?.id ?? null);
    }
  }, [providers, selectedProviderId]);

  useEffect(() => {
    if (!selectedProvider) {
      setShowAdvancedSettings(false);
      setCustomHeadersInput("{}");
      setCustomHeadersError(null);
      setIsMiniMaxCustomBaseUrl(false);
      return;
    }

    const hasCustomHeaders = Object.keys(selectedProvider.customHeaders).length > 0;
    setApiKeyDraft(selectedProvider.apiKey);
    setShowAdvancedSettings(hasCustomHeaders);
    setCustomHeadersInput(formatCustomHeaders(selectedProvider.customHeaders));
    setCustomHeadersError(null);
    setIsMiniMaxCustomBaseUrl(
      selectedProvider.providerKey === "minimax"
      && !MINIMAX_BASE_URL_OPTIONS.includes(selectedProvider.baseUrl as typeof MINIMAX_BASE_URL_OPTIONS[number]),
    );
  }, [selectedProvider?.id]);

  useEffect(() => {
    if (!selectedProvider?.models.some((model) => model.id === expandedModelId)) {
      setExpandedModelId(null);
    }
  }, [expandedModelId, selectedProvider]);

  useEffect(() => {
    setFetchFeedback(null);
    setIsFetchingModels(false);
    setTestingModelId(null);
    Object.values(modelTestFeedbackTimeoutsRef.current).forEach((timeoutId) => clearTimeout(timeoutId));
    modelTestFeedbackTimeoutsRef.current = {};
    setModelTestFeedback({});
  }, [selectedProvider?.id]);

  useEffect(() => () => {
    Object.values(modelTestFeedbackTimeoutsRef.current).forEach((timeoutId) => clearTimeout(timeoutId));
  }, []);

  const handleAddCustomProvider = () => {
    const newProvider: Omit<ProviderEntry, "id"> = {
      kind: "custom",
      providerKey: crypto.randomUUID(),
      providerType: customProviderTypeOptions[0]?.value ?? "openai-compatible",
      displayName: "Custom Provider",
      baseUrl: customProviderTypeOptions[0]?.defaultBaseUrl ?? "https://api.example.com/v1",
      apiKey: "",
      hasApiKey: false,
      lockedMapping: false,
      customHeaders: {},
      enabled: false,
      models: [],
    };
    onAddProvider(newProvider);
  };

  const handleCustomHeadersChange = (value: string) => {
    setCustomHeadersInput(value);
    const { customHeaders, error } = parseCustomHeadersInput(value);
    setCustomHeadersError(error);
    if (!selectedProvider || !customHeaders) {
      return;
    }

    onUpdateProvider(selectedProvider.id, { customHeaders });
  };

  const handleApiKeySave = () => {
    if (!selectedProvider) {
      return;
    }

    const value = apiKeyDraft.trim();
    if (!value) {
      return;
    }

    onUpdateProvider(selectedProvider.id, {
      apiKey: value,
      enabled: true,
    });
  };

  const handleUpdateModel = (modelId: string, patch: Partial<ProviderModel>) => {
    if (!selectedProvider) return;
    onUpdateProvider(selectedProvider.id, {
      models: selectedProvider.models.map((model) =>
        model.id === modelId ? { ...model, ...patch } : model,
      ),
    });
  };

  const handleToggleModel = (modelId: string, enabled: boolean) => {
    handleUpdateModel(modelId, { enabled });
  };

  const handleRemoveModel = (modelId: string) => {
    if (!selectedProvider) return;
    if (expandedModelId === modelId) {
      setExpandedModelId(null);
    }
    onUpdateProvider(selectedProvider.id, {
      models: selectedProvider.models.filter((model) => model.id !== modelId),
    });
  };

  const handleAddModel = () => {
    if (!selectedProvider || !newModelId.trim()) return;
    const newModel: ProviderModel = {
      id: crypto.randomUUID(),
      modelId: newModelId.trim(),
      sortIndex: selectedProvider.models.reduce((max, model) => Math.max(max, model.sortIndex), -1) + 1,
      displayName: newModelDisplayName.trim(),
      enabled: true,
      capabilityOverrides: {},
      providerOptions: {},
      isManual: true,
    };
    onUpdateProvider(selectedProvider.id, {
      models: [newModel, ...selectedProvider.models],
    });
    setNewModelId("");
    setNewModelDisplayName("");
    setExpandedModelId(newModel.id);
  };

  const handleFetchModels = async () => {
    if (!selectedProvider || isFetchingModels) {
      return;
    }

    setIsFetchingModels(true);
    setFetchFeedback(null);

    try {
      await onFetchProviderModels(selectedProvider.id);
      setFetchFeedback({
        kind: "success",
        message: "Model catalog updated from provider API.",
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : "Failed to fetch provider models.";
      setFetchFeedback({
        kind: "error",
        message,
      });
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleTestModelConnection = async (modelId: string) => {
    if (!selectedProvider || testingModelId === modelId) {
      return;
    }

    setTestingModelId(modelId);

    try {
      const result = await onTestProviderModelConnection(selectedProvider.id, modelId);
      setModelTestFeedback((current) => ({
        ...current,
        [modelId]: result,
      }));
      if (modelTestFeedbackTimeoutsRef.current[modelId]) {
        clearTimeout(modelTestFeedbackTimeoutsRef.current[modelId]);
      }
      modelTestFeedbackTimeoutsRef.current[modelId] = setTimeout(() => {
        setModelTestFeedback((current) => {
          const next = { ...current };
          delete next[modelId];
          return next;
        });
        delete modelTestFeedbackTimeoutsRef.current[modelId];
      }, 5000);
    } catch (error) {
      const detail = error instanceof Error ? error.message : "Unknown error";
      setModelTestFeedback((current) => ({
        ...current,
        [modelId]: {
          success: false,
          unsupported: false,
          message: "Connection test failed.",
          detail,
        },
      }));
      if (modelTestFeedbackTimeoutsRef.current[modelId]) {
        clearTimeout(modelTestFeedbackTimeoutsRef.current[modelId]);
      }
      modelTestFeedbackTimeoutsRef.current[modelId] = setTimeout(() => {
        setModelTestFeedback((current) => {
          const next = { ...current };
          delete next[modelId];
          return next;
        });
        delete modelTestFeedbackTimeoutsRef.current[modelId];
      }, 5000);
    } finally {
      setTestingModelId((current) => (current === modelId ? null : current));
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="Providers" description={description} />

      <section>
        <div className="mb-2 flex items-center justify-between px-1">
          <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Providers</h2>
          <button
            type="button"
            className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
            onClick={handleAddCustomProvider}
          >
            <Plus className="size-3.5" />
            <span>Add Provider</span>
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
                      <ProviderIcon name={provider.displayName} className="size-5 shrink-0" />
                      <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{provider.displayName}</span>
                      {provider.kind === "custom" ? (
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
                      <h3 className="text-[15px] font-semibold text-app-foreground">{selectedProvider.displayName}</h3>
                      {selectedProvider.kind === "custom" ? (
                        <span className="rounded-md border border-app-border bg-app-surface-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-app-muted">
                          Custom
                        </span>
                      ) : (
                        <span className="rounded-md border border-app-border bg-app-surface-muted px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide text-app-muted">
                          Built-in
                        </span>
                      )}
                      {selectedProvider.enabled ? (
                        <span className="rounded-md bg-app-success/15 px-1.5 py-0.5 text-[10px] font-medium text-app-success">
                          Active
                        </span>
                      ) : null}
                    </div>
                    <p className="mt-0.5 truncate text-[12px] text-app-subtle">{selectedProvider.baseUrl}</p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {selectedProvider.kind === "custom" ? (
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
                      value={selectedProvider.displayName}
                      onChange={(event) => onUpdateProvider(selectedProvider.id, { displayName: event.target.value })}
                      disabled={selectedProvider.lockedMapping}
                    />
                  </ProviderField>

                  <ProviderField label="Base URL">
                    {isMiniMaxProvider ? (
                      <div className="space-y-2">
                        <div className="relative">
                          <select
                            value={minimaxBaseUrlMode}
                            onChange={(event) => {
                              if (event.target.value === "__custom__") {
                                setIsMiniMaxCustomBaseUrl(true);
                                return;
                              }

                              setIsMiniMaxCustomBaseUrl(false);
                              onUpdateProvider(selectedProvider.id, { baseUrl: event.target.value });
                            }}
                            className="h-9 w-full appearance-none rounded-lg border border-app-border bg-app-surface-muted px-3 pr-8 text-[13px] text-app-foreground outline-none transition-colors focus-visible:border-app-border-strong"
                          >
                            {MINIMAX_BASE_URL_OPTIONS.map((option) => (
                              <option key={option} value={option}>
                                {option}
                              </option>
                            ))}
                            <option value="__custom__">Custom Base URL</option>
                          </select>
                          <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-app-subtle" />
                        </div>
                        {minimaxBaseUrlMode === "__custom__" ? (
                          <Input
                            value={selectedProvider.baseUrl}
                            onChange={(event) => onUpdateProvider(selectedProvider.id, { baseUrl: event.target.value })}
                            placeholder="https://your-endpoint.example.com/anthropic"
                          />
                        ) : null}
                        <p className="text-[11px] text-app-subtle">
                          Choose a recommended MiniMax endpoint or enter your own Base URL.
                        </p>
                      </div>
                    ) : (
                      <Input
                        value={selectedProvider.baseUrl}
                        onChange={(event) => onUpdateProvider(selectedProvider.id, { baseUrl: event.target.value })}
                      />
                    )}
                  </ProviderField>

                <ProviderField label="API Key">
                  <div className="relative">
                    <Input
                      type={showApiKey ? "text" : "password"}
                      value={apiKeyDraft}
                      onChange={(event) => setApiKeyDraft(event.target.value)}
                      onBlur={handleApiKeySave}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          handleApiKeySave();
                        }
                      }}
                      className="pr-10"
                      placeholder={selectedProvider.hasApiKey && !apiKeyDraft ? "Saved in app" : "Not set"}
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

                <ProviderField
                  label="Provider Type"
                  action={(
                    <button
                      type="button"
                      title="Provider settings"
                      aria-label="Provider settings"
                      aria-pressed={showAdvancedSettings}
                      onClick={() => setShowAdvancedSettings((current) => !current)}
                      className={cn(
                        "flex size-8 items-center justify-center rounded-lg border transition-colors",
                        showAdvancedSettings
                          ? "border-app-border-strong bg-app-surface-hover text-app-foreground"
                          : "border-app-border text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                      )}
                    >
                      <Settings2 className="size-3.5" />
                    </button>
                  )}
                >
                  {selectedProvider.lockedMapping ? (
                    <div className="rounded-lg border border-app-border bg-app-surface-muted px-3 py-2 text-[13px] text-app-foreground">
                      {selectedProvider.providerType}
                    </div>
                  ) : (
                    <div className="relative">
                      <select
                        value={selectedProvider.providerType}
                        onChange={(event) =>
                          onUpdateProvider(selectedProvider.id, { providerType: event.target.value as CustomProviderType })
                        }
                        className="h-9 w-full appearance-none rounded-lg border border-app-border bg-app-surface-muted px-3 pr-8 text-[13px] text-app-foreground outline-none transition-colors focus-visible:border-app-border-strong"
                      >
                        {customProviderTypeOptions.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                      <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-app-subtle" />
                    </div>
                  )}
                  <p className="mt-1.5 text-[11px] text-app-subtle">
                    {selectedProvider.lockedMapping
                      ? "Built-in providers stay mapped to their tiy-core provider type."
                      : "Choose which tiy-core provider type backs this custom provider."}
                  </p>
                  {showAdvancedSettings ? (
                    <div className="mt-3 rounded-xl border border-app-border bg-app-surface-muted/70 p-3">
                      <div className="mb-1 text-[12px] font-medium text-app-foreground">Custom HTTP Headers</div>
                      <p className="mb-3 text-[11px] leading-5 text-app-subtle">
                        Use JSON object format. Header values must be strings.
                      </p>
                      <Textarea
                        value={customHeadersInput}
                        onChange={(event) => handleCustomHeadersChange(event.target.value)}
                        aria-invalid={Boolean(customHeadersError)}
                        className="min-h-28 bg-app-surface text-[12px] leading-5"
                        placeholder={"{\n  \"HTTP-Referer\": \"https://example.com\",\n  \"X-Client\": \"tiy-desktop\"\n}"}
                        spellCheck={false}
                      />
                      <p className={cn("mt-2 text-[11px]", customHeadersError ? "text-app-danger" : "text-app-subtle")}>
                        {customHeadersError ?? "Saved automatically when the JSON is valid."}
                      </p>
                    </div>
                  ) : null}
                </ProviderField>

                {/* Models section */}
                <div>
                  <div className="mb-3">
                    <h4 className="text-[13px] font-medium text-app-foreground">Models</h4>
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
                  <div className="mb-3 flex items-center gap-2">
                    <div className="flex min-w-0 flex-1 items-center gap-2 rounded-lg border border-app-border bg-app-surface-muted px-3 py-1.5">
                      <Search className="size-3.5 shrink-0 text-app-subtle" />
                      <input
                        type="text"
                        placeholder="Search models..."
                        value={modelSearch}
                        onChange={(event) => setModelSearch(event.target.value)}
                        className="min-w-0 flex-1 bg-transparent text-[12px] text-app-foreground placeholder:text-app-subtle outline-none"
                      />
                    </div>
                    <button
                      type="button"
                      className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-2.5 py-1.5 text-[12px] font-medium text-app-foreground transition-colors hover:bg-app-surface-hover"
                      onClick={() => {
                        void handleFetchModels();
                      }}
                      disabled={isFetchingModels}
                    >
                      {isFetchingModels ? <RefreshCw className="size-3 animate-spin" /> : <Download className="size-3" />}
                      <span>{isFetchingModels ? "Fetching..." : "Fetch"}</span>
                    </button>
                  </div>

                  <p className={cn(
                    "mb-2 text-[11px]",
                    fetchFeedback?.kind === "error" ? "text-app-danger" : "text-app-subtle",
                  )}>
                    {fetchFeedback?.message
                      ?? `Showing ${filteredModels.length} model${filteredModels.length !== 1 ? "s" : ""} (enabled models shown first)`}
                  </p>

                  {/* Model list */}
                  <div className="space-y-1">
                    {[...filteredModels]
                      .sort((a, b) => {
                        if (a.enabled !== b.enabled) {
                          return a.enabled ? -1 : 1;
                        }
                        if (!a.enabled && !b.enabled && Boolean(a.isManual) !== Boolean(b.isManual)) {
                          return a.isManual ? -1 : 1;
                        }
                        return a.sortIndex - b.sortIndex;
                      })
                      .map((model) => (
                        <ProviderModelRow
                          key={model.id}
                          model={model}
                          isExpanded={expandedModelId === model.id}
                          isTesting={testingModelId === model.id}
                          testFeedback={modelTestFeedback[model.id] ?? null}
                          onTestConnection={() => {
                            void handleTestModelConnection(model.id);
                          }}
                          onToggleExpanded={() =>
                            setExpandedModelId((current) => (current === model.id ? null : model.id))
                          }
                          onToggleEnabled={(checked) => handleToggleModel(model.id, checked)}
                          onRemove={() => handleRemoveModel(model.id)}
                          onUpdate={(patch) => handleUpdateModel(model.id, patch)}
                        />
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
      </section>
    </div>
  );
}

function ProviderModelRow({
  isExpanded,
  isTesting,
  model,
  testFeedback,
  onRemove,
  onTestConnection,
  onToggleEnabled,
  onToggleExpanded,
  onUpdate,
}: {
  isExpanded: boolean;
  isTesting: boolean;
  model: ProviderModel;
  testFeedback: ProviderModelConnectionTestResultDto | null;
  onRemove: () => void;
  onTestConnection: () => void;
  onToggleEnabled: (checked: boolean) => void;
  onToggleExpanded: () => void;
  onUpdate: (patch: Partial<ProviderModel>) => void;
}) {
  const effectiveCapabilities = getEffectiveModelCapabilities(model);
  const activeCapabilities = MODEL_CAPABILITY_META.filter((item) => effectiveCapabilities[item.key]);
  const [providerOptionsInput, setProviderOptionsInput] = useState(() => formatProviderOptions(model.providerOptions));
  const [providerOptionsError, setProviderOptionsError] = useState<string | null>(null);

  useEffect(() => {
    setProviderOptionsInput(formatProviderOptions(model.providerOptions));
    setProviderOptionsError(null);
  }, [model.id]);

  const handleProviderOptionsChange = (value: string) => {
    setProviderOptionsInput(value);
    const { providerOptions, error } = parseProviderOptionsInput(value);
    setProviderOptionsError(error);
    if (!providerOptions) {
      return;
    }

    onUpdate({ providerOptions });
  };

  const handleCapabilityToggle = (key: keyof ProviderModelCapabilities, checked: boolean) => {
    const inferredCapabilities = inferModelCapabilities(model.modelId);
    const nextOverrides = { ...model.capabilityOverrides };

    if (checked === inferredCapabilities[key]) {
      delete nextOverrides[key];
    } else {
      nextOverrides[key] = checked;
    }

    onUpdate({ capabilityOverrides: nextOverrides });
  };

  return (
    <div className="overflow-hidden rounded-xl border border-app-border bg-app-surface-muted">
      <div className="group/model flex items-center justify-between gap-3 px-3.5 py-2.5 transition-colors hover:bg-app-surface-hover/50">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <ModelBrandIcon modelId={model.modelId} displayName={model.displayName} className="size-5 text-[16px]" />
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
            <div className="mt-1 flex min-w-0 items-center gap-2">
              {activeCapabilities.length > 0 ? (
                <div className="flex shrink-0 items-center gap-1">
                  {activeCapabilities.map((capability) => {
                    const Icon = capability.icon;
                    return (
                      <span
                        key={capability.key}
                        title={capability.label}
                        className="flex size-5 items-center justify-center rounded-md border border-app-border bg-app-surface text-app-subtle"
                      >
                        <Icon className="size-3" />
                      </span>
                    );
                  })}
                </div>
              ) : null}
              {model.contextWindow ? (
                <span className="inline-flex shrink-0 items-center rounded-md border border-app-border bg-app-surface px-1.5 py-0.5 text-[10px] font-medium text-app-muted">
                  {model.contextWindow}
                </span>
              ) : null}
              <span className="truncate font-mono text-[11px] text-app-subtle">
                {model.modelId}
              </span>
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <div
            className={cn(
              "flex items-center gap-1 transition-opacity",
              isExpanded
                ? "visible opacity-100"
                : "pointer-events-none invisible opacity-0 group-hover/model:pointer-events-auto group-hover/model:visible group-hover/model:opacity-100 group-focus-within/model:pointer-events-auto group-focus-within/model:visible group-focus-within/model:opacity-100",
            )}
          >
            <button
              type="button"
              title={isTesting ? "Testing connection" : "Test connection"}
              aria-label="Test model connection"
              onClick={onTestConnection}
              disabled={isTesting}
              className={cn(
                "flex size-6 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface hover:text-app-foreground",
                isTesting && "cursor-wait opacity-70",
              )}
            >
              {isTesting ? <RefreshCw className="size-3 animate-spin" /> : <MousePointerClick className="size-3" />}
            </button>
            <button
              type="button"
              title="Settings"
              aria-label="Model settings"
              aria-expanded={isExpanded}
              onClick={onToggleExpanded}
              className={cn(
                "flex size-6 items-center justify-center rounded-md transition-colors",
                isExpanded
                  ? "bg-app-surface text-app-foreground"
                  : "text-app-subtle hover:bg-app-surface hover:text-app-foreground",
              )}
            >
              <Settings2 className="size-3" />
            </button>
            {model.isManual ? (
              <button
                type="button"
                title="Remove model"
                aria-label="Remove model"
                className="flex size-6 items-center justify-center rounded-md text-app-subtle transition-colors hover:bg-app-surface hover:text-app-danger"
                onClick={onRemove}
              >
                <Trash2 className="size-3" />
              </button>
            ) : null}
          </div>
          <Switch
            size="sm"
            checked={model.enabled}
            aria-label={`Toggle ${model.displayName || model.modelId}`}
            onCheckedChange={onToggleEnabled}
          />
        </div>
      </div>

      {isTesting || testFeedback ? (
        <div className="border-t border-app-border/70 bg-app-surface px-4 py-2">
          <p
            className={cn(
              "text-[11px] leading-5",
              isTesting
                ? "text-app-subtle"
                : testFeedback?.success
                ? "text-app-success"
                : testFeedback?.unsupported
                ? "text-app-muted"
                : "text-app-danger",
            )}
          >
            {isTesting
              ? "Testing connection..."
              : testFeedback?.detail
              ? `${testFeedback.message} ${testFeedback.detail}`
              : testFeedback?.message}
          </p>
        </div>
      ) : null}

      {isExpanded ? (
        <div className="border-t border-app-border bg-app-surface px-4 py-4">
          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">Model ID</label>
              <Input
                value={model.modelId}
                onChange={(event) => onUpdate({ modelId: event.target.value })}
                placeholder="gpt-4o"
              />
            </div>
            <div>
              <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">Display Name</label>
              <Input
                value={model.displayName}
                onChange={(event) => onUpdate({ displayName: event.target.value })}
                placeholder="GPT-4o"
              />
            </div>
          </div>

          <div className="mb-3 mt-4">
            <h5 className="text-[13px] font-medium text-app-foreground">Model Capabilities</h5>
            <p className="mt-1 text-[12px] text-app-muted">
              Override the auto-detected capabilities for this model.
            </p>
          </div>

          <div className="grid gap-3 md:grid-cols-2">
            {MODEL_CAPABILITY_META.map((capability) => {
              const Icon = capability.icon;
              const isAuto = model.capabilityOverrides[capability.key] === undefined;
              return (
                <div
                  key={capability.key}
                  className="flex items-center justify-between rounded-xl border border-app-border bg-app-surface-muted px-3.5 py-3"
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <span className="flex size-7 items-center justify-center rounded-lg bg-app-surface text-app-subtle">
                      <Icon className="size-4" />
                    </span>
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5">
                        <span className="text-[13px] font-medium text-app-foreground">{capability.label}</span>
                        {isAuto ? (
                          <span className="text-[11px] font-medium text-app-subtle">(auto)</span>
                        ) : null}
                      </div>
                    </div>
                  </div>
                  <Switch
                    checked={effectiveCapabilities[capability.key]}
                    onCheckedChange={(checked) => handleCapabilityToggle(capability.key, checked)}
                  />
                </div>
              );
            })}
          </div>

          <div className="mt-4 grid gap-3 md:grid-cols-2">
            <div>
              <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">Context Window</label>
              <Input
                value={model.contextWindow ?? ""}
                onChange={(event) => onUpdate({ contextWindow: event.target.value })}
                placeholder="256000"
              />
            </div>
            <div>
              <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">Max Output Tokens</label>
              <Input
                value={model.maxOutputTokens ?? ""}
                onChange={(event) => onUpdate({ maxOutputTokens: event.target.value })}
                placeholder="256000"
              />
            </div>
          </div>

          <div className="mt-4">
            <label className="mb-1.5 block text-[13px] font-medium text-app-foreground">Model Options (JSON)</label>
            <Textarea
              value={providerOptionsInput}
              onChange={(event) => handleProviderOptionsChange(event.target.value)}
              aria-invalid={Boolean(providerOptionsError)}
              className="min-h-40 bg-app-surface text-[12px] leading-6"
              placeholder={"{\n  \"thinking\": {\n    \"type\": \"disabled\"\n  }\n}"}
              spellCheck={false}
            />
            <p className={cn("mt-2 text-[11px] leading-5", providerOptionsError ? "text-app-danger" : "text-app-subtle")}>
              {providerOptionsError ?? "Example: { \"thinking\": { \"type\": \"disabled\" } }"}
            </p>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function ProviderField({
  action,
  children,
  label,
}: {
  action?: ReactNode;
  children: ReactNode;
  label: string;
}) {
  return (
    <div>
      <div className="mb-1.5 flex items-center justify-between gap-3">
        <label className="block text-[13px] font-medium text-app-foreground">{label}</label>
        {action ? <div className="shrink-0">{action}</div> : null}
      </div>
      {children}
    </div>
  );
}

function ProviderIcon({ className, name }: { className?: string; name: string }) {
  const slug = matchProviderIcon(name);
  if (slug) {
    return <LocalLlmIcon className={cn("text-app-muted", className)} slug={slug} title={name} />;
  }
  const initial = getDisplayInitial(name);
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

function getDisplayInitial(value?: string) {
  const candidate = value?.trim();
  return candidate ? candidate.charAt(0).toUpperCase() : "?";
}

function WorkspaceSettingsPanel({
  description,
  systemMetadata,
  workspaces,
  onAddWorkspace,
  onRemoveWorkspace,
  onSetDefaultWorkspace,
}: {
  description: string;
  systemMetadata: SystemMetadata | null;
  workspaces: Array<WorkspaceEntry>;
  onAddWorkspace: (entry: Omit<WorkspaceEntry, "id">) => void;
  onRemoveWorkspace: (id: string) => void;
  onSetDefaultWorkspace: (id: string) => void;
}) {
  const [activeOpenWorkspaceId, setActiveOpenWorkspaceId] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const isMacOS = systemMetadata?.platform === "macos"
    || (typeof navigator !== "undefined" && navigator.userAgent.includes("Mac"));
  const isWindows = systemMetadata?.platform === "windows"
    || (typeof navigator !== "undefined" && navigator.userAgent.includes("Windows"));
  const openWorkspaceLabel = isWindows ? "Open in Explorer" : isMacOS ? "Open in Finder" : "Open folder";

  const handleAddWorkspace = () => {
    if (!isTauri()) {
      onAddWorkspace({
        name: "New Workspace",
        path: "/Users/jorben/Documents/Codespace/new-project",
        isDefault: false,
        isGit: false,
        autoWorkTree: false,
      });
      return;
    }

    void open({
      directory: true,
      multiple: false,
      title: "Choose workspace folder",
    }).then((selectedPath) => {
      if (typeof selectedPath !== "string") {
        return;
      }

      onAddWorkspace({
        name: deriveWorkspaceNameFromPath(selectedPath),
        path: selectedPath,
        isDefault: false,
        isGit: false,
        autoWorkTree: false,
      });
    });
  };

  const handleOpenWorkspace = (workspace: WorkspaceEntry) => {
    if (!isTauri() || !workspace.path || activeOpenWorkspaceId) {
      return;
    }

    const appId = isWindows ? "explorer" : "finder";

    void (async () => {
      setActiveOpenWorkspaceId(workspace.id);

      try {
        if (isWindows || isMacOS) {
          await invoke("open_workspace_in_app", {
            targetPath: workspace.path,
            appId,
            appPath: null,
          });
        } else {
          await openPath(workspace.path);
        }

        setOpenError(null);
      } catch (error) {
        setOpenError(getInvokeErrorMessage(error, `Couldn't open ${workspace.name}`));
      } finally {
        setActiveOpenWorkspaceId(null);
      }
    })();
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
        {openError ? (
          <div className="border-b border-app-border bg-app-surface-muted px-4 py-2 text-[12px] text-red-500">
            {openError}
          </div>
        ) : null}

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
                    label={activeOpenWorkspaceId === workspace.id ? "Opening folder" : openWorkspaceLabel}
                    disabled={activeOpenWorkspaceId === workspace.id}
                    onClick={() => handleOpenWorkspace(workspace)}
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
  disabled,
  icon: Icon,
  label,
  onClick,
}: {
  active?: boolean;
  className?: string;
  disabled?: boolean;
  icon: typeof Star;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      className={cn(
        "flex size-7 items-center justify-center rounded-md transition-colors",
        disabled && "cursor-not-allowed opacity-50",
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

function SectionDivider() {
  return <Separator />;
}
