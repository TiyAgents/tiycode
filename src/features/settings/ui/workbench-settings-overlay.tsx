import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  ArrowDownToLine,
  ArrowLeft,
  ArrowUpFromLine,
  Brain,
  Blocks,
  Check,
  ChevronDown,
  CircleUserRound,
  Coins,
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
  MessageSquare,
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
  Wrench,
  X,
  Zap,
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
  AgentProfile,
  ApiProtocol,
  ApprovalPolicy,
  CommandEntry,
  CommandSettings,
  GeneralPreferences,
  NetworkAccessPolicy,
  PolicySettings,
  PromptResponseStyle,
  ProviderEntry,
  ProviderModelCapabilities,
  ProviderModel,
  SandboxPolicy,
  SettingsCategory,
  PatternEntry,
  WorkspaceEntry,
  WritableRootEntry,
} from "@/features/settings/model/use-workbench-settings";

type UserSession = {
  name: string;
  avatar: string;
  email: string;
};

type WorkbenchSettingsOverlayProps = {
  activeCategory: SettingsCategory;
  agentProfiles: Array<AgentProfile>;
  activeAgentProfileId: string;
  contentRef: RefObject<HTMLDivElement | null>;
  generalPreferences: GeneralPreferences;
  isCheckingUpdates: boolean;
  language: LanguagePreference;
  commands: CommandSettings;
  policy: PolicySettings;
  providers: Array<ProviderEntry>;
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
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
  onUpdateProvider: (id: string, patch: Partial<Omit<ProviderEntry, "id">>) => void;
  onUpdateWorkspace: (id: string, patch: Partial<Omit<WorkspaceEntry, "id">>) => void;
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
    description: "Identity, local session state, and desktop operator details.",
    icon: CircleUserRound,
  },
  {
    key: "general",
    title: "General",
    description: "Core app preferences for language and theme.",
    icon: Monitor,
  },
  {
    key: "providers",
    title: "Providers",
    description: "Configure AI model providers, API keys, and available models.",
    icon: Blocks,
  },
  {
    key: "commands",
    title: "Commands",
    description: "Slash commands for common workflows.",
    icon: Sparkles,
  },
  {
    key: "policy",
    title: "Policy",
    description: "Execution approval, tool access, and sandbox boundaries.",
    icon: ShieldCheck,
  },
  {
    key: "workspace",
    title: "Workspace",
    description: "Manage project workspaces. New conversations will use these directories instead of creating temporary ones.",
    icon: FolderOpen,
  },
  {
    key: "about",
    title: "About",
    description: "Runtime platform, app version, and update checks.",
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
  { value: "balanced", label: "Balanced", description: "Clear by default, detailed when needed." },
  { value: "concise", label: "Concise", description: "Short, direct, and low-friction." },
  { value: "guide", label: "Guided", description: "More explanation around tradeoffs and choices." },
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

export function WorkbenchSettingsOverlay({
  activeCategory,
  agentProfiles,
  activeAgentProfileId,
  contentRef,
  generalPreferences,
  isCheckingUpdates,
  language,
  commands,
  policy,
  providers,
  selectedLanguageLabel,
  selectedThemeSummary,
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
  onUpdateProvider,
  onUpdateWorkspace,
  onUpdateWritableRoot,
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
                    description={activeMeta.description}
                    isCheckingUpdates={isCheckingUpdates}
                    runtime={systemMetadata}
                    selectedLanguageLabel={selectedLanguageLabel}
                    selectedThemeSummary={selectedThemeSummary}
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

type UsagePeriod = "this-month" | "last-month" | "this-week" | "all-time";

const USAGE_PERIOD_OPTIONS: ReadonlyArray<{ label: string; value: UsagePeriod }> = [
  { label: "This Month", value: "this-month" },
  { label: "Last Month", value: "last-month" },
  { label: "This Week", value: "this-week" },
  { label: "All Time", value: "all-time" },
];

type UsageStats = {
  totalCost: number;
  messages: number;
  inputTokens: number;
  outputTokens: number;
  cacheRead: number;
  cacheWrite: number;
};

function formatTokenCount(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${(count / 1_000).toFixed(1)}K`;
  return count.toString();
}

function UsageStatCard({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Coins;
  label: string;
  value: string;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-app-border bg-app-surface px-4 py-3">
      <div className="flex items-center gap-1.5 text-app-subtle">
        <Icon className="size-3.5" />
        <span className="text-[11px] font-medium">{label}</span>
      </div>
      <span className="text-[22px] font-semibold leading-tight text-app-foreground">{value}</span>
    </div>
  );
}

function formatHeatmapDate(date: Date): string {
  const year = date.getFullYear();
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function parseHeatmapDate(date: string): Date {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function hashHeatmapDate(date: string): number {
  return date.split("").reduce((acc, char) => acc + char.charCodeAt(0), 0);
}

function getHeatmapCount(date: Date, today: Date): number {
  const dayMs = 1000 * 60 * 60 * 24;
  const daysAgo = Math.floor((today.getTime() - date.getTime()) / dayMs);
  const dateKey = formatHeatmapDate(date);
  const hash = hashHeatmapDate(dateKey);
  const isWeekend = date.getDay() === 0 || date.getDay() === 6;
  const isRecentWindow = daysAgo <= 60;

  if (isRecentWindow) {
    const boosted = 2 + (hash % 3);
    return Math.max(1, boosted - (isWeekend ? 1 : 0));
  }

  if (daysAgo <= 120) {
    return hash % 5 === 0 ? 0 : 1 + (hash % 3);
  }

  if (daysAgo <= 240) {
    return hash % 4 === 0 ? 0 : 1 + (hash % 2);
  }

  return hash % 6 === 0 ? 1 : 0;
}

function generateHeatmapData(): Array<{ date: string; count: number }> {
  const data: Array<{ date: string; count: number }> = [];
  const today = new Date();
  today.setHours(0, 0, 0, 0);

  const startDate = new Date(today);
  startDate.setFullYear(startDate.getFullYear() - 1);
  startDate.setDate(startDate.getDate() - startDate.getDay());

  for (const d = new Date(startDate); d <= today; d.setDate(d.getDate() + 1)) {
    const currentDate = new Date(d);
    currentDate.setHours(0, 0, 0, 0);
    data.push({
      date: formatHeatmapDate(currentDate),
      count: getHeatmapCount(currentDate, today),
    });
  }

  return data;
}

const HEATMAP_EMPTY_COLOR = "bg-emerald-100/70 dark:bg-emerald-950/55";

const HEATMAP_COLORS = [
  HEATMAP_EMPTY_COLOR,
  "bg-emerald-200 dark:bg-emerald-900",
  "bg-emerald-300 dark:bg-emerald-700",
  "bg-emerald-500 dark:bg-emerald-500",
  "bg-emerald-700 dark:bg-emerald-300",
] as const;

function ActivityHeatmap() {
  const heatmapData = useMemo(() => generateHeatmapData(), []);

  const weeks = useMemo(() => {
    const result: Array<Array<{ date: string; count: number } | null>> = [];
    let currentWeek: Array<{ date: string; count: number } | null> = [];

    if (heatmapData.length > 0) {
      const firstDay = parseHeatmapDate(heatmapData[0].date).getDay();
      for (let i = 0; i < firstDay; i++) {
        currentWeek.push(null);
      }
    }

    for (const entry of heatmapData) {
      currentWeek.push(entry);
      if (currentWeek.length === 7) {
        result.push(currentWeek);
        currentWeek = [];
      }
    }
    if (currentWeek.length > 0) {
      while (currentWeek.length < 7) currentWeek.push(null);
      result.push(currentWeek);
    }
    return result;
  }, [heatmapData]);
  const columnCount = Math.max(weeks.length, 1);
  const minGridWidth = Math.max(columnCount * 12, 720);

  const months = useMemo(() => {
    const labels: Array<{ label: string; col: number }> = [];
    let lastMonth = -1;
    const monthNames = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

    for (let w = 0; w < weeks.length; w++) {
      const firstEntry = weeks[w].find((d) => d !== null);
      if (firstEntry) {
        const month = parseHeatmapDate(firstEntry.date).getMonth();
        if (month !== lastMonth) {
          labels.push({ label: monthNames[month], col: w });
          lastMonth = month;
        }
      }
    }
    return labels;
  }, [weeks]);

  return (
    <div className="overflow-hidden rounded-2xl border border-app-border bg-app-surface">
      <div className="px-4 py-3">
        <p className="text-[13px] font-medium text-app-foreground">Activity Heatmap</p>
        <p className="mt-0.5 text-[12px] text-app-muted">Your chat activity over the past year</p>
      </div>
      <div className="overflow-x-auto px-4 pb-4">
        <div className="w-full" style={{ minWidth: `${minGridWidth}px` }}>
          <div className="relative mb-2 h-4 w-full">
            {months.map((m) => (
              <span
                key={`${m.label}-${m.col}`}
                className="absolute text-[10px] text-app-subtle"
                style={{
                  left: `${(m.col / columnCount) * 100}%`,
                }}
              >
                {m.label}
              </span>
            ))}
          </div>
          <div
            className="grid w-full gap-[4px]"
            style={{
              gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
            }}
          >
            {weeks.map((week, wi) => (
              <div key={wi} className="grid min-w-0 grid-rows-7 gap-[4px]">
                {week.map((day, di) => (
                  <div
                    key={day?.date ?? `empty-${wi}-${di}`}
                    className={cn(
                      "aspect-square w-full rounded-[3px]",
                      day === null ? HEATMAP_EMPTY_COLOR : HEATMAP_COLORS[Math.min(day.count, 4)],
                    )}
                    title={day ? `${day.date}: ${day.count} messages` : undefined}
                  />
                ))}
              </div>
            ))}
          </div>
          <div className="mt-2 flex items-center justify-end gap-1">
            <span className="mr-1 text-[10px] text-app-subtle">Less</span>
            {HEATMAP_COLORS.map((color, i) => (
              <div key={i} className={cn("size-[12px] rounded-[3px]", color)} />
            ))}
            <span className="ml-1 text-[10px] text-app-subtle">More</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function UsagePeriodSelect({
  value,
  onChange,
}: {
  value: UsagePeriod;
  onChange: (value: UsagePeriod) => void;
}) {
  const [open, setOpen] = useState(false);
  const selectedLabel = USAGE_PERIOD_OPTIONS.find((o) => o.value === value)?.label ?? "This Month";

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1.5 rounded-lg border border-app-border bg-app-surface px-3 py-1.5 text-[12px] font-medium text-app-foreground shadow-none transition-colors hover:bg-app-surface-hover"
      >
        {selectedLabel}
        <ChevronDown className="size-3.5 text-app-subtle" />
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute right-0 top-full z-50 mt-1 min-w-[140px] overflow-hidden rounded-lg border border-app-border bg-app-surface shadow-lg">
            {USAGE_PERIOD_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
                className={cn(
                  "flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] transition-colors hover:bg-app-surface-hover",
                  option.value === value ? "font-medium text-app-foreground" : "text-app-muted",
                )}
              >
                {option.value === value && <Check className="size-3" />}
                <span className={option.value !== value ? "pl-5" : ""}>{option.label}</span>
              </button>
            ))}
          </div>
        </>
      )}
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
  const [usagePeriod, setUsagePeriod] = useState<UsagePeriod>("this-month");

  // Placeholder usage data — replace with real data source
  const usageStats: UsageStats = useMemo(
    () => ({
      totalCost: 0.0,
      messages: 3,
      inputTokens: 30000,
      outputTokens: 434,
      cacheRead: 22400,
      cacheWrite: 0,
    }),
    [],
  );

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

      <section>
        <div className="mb-2 flex items-center justify-between px-1">
          <h2 className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">Usage</h2>
          <UsagePeriodSelect value={usagePeriod} onChange={setUsagePeriod} />
        </div>
        <div className="grid grid-cols-3 gap-3">
          <UsageStatCard icon={Coins} label="Total Cost" value={`$${usageStats.totalCost.toFixed(4)}`} />
          <UsageStatCard icon={ArrowDownToLine} label="Input Tokens" value={formatTokenCount(usageStats.inputTokens)} />
          <UsageStatCard icon={ArrowUpFromLine} label="Output Tokens" value={formatTokenCount(usageStats.outputTokens)} />
          <UsageStatCard icon={MessageSquare} label="Messages" value={usageStats.messages.toString()} />
          <UsageStatCard icon={Database} label="Cache Read" value={formatTokenCount(usageStats.cacheRead)} />
          <UsageStatCard icon={Database} label="Cache Write" value={formatTokenCount(usageStats.cacheWrite)} />
        </div>
      </section>

      <ActivityHeatmap />
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
        <span className="max-w-[120px] truncate">{activeProfile.name}</span>
        <ChevronDown className="size-3 text-app-subtle" />
      </button>
      {isOpen
        ? createPortal(
            <div
              ref={dropdownRef}
              style={dropdownStyle}
              className="z-50 overflow-y-auto rounded-xl border border-app-border bg-app-surface p-1 shadow-lg"
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
    const models: Array<{ modelId: string; displayName: string; providerName: string }> = [];
    for (const provider of providers) {
      if (!provider.enabled) continue;
      for (const model of provider.models) {
        if (!model.enabled) continue;
        models.push({
          modelId: `${provider.name}/${model.modelId}`,
          displayName: model.displayName || model.modelId,
          providerName: provider.name,
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
      responseStyle: "balanced",
      responseLanguage: "English",
      primaryModel: "",
      assistantModel: "",
      liteModel: "",
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
        title="Agent Defaults"
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
              Add
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
        <ModelSelectRow
          label="Primary model"
          description="Handles main tasks including planning, building, and reasoning."
          value={activeProfile.primaryModel}
          availableModels={availableModels}
          onValueChange={(value) => onUpdateAgentProfile(activeProfile.id, { primaryModel: value })}
        />
        <SectionDivider />
        <ModelSelectRow
          label="Assistant model"
          description="Supports the primary model with sub-agent tasks and tool calls."
          value={activeProfile.assistantModel}
          availableModels={availableModels}
          onValueChange={(value) => onUpdateAgentProfile(activeProfile.id, { assistantModel: value })}
        />
        <SectionDivider />
        <ModelSelectRow
          label="Lite model"
          description="Lightweight model for title generation and quick summaries."
          value={activeProfile.liteModel}
          availableModels={availableModels}
          onValueChange={(value) => onUpdateAgentProfile(activeProfile.id, { liteModel: value })}
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
      </SettingsSection>
    </div>
  );
}

function ModelSelectRow({
  availableModels,
  description,
  label,
  value,
  onValueChange,
}: {
  availableModels: Array<{ modelId: string; displayName: string; providerName: string }>;
  description: string;
  label: string;
  value: string;
  onValueChange: (value: string) => void;
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

  const selectedModel = availableModels.find((m) => m.modelId === value);
  const displayValue = selectedModel
    ? `${selectedModel.displayName}`
    : value
      ? value
      : "Not set";

  const grouped = useMemo(() => {
    const map = new Map<string, Array<{ modelId: string; displayName: string }>>();
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
            !selectedModel && !value && "text-app-muted",
          )}
          onClick={() => setIsOpen(!isOpen)}
        >
          <span className="truncate">{displayValue}</span>
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
                    !value && "text-app-accent",
                  )}
                  onClick={() => { onValueChange(""); setIsOpen(false); }}
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
                        key={model.modelId}
                        type="button"
                        className={cn(
                          "flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] transition-colors hover:bg-app-surface-hover",
                          value === model.modelId && "text-app-accent",
                        )}
                        onClick={() => { onValueChange(model.modelId); setIsOpen(false); }}
                      >
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
  description,
  isCheckingUpdates,
  runtime,
  selectedLanguageLabel,
  selectedThemeSummary,
  updateStatus,
  onCheckUpdates,
}: {
  description: string;
  isCheckingUpdates: boolean;
  runtime: SystemMetadata | null;
  selectedLanguageLabel: string;
  selectedThemeSummary: string;
  updateStatus: string | null;
  onCheckUpdates: () => void;
}) {
  return (
    <div className="flex flex-col gap-6">
      <PageHeading title="About" description={description} />

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
    const newId = crypto.randomUUID();
    onAddCommand({
      name: "",
      path: "",
      argumentHint: "",
      description: "",
    });
    // find the newly added command and set editing — we use a timeout so state has updated
    setTimeout(() => {
      setEditingId(newId);
    }, 0);
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
              placeholder="commit"
              className="text-[13px]"
            />
            <p className="mt-1 text-[11px] text-app-subtle">Command path: {command.name ? `/prompts:${command.name}` : "/prompts:..."}</p>
          </div>
          <div className="mt-3">
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-app-subtle">Command prompt</label>
            <Textarea
              value={command.description}
              onChange={(event) => onUpdate({ description: event.target.value })}
              placeholder="Describe what this command does..."
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
      <PageHeading title="Policy" description={description} />

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

const API_PROTOCOL_OPTIONS: ReadonlyArray<{ label: string; value: ApiProtocol }> = [
  { value: "chat-completions", label: "Chat Completions (/chat/completions)" },
  { value: "responses", label: "Responses (/responses)" },
  { value: "anthropic", label: "Anthropic (/messages)" },
  { value: "gemini", label: "Gemini (/generateContent)" },
  { value: "ollama", label: "Ollama (/api/chat)" },
];

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
  const [showAdvancedSettings, setShowAdvancedSettings] = useState(false);
  const [customHeadersInput, setCustomHeadersInput] = useState("{}");
  const [customHeadersError, setCustomHeadersError] = useState<string | null>(null);
  const [expandedModelId, setExpandedModelId] = useState<string | null>(null);
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

  useEffect(() => {
    if (!selectedProvider) {
      setShowAdvancedSettings(false);
      setCustomHeadersInput("{}");
      setCustomHeadersError(null);
      return;
    }

    const hasCustomHeaders = Object.keys(selectedProvider.customHeaders).length > 0;
    setShowAdvancedSettings(hasCustomHeaders);
    setCustomHeadersInput(formatCustomHeaders(selectedProvider.customHeaders));
    setCustomHeadersError(null);
  }, [selectedProvider?.id]);

  useEffect(() => {
    if (!selectedProvider?.models.some((model) => model.id === expandedModelId)) {
      setExpandedModelId(null);
    }
  }, [expandedModelId, selectedProvider]);

  const handleAddCustomProvider = () => {
    const newProvider: Omit<ProviderEntry, "id"> = {
      name: "Custom Provider",
      baseUrl: "https://api.example.com/v1",
      apiKey: "",
      apiProtocol: "chat-completions",
      customHeaders: {},
      enabled: false,
      isCustom: true,
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
      displayName: newModelDisplayName.trim() || newModelId.trim(),
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

                <ProviderField
                  label="API Protocol"
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
                        <ProviderModelRow
                          key={model.id}
                          model={model}
                          isExpanded={expandedModelId === model.id}
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
    </div>
  );
}

function ProviderModelRow({
  isExpanded,
  model,
  onRemove,
  onToggleEnabled,
  onToggleExpanded,
  onUpdate,
}: {
  isExpanded: boolean;
  model: ProviderModel;
  onRemove: () => void;
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

      {isExpanded ? (
        <div className="border-t border-app-border bg-app-surface px-4 py-4">
          <div className="mb-3">
            <h5 className="text-[16px] font-semibold text-app-foreground">Model Capabilities</h5>
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
