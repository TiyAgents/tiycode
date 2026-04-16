import { type ReactNode, type RefObject, useCallback, useEffect, useMemo, useState } from "react";
import { Streamdown } from "streamdown";
import { cjk } from "@streamdown/cjk";
import { code } from "@streamdown/code";
import { math } from "@streamdown/math";
import { mermaid } from "@streamdown/mermaid";
import { useT, type TranslationKey } from "@/i18n";
import {
  ArrowLeft,
  BookCopy,
  Boxes,
  CirclePlus,
  CircleX,
  Eye,
  PackageOpen,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { getInvokeErrorMessage } from "@/shared/lib/invoke-error";
import { streamdownLinkSafety } from "@/shared/lib/streamdown-link-safety";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/shared/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { LocalLlmIcon } from "@/shared/ui/local-llm-icon";
import { ScrollArea } from "@/shared/ui/scroll-area";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/shared/ui/select";
import { Switch } from "@/shared/ui/switch";
import { Spinner } from "@/shared/ui/spinner";
import { Textarea } from "@/shared/ui/textarea";
import type {
  ConfigDiagnostic,
  ExtensionDetail,
  ExtensionSummary,
  MarketplaceItem,
  MarketplaceRemoveSourcePlan,
  MarketplaceSource,
  MarketplaceSourceInput,
  McpServerConfigInput,
  McpServerState,
  PluginHookGroup,
  SkillPreview,
  SkillRecord,
} from "@/shared/types/extensions";

type ExtensionTab = "plugins" | "mcps" | "skills";
type PluginSourceFilter = "all" | string;
type SkillFrontmatterEntry = {
  key: string;
  value: string;
};

type PluginCollectionItem = {
  key: string;
  name: string;
  description: string;
  version: string;
  tags: string[];
  sourceLabel: string;
  sourceFilter: PluginSourceFilter;
  installed: boolean;
  enabled: boolean;
  installState: string;
  health: ExtensionSummary["health"];
  extensionId: string | null;
  extensionSummary: ExtensionSummary | null;
  marketplaceItem: MarketplaceItem | null;
  commands: Array<{ description?: string | null; name: string }>;
  mcpServers: string[];
  skillNames: string[];
  hooks: PluginHookGroup[];
};

function comparePluginCollectionItems(left: PluginCollectionItem, right: PluginCollectionItem) {
  if (left.enabled !== right.enabled) {
    return left.enabled ? -1 : 1;
  }
  if (left.installed !== right.installed) {
    return left.installed ? -1 : 1;
  }
  return left.name.localeCompare(right.name, undefined, { sensitivity: "base" });
}

function PluginIcon() {
  return (
    <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl border border-app-border bg-app-canvas shadow-[inset_0_1px_0_rgba(255,255,255,0.45)]">
      <PackageOpen className="size-[18px] shrink-0 stroke-[1.9]" />
    </div>
  );
}

function SkillIcon() {
  return (
    <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl border border-app-border bg-app-canvas shadow-[inset_0_1px_0_rgba(255,255,255,0.45)]">
      <BookCopy className="size-[18px] shrink-0 stroke-[1.9]" />
    </div>
  );
}

type ExtensionsCenterOverlayProps = {
  contentRef: RefObject<HTMLDivElement | null>;
  error: string | null;
  isLoading: boolean;
  extensions: ExtensionSummary[];
  mcpServers: McpServerState[];
  skills: SkillRecord[];
  detailById: Record<string, ExtensionDetail>;
  skillPreviewById: Record<string, SkillPreview>;
  configDiagnostics: ConfigDiagnostic[];
  marketplaceSources: MarketplaceSource[];
  marketplaceItems: MarketplaceItem[];
  onClose: () => void;
  onRefresh: () => void;
  onLoadDetail: (id: string) => Promise<ExtensionDetail>;
  onLoadSkillPreview: (id: string) => Promise<SkillPreview>;
  onEnableExtension: (id: string) => Promise<void>;
  onDisableExtension: (id: string) => Promise<void>;
  onUninstallExtension: (id: string) => Promise<void>;
  onAddMcpServer: (input: McpServerConfigInput) => Promise<void>;
  onUpdateMcpServer: (id: string, input: McpServerConfigInput) => Promise<void>;
  onRemoveMcpServer: (id: string) => Promise<void>;
  onRestartMcpServer: (id: string) => Promise<void>;
  onRescanSkills: () => Promise<void>;
  onEnableSkill: (id: string) => Promise<void>;
  onDisableSkill: (id: string) => Promise<void>;
  onAddMarketplaceSource: (input: MarketplaceSourceInput) => Promise<void>;
  onGetMarketplaceSourceRemovePlan: (id: string) => Promise<MarketplaceRemoveSourcePlan>;
  onRemoveMarketplaceSource: (id: string) => Promise<void>;
  onRefreshMarketplaceSource: (id: string) => Promise<void>;
  onInstallMarketplaceItem: (id: string) => Promise<void>;
};

type McpFormState = McpServerConfigInput;

type TFunc = (key: TranslationKey, params?: Record<string, string | number>) => string;

function getTabMeta(t: TFunc): Record<ExtensionTab, { description: string; label: string }> {
  return {
    plugins: {
      label: t("extensions.tab.plugins"),
      description: t("extensions.tab.pluginsDesc"),
    },
    mcps: {
      label: t("extensions.tab.mcpServers"),
      description: t("extensions.tab.mcpServersDesc"),
    },
    skills: {
      label: t("extensions.tab.skills"),
      description: t("extensions.tab.skillsDesc"),
    },
  };
}

function createDefaultMcpFormState(): McpFormState {
  return {
    id: "",
    label: "",
    transport: "stdio",
    enabled: true,
    autoStart: true,
    command: "",
    args: [],
    env: {},
    cwd: "",
    url: "",
    headers: {},
    timeoutMs: 30_000,
  };
}

function formatHeaderMap(headers: Record<string, string> | undefined) {
  return Object.entries(headers ?? {})
    .map(([key, value]) => `${key}: ${value}`)
    .join("\n");
}

function parseHeaderMapInput(value: string) {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .reduce<Record<string, string>>((headers, line) => {
      const [rawKey, ...rawValue] = line.split(":");
      const key = rawKey?.trim();
      if (!key) {
        return headers;
      }
      headers[key] = rawValue.join(":").trim();
      return headers;
    }, {});
}

function getTabCount(tab: ExtensionTab, props: ExtensionsCenterOverlayProps) {
  switch (tab) {
    case "plugins": {
      const marketplacePaths = new Set(props.marketplaceItems.map((item) => item.path).filter(Boolean));
      const localOnlyPlugins = props.extensions.filter(
        (item) =>
          item.kind === "plugin"
          && (item.source.type !== "local-dir" || !marketplacePaths.has(item.source.path)),
      );
      return props.marketplaceItems.length + localOnlyPlugins.length;
    }
    case "mcps":
      return props.mcpServers.length;
    case "skills":
      return props.skills.length;
  }
}

function normalizePluginNameList(items: string[]) {
  return [...items].sort((left, right) => left.localeCompare(right, undefined, { sensitivity: "base" }));
}

function formatSource(summary: ExtensionSummary, t?: TFunc) {
  switch (summary.source.type) {
    case "builtin":
      return t ? t("extensions.builtinBadge") : "Builtin";
    case "local-dir":
      return summary.source.path;
    case "marketplace":
      return `Marketplace · ${summary.source.listingId}`;
  }
}

function getHealthBadgeClass(health: ExtensionSummary["health"]) {
  switch (health) {
    case "healthy":
      return "bg-app-success/12 text-app-success";
    case "degraded":
      return "bg-app-warning/12 text-app-warning";
    case "error":
      return "bg-app-danger/12 text-app-danger";
    default:
      return "bg-app-surface-muted/80 text-app-subtle";
  }
}

function getStatusBadgeClass(status: string) {
  if (status === "connected" || status === "ready") {
    return "bg-app-success/12 text-app-success";
  }
  if (status === "degraded" || status === "config_error") {
    return "bg-app-warning/12 text-app-warning";
  }
  if (status === "error") {
    return "bg-app-danger/12 text-app-danger";
  }
  return "bg-app-surface-muted/80 text-app-subtle";
}

function getStatusBadgeLabel(status: string) {
  return status.replace(/_/g, " ");
}

function getSkillStatusBadgeClass(enabled: boolean) {
  return enabled ? "bg-app-success/12 text-app-success" : "bg-app-surface-muted/80 text-app-subtle";
}

function getSkillStatusLabel(enabled: boolean, t?: TFunc) {
  if (t) {
    return enabled ? t("extensions.enabledBadge") : t("extensions.disabledBadge");
  }
  return enabled ? "enabled" : "disabled";
}

const streamdownPlugins = { cjk, code, math, mermaid };
const streamdownControls = { code: { download: false } };

function normalizeFrontmatterValue(value: string) {
  const trimmed = value.trim();
  if (
    (trimmed.startsWith("\"") && trimmed.endsWith("\""))
    || (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1).trim();
  }
  return trimmed;
}

function countLeadingSpaces(value: string) {
  const match = value.match(/^ */);
  return match ? match[0].length : 0;
}

function formatYamlBlockScalar(lines: string[], mode: "folded" | "literal") {
  if (mode === "literal") {
    return lines.join("\n").trim();
  }

  const paragraphs: string[] = [];
  let current: string[] = [];

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (current.length > 0) {
        paragraphs.push(current.join(" "));
        current = [];
      }
      continue;
    }
    current.push(trimmed);
  }

  if (current.length > 0) {
    paragraphs.push(current.join(" "));
  }

  return paragraphs.join("\n\n").trim();
}

function parseSkillFrontmatter(content: string | null) {
  if (!content) {
    return { body: "", entries: [] as SkillFrontmatterEntry[] };
  }

  const normalized = content.replace(/\r\n/g, "\n");
  if (!normalized.startsWith("---\n")) {
    return { body: normalized.trim(), entries: [] as SkillFrontmatterEntry[] };
  }

  const closingIndex = normalized.indexOf("\n---\n", 4);
  if (closingIndex === -1) {
    return { body: normalized.trim(), entries: [] as SkillFrontmatterEntry[] };
  }

  const frontmatter = normalized.slice(4, closingIndex);
  const body = normalized.slice(closingIndex + 5).trim();
  const lines = frontmatter.split("\n");
  const entries: SkillFrontmatterEntry[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = line.trim();

    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }

    const separatorIndex = line.indexOf(":");
    if (separatorIndex === -1) {
      continue;
    }

    const key = line.slice(0, separatorIndex).trim();
    const rawValue = line.slice(separatorIndex + 1);

    if (!key) {
      continue;
    }

    const marker = rawValue.trim();
    if (marker === ">" || marker === ">-" || marker === ">+" || marker === "|" || marker === "|-" || marker === "|+") {
      const mode = marker.startsWith(">") ? "folded" : "literal";
      const blockLines: string[] = [];
      let blockIndent: number | null = null;

      while (index + 1 < lines.length) {
        const nextLine = lines[index + 1];
        const nextTrimmed = nextLine.trim();

        if (!nextTrimmed) {
          if (blockIndent !== null) {
            blockLines.push("");
            index += 1;
            continue;
          }
          index += 1;
          continue;
        }

        const nextIndent = countLeadingSpaces(nextLine);
        if (nextIndent === 0) {
          break;
        }

        if (blockIndent === null) {
          blockIndent = nextIndent;
        }

        if (nextIndent < blockIndent) {
          break;
        }

        blockLines.push(nextLine.slice(blockIndent));
        index += 1;
      }

      const value = formatYamlBlockScalar(blockLines, mode);
      if (value) {
        entries.push({ key, value });
      }
      continue;
    }

    const value = normalizeFrontmatterValue(rawValue);
    if (!value) {
      continue;
    }

    entries.push({ key, value });
  }

  return { body, entries };
}

function getMcpToolParameters(inputSchema: unknown) {
  if (!inputSchema || typeof inputSchema !== "object" || Array.isArray(inputSchema)) {
    return [];
  }

  const schema = inputSchema as {
    properties?: Record<string, { description?: string; type?: string | string[] }>;
    required?: string[];
  };
  const properties = schema.properties ?? {};
  const required = new Set(Array.isArray(schema.required) ? schema.required : []);

  return Object.entries(properties).map(([name, definition]) => ({
    description: definition?.description ?? null,
    name,
    required: required.has(name),
    type: Array.isArray(definition?.type) ? definition.type.join(" | ") : (definition?.type ?? null),
  }));
}

function getMcpServerDescription(server: McpServerState) {
  if (server.lastError) {
    return server.lastError;
  }
  if (server.config.transport === "streamable-http") {
    return server.config.url ?? "Remote MCP server";
  }
  const command = server.config.command?.trim() ?? "";
  const args = server.config.args.join(" ").trim();
  return [command, args].filter(Boolean).join(" ").trim() || "Local stdio MCP server";
}

function getPluginStateLabel(item: PluginCollectionItem, t?: TFunc) {
  if (!item.installed) {
    return null;
  }
  if (item.installState === "enabled") {
    return t ? t("extensions.enabledBadge") : "enabled";
  }
  if (item.installState === "error") {
    return "error";
  }
  if (item.installState === "disabled") {
    return t ? t("extensions.disabledBadge") : "disabled";
  }
  return t ? t("extensions.installedBadge") : "installed";
}

const DETAIL_WRAP_BADGE_CLASS = "h-auto max-w-full whitespace-normal break-all py-1 text-left leading-5";

function filterByQuery<T>(items: T[], query: string, toHaystack: (item: T) => string) {
  const normalized = query.trim().toLowerCase();
  if (!normalized) {
    return items;
  }
  return items.filter((item) => toHaystack(item).toLowerCase().includes(normalized));
}

export function ExtensionsCenterOverlay(props: ExtensionsCenterOverlayProps) {
  const t = useT();
  const TAB_META = getTabMeta(t);
  const [activeTab, setActiveTab] = useState<ExtensionTab>("plugins");
  const [queryByTab, setQueryByTab] = useState<Record<ExtensionTab, string>>({
    plugins: "",
    mcps: "",
    skills: "",
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pluginSourceFilter, setPluginSourceFilter] = useState<PluginSourceFilter>("all");
  const [mcpDialogOpen, setMcpDialogOpen] = useState(false);
  const [mcpDialogMode, setMcpDialogMode] = useState<"create" | "edit">("create");
  const [mcpForm, setMcpForm] = useState<McpFormState>(createDefaultMcpFormState);
  const [mcpHeadersText, setMcpHeadersText] = useState("");
  const [marketSourceDialogOpen, setMarketSourceDialogOpen] = useState(false);
  const [marketSourceForm, setMarketSourceForm] = useState<MarketplaceSourceInput>({ name: "", url: "" });
  const [marketSourceError, setMarketSourceError] = useState<string | null>(null);
  const [isMarketSourceSubmitting, setMarketSourceSubmitting] = useState(false);
  const [removeSourceDialogOpen, setRemoveSourceDialogOpen] = useState(false);
  const [removeSourcePlan, setRemoveSourcePlan] = useState<MarketplaceRemoveSourcePlan | null>(null);
  const [removeSourceTargetId, setRemoveSourceTargetId] = useState<string | null>(null);
  const [removeSourceError, setRemoveSourceError] = useState<string | null>(null);
  const [isRemoveSourcePreviewLoading, setRemoveSourcePreviewLoading] = useState(false);
  const [isRemoveSourceSubmitting, setRemoveSourceSubmitting] = useState(false);
  const [detailLoadingId, setDetailLoadingId] = useState<string | null>(null);
  const [isActionPending, setActionPending] = useState(false);
  const [pendingMcpIds, setPendingMcpIds] = useState<Set<string>>(new Set());
  const [skillPreviewDialogOpen, setSkillPreviewDialogOpen] = useState(false);
  const activeQuery = queryByTab[activeTab];

  const allPluginItems = useMemo(() => {
    const pluginExtensions = props.extensions.filter((item) => item.kind === "plugin");
    const extensionByPath = new Map<string, ExtensionSummary>();
    for (const item of pluginExtensions) {
      if (item.source.type === "local-dir") {
        extensionByPath.set(item.source.path, item);
      }
    }

    const matchedExtensionIds = new Set<string>();
    const combined: PluginCollectionItem[] = props.marketplaceItems.map((item) => {
      const matchedExtension = item.path ? (extensionByPath.get(item.path) ?? null) : null;
      if (matchedExtension) {
        matchedExtensionIds.add(matchedExtension.id);
      }
      const installed = item.installed || Boolean(matchedExtension && matchedExtension.installState !== "discovered");
      const enabled = matchedExtension
        ? matchedExtension.installState === "enabled"
        : item.enabled;

      return {
        key: `marketplace:${item.id}`,
        name: item.name,
        description: item.summary || item.description,
        version: item.version,
        tags: item.tags,
        sourceLabel: item.sourceName,
        sourceFilter: item.sourceId,
        installed,
        enabled,
        installState: installed ? (enabled ? "enabled" : "installed") : "available",
        health: matchedExtension?.health ?? (installed ? "healthy" : "unknown"),
        extensionId: matchedExtension?.id ?? null,
        extensionSummary: matchedExtension,
        marketplaceItem: item,
        commands: item.commandNames.map((name) => ({ description: null, name })),
        mcpServers: normalizePluginNameList(item.mcpServers),
        skillNames: normalizePluginNameList(item.skillNames),
        hooks: item.hooks,
      } satisfies PluginCollectionItem;
    });

    combined.push(
      ...pluginExtensions
        .filter((item) => !matchedExtensionIds.has(item.id))
        .map((item) => ({
          key: `extension:${item.id}`,
          name: item.name,
          description: item.description ?? "Local plugin package",
          version: item.version,
          tags: item.tags,
          sourceLabel: formatSource(item, t),
          sourceFilter: "local",
          installed: item.installState !== "discovered",
          enabled: item.installState === "enabled",
          installState: item.installState,
          health: item.health,
          extensionId: item.id,
          extensionSummary: item,
          marketplaceItem: null,
          commands: [],
          mcpServers: [],
          skillNames: [],
          hooks: [],
        })),
    );

    return combined.sort(comparePluginCollectionItems);
  }, [
    props.extensions,
    props.marketplaceItems,
    t,
  ]);
  const pluginItems = useMemo(() => {
    const sourceFiltered =
      pluginSourceFilter === "all"
        ? allPluginItems
        : allPluginItems.filter((item) => item.sourceFilter === pluginSourceFilter);
    return filterByQuery(
      sourceFiltered,
      activeTab === "plugins" ? activeQuery : "",
      (item) =>
        [
          item.name,
          item.description,
          item.sourceLabel,
          item.tags.join(" "),
          item.commands.map((command) => command.name).join(" "),
          item.mcpServers.join(" "),
          item.skillNames.join(" "),
          item.hooks.map((hook) => `${hook.event} ${hook.handlers.join(" ")}`).join(" "),
        ].join(" "),
    );
  }, [activeQuery, activeTab, allPluginItems, pluginSourceFilter]);
  const pluginSourceOptions = useMemo(() => {
    return [
      { count: allPluginItems.length, key: "all" as const, label: "All", source: null },
      ...props.marketplaceSources.map((source) => ({
        count: allPluginItems.filter((item) => item.sourceFilter === source.id).length,
        key: source.id,
        label: source.name,
        source,
      })),
    ];
  }, [allPluginItems, props.marketplaceSources]);
  const filteredMcpServers = useMemo(
    () =>
      filterByQuery(props.mcpServers, activeTab === "mcps" ? activeQuery : "", (item) =>
        [
          item.label,
          item.status,
          item.phase,
          item.tools.map((tool) => `${tool.name} ${tool.qualifiedName}`).join(" "),
        ].join(" "),
      ),
    [activeQuery, activeTab, props.mcpServers],
  );
  const filteredSkills = useMemo(
    () =>
      filterByQuery(props.skills, activeTab === "skills" ? activeQuery : "", (item) =>
        [item.name, item.description ?? "", item.tags.join(" "), item.triggers.join(" ")].join(" "),
      ),
    [activeQuery, activeTab, props.skills],
  );

  useEffect(() => {
    if (activeTab === "plugins") {
      setSelectedId(pluginItems[0]?.key ?? null);
      return;
    }
    if (activeTab === "mcps") {
      setSelectedId(filteredMcpServers[0]?.id ?? null);
      return;
    }
    if (activeTab === "skills") {
      setSelectedId(filteredSkills[0]?.id ?? null);
    }
  }, [activeTab, filteredMcpServers, filteredSkills, pluginItems]);

  useEffect(() => {
    if (activeTab === "plugins" && !selectedId && pluginItems[0]) {
      setSelectedId(pluginItems[0].key);
    }
    if (activeTab === "mcps" && !selectedId && filteredMcpServers[0]) {
      setSelectedId(filteredMcpServers[0].id);
    }
    if (activeTab === "skills" && !selectedId && filteredSkills[0]) {
      setSelectedId(filteredSkills[0].id);
    }
  }, [activeTab, filteredMcpServers, filteredSkills, pluginItems, selectedId]);

  useEffect(() => {
    if (!pluginSourceOptions.some((option) => option.key === pluginSourceFilter)) {
      setPluginSourceFilter("all");
    }
  }, [pluginSourceFilter, pluginSourceOptions]);

  useEffect(() => {
    if (!selectedId || (activeTab !== "plugins" && activeTab !== "mcps" && activeTab !== "skills")) {
      return;
    }
    const detailId =
      activeTab === "plugins"
        ? (pluginItems.find((item) => item.key === selectedId)?.extensionId ?? null)
        : selectedId;
    if (!detailId || props.detailById[detailId]) {
      return;
    }
    setDetailLoadingId(selectedId);
    void props
      .onLoadDetail(detailId)
      .finally(() => setDetailLoadingId((current) => (current === selectedId ? null : current)));
  }, [activeTab, pluginItems, props, selectedId]);

  useEffect(() => {
    if (activeTab !== "skills" || !selectedId) {
      return;
    }
    if (props.skillPreviewById[selectedId]) {
      return;
    }
    void props.onLoadSkillPreview(selectedId);
  }, [activeTab, props, selectedId]);

  useEffect(() => {
    setSkillPreviewDialogOpen(false);
  }, [activeTab, selectedId]);

  const selectedPluginItem =
    activeTab === "plugins"
      ? pluginItems.find((item) => item.key === selectedId) ?? null
      : null;
  const selectedPluginSource = selectedPluginItem?.marketplaceItem
    ? props.marketplaceSources.find((source) => source.id === selectedPluginItem.marketplaceItem?.sourceId) ?? null
    : null;
  const selectedDetail =
    activeTab === "plugins"
      ? (selectedPluginItem?.extensionId ? (props.detailById[selectedPluginItem.extensionId] ?? null) : null)
      : selectedId
        ? (props.detailById[selectedId] ?? null)
        : null;
  const selectedSkillContent = selectedDetail?.skill
    ? (props.skillPreviewById[selectedDetail.skill.id]?.content ?? selectedDetail.skill.contentPreview)
    : null;
  const selectedSkillPreview = useMemo(
    () => parseSkillFrontmatter(selectedSkillContent),
    [selectedSkillContent],
  );
  const activeMeta = TAB_META[activeTab];
  const sourceActionLockedId =
    isRemoveSourcePreviewLoading || isRemoveSourceSubmitting ? removeSourceTargetId : null;

  const runAction = async (action: () => Promise<void>) => {
    setActionPending(true);
    try {
      await action();
    } catch (error) {
      console.error("extensions-center action failed", error);
    } finally {
      setActionPending(false);
    }
  };

  const runMcpAction = useCallback(async (serverId: string, action: () => Promise<void>) => {
    setPendingMcpIds((prev) => new Set(prev).add(serverId));
    setActionPending(true);
    try {
      await action();
    } catch (error) {
      console.error("extensions-center mcp action failed", error);
    } finally {
      setPendingMcpIds((prev) => {
        const next = new Set(prev);
        next.delete(serverId);
        return next;
      });
      setActionPending(false);
    }
  }, []);

  const handleOpenCreateMcpDialog = () => {
    setMcpDialogMode("create");
    setMcpForm(createDefaultMcpFormState());
    setMcpHeadersText("");
    setMcpDialogOpen(true);
  };

  const handleOpenEditMcpDialog = (server: McpServerState) => {
    setMcpDialogMode("edit");
    setMcpForm({
      id: server.id,
      label: server.label,
      transport: server.config.transport as "stdio" | "streamable-http",
      enabled: server.config.enabled,
      autoStart: server.config.autoStart,
      command: server.config.command ?? "",
      args: server.config.args,
      env: server.config.env,
      cwd: server.config.cwd ?? "",
      url: server.config.url ?? "",
      headers: server.config.headers,
      timeoutMs: server.config.timeoutMs ?? 30_000,
    });
    setMcpHeadersText(formatHeaderMap(server.config.headers));
    setMcpDialogOpen(true);
  };

  const handleSubmitMcp = async () => {
    const headers = parseHeaderMapInput(mcpHeadersText);
    const payload: McpServerConfigInput = {
      ...mcpForm,
      command: mcpForm.command?.trim() || undefined,
      cwd: mcpForm.cwd?.trim() || undefined,
      url: mcpForm.url?.trim() || undefined,
      args: (mcpForm.args ?? []).filter(Boolean),
      headers: Object.keys(headers).length > 0 ? headers : undefined,
    };
    await runAction(() =>
      mcpDialogMode === "create"
        ? props.onAddMcpServer(payload)
        : props.onUpdateMcpServer(payload.id, payload),
    );
    setMcpDialogOpen(false);
  };

  const handleSubmitMarketplaceSource = async () => {
    setMarketSourceSubmitting(true);
    setMarketSourceError(null);
    try {
      await props.onAddMarketplaceSource(marketSourceForm);
      setMarketSourceDialogOpen(false);
      setMarketSourceForm({ name: "", url: "" });
    } catch (error) {
      setMarketSourceError(getInvokeErrorMessage(error, "Failed to add marketplace source"));
    } finally {
      setMarketSourceSubmitting(false);
    }
  };

  const handleOpenRemoveSourceDialog = async (source: MarketplaceSource) => {
    setRemoveSourceDialogOpen(true);
    setRemoveSourceTargetId(source.id);
    setRemoveSourcePlan(null);
    setRemoveSourceError(null);
    setRemoveSourcePreviewLoading(true);
    try {
      const plan = await props.onGetMarketplaceSourceRemovePlan(source.id);
      setRemoveSourcePlan(plan);
    } catch (error) {
      setRemoveSourceError(getInvokeErrorMessage(error, "Failed to load source removal details"));
    } finally {
      setRemoveSourcePreviewLoading(false);
    }
  };

  const handleConfirmRemoveSource = async () => {
    if (!removeSourceTargetId) {
      return;
    }
    setRemoveSourceSubmitting(true);
    setRemoveSourceError(null);
    try {
      await props.onRemoveMarketplaceSource(removeSourceTargetId);
      setRemoveSourceDialogOpen(false);
      setRemoveSourcePlan(null);
      setRemoveSourceTargetId(null);
    } catch (error) {
      setRemoveSourceError(getInvokeErrorMessage(error, "Failed to remove source"));
      try {
        const nextPlan = await props.onGetMarketplaceSourceRemovePlan(removeSourceTargetId);
        setRemoveSourcePlan(nextPlan);
      } catch {
        // Keep the latest visible error if preview refresh also fails.
      }
    } finally {
      setRemoveSourceSubmitting(false);
    }
  };

  return (
    <div className="fixed inset-x-0 bottom-0 top-9 z-[60] overflow-hidden bg-app-canvas text-app-foreground">
      <div className="flex h-full min-h-0 flex-col">
        <header className="shrink-0 border-b border-app-border bg-app-canvas/92 backdrop-blur-xl">
          <div className="mx-auto w-full max-w-7xl px-5 py-4">
            <button
              type="button"
              className="inline-flex items-center gap-2 text-[12px] text-app-muted transition-colors hover:text-app-foreground"
              onClick={props.onClose}
            >
              <ArrowLeft className="size-3.5" />
              <span>{t("extensions.backToApp")}</span>
            </button>

            <div className="mt-3 flex items-start gap-3">
              <div className="flex size-10 items-center justify-center rounded-2xl border border-app-border bg-app-surface/80 text-app-foreground">
                <Boxes className="size-4" />
              </div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-1.5">
                  <h1 className="text-[19px] font-semibold tracking-[-0.03em] text-app-foreground">{t("extensions.title")}</h1>
                  <span className="rounded-full border border-app-border bg-app-surface-muted/70 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
                    {activeMeta.label}
                  </span>
                </div>
                <p className="mt-1 text-[13px] leading-5 text-app-muted">{activeMeta.description}</p>
              </div>
            </div>

            <div className="mt-4 flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
              <div className="flex flex-wrap items-center gap-2">
                {(Object.keys(TAB_META) as ExtensionTab[]).map((tab) => {
                  const isActive = tab === activeTab;
                  return (
                    <button
                      key={tab}
                      type="button"
                      className={cn(
                        "inline-flex h-8 items-center gap-2 rounded-lg border px-3 text-[12px] font-medium transition-colors",
                        isActive
                          ? "border-app-border-strong bg-app-surface text-app-foreground"
                          : "border-app-border bg-transparent text-app-muted hover:bg-app-surface-hover hover:text-app-foreground",
                      )}
                      onClick={() => setActiveTab(tab)}
                    >
                      <span>{TAB_META[tab].label}</span>
                      <span className="rounded-full bg-app-surface-muted px-1.5 py-0.5 text-[10px] text-app-subtle">
                        {getTabCount(tab, props)}
                      </span>
                    </button>
                  );
                })}
              </div>

              <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
                <div className="relative min-w-0 flex-1 sm:w-[320px]">
                  <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-app-subtle" />
                  <Input
                    value={activeQuery}
                    onChange={(event) =>
                      setQueryByTab((current) => ({ ...current, [activeTab]: event.target.value }))
                    }
                    placeholder={t("extensions.searchPlaceholder", { label: activeMeta.label.toLowerCase() })}
                    className="h-9 rounded-xl border-app-border bg-app-surface-muted/80 pl-10 text-[13px]"
                  />
                </div>

                <Button
                  size="sm"
                  variant="outline"
                  className="h-8 shrink-0 rounded-lg bg-app-surface/70 px-3 text-[12px]"
                  onClick={() => void props.onRefresh()}
                  disabled={props.isLoading || isActionPending}
                >
                  <RefreshCw className={cn("size-4", props.isLoading && "animate-spin")} />
                  {t("extensions.refresh")}
                </Button>

                {activeTab === "plugins" ? (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-8 shrink-0 rounded-lg bg-app-surface/70 px-3 text-[12px]"
                      onClick={() => setMarketSourceDialogOpen(true)}
                      disabled={isActionPending}
                    >
                      <CirclePlus className="size-4" />
                      {t("extensions.addSource")}
                    </Button>
                  </>
                ) : null}

                {activeTab === "mcps" ? (
                  <Button
                    size="sm"
                    className="h-8 shrink-0 rounded-lg px-3 text-[12px]"
                    onClick={handleOpenCreateMcpDialog}
                    disabled={isActionPending}
                  >
                    <CirclePlus className="size-4" />
                    {t("extensions.addMcp")}
                  </Button>
                ) : null}

                {activeTab === "skills" ? (
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0 rounded-lg bg-app-surface/70 px-3 text-[12px]"
                    onClick={() => void runAction(props.onRescanSkills)}
                    disabled={isActionPending}
                  >
                    <RefreshCw className="size-4" />
                    {t("extensions.rescanSkills")}
                  </Button>
                ) : null}
              </div>
            </div>

            {props.error ? (
              <div className="mt-3 rounded-xl border border-app-danger/30 bg-app-danger/8 px-3 py-2 text-[12px] text-app-danger">
                {props.error}
              </div>
            ) : null}

            {props.configDiagnostics.length > 0 ? (
              <div className="mt-3 rounded-xl border border-app-warning/30 bg-app-warning/8 px-3 py-3 text-[12px] text-app-warning">
                <p className="font-medium text-app-foreground">{t("extensions.configIssuesDetected")}</p>
                <div className="mt-2 space-y-2 text-app-warning">
                  {props.configDiagnostics.map((diagnostic) => (
                    <div key={diagnostic.id} className="rounded-lg border border-app-warning/20 bg-app-canvas/40 px-3 py-2">
                      <p className="font-medium text-app-foreground">{diagnostic.summary}</p>
                      <p className="mt-1 break-all">{diagnostic.filePath}</p>
                      <p className="mt-1 text-app-subtle">{diagnostic.suggestion}</p>
                    </div>
                  ))}
                </div>
              </div>
            ) : null}
          </div>
        </header>

        <div ref={props.contentRef} className="relative min-h-0 flex-1">
          <div className="mx-auto grid h-full w-full max-w-7xl min-h-0 gap-5 px-5 py-5 lg:grid-cols-[minmax(0,1fr)_360px]">
            <ScrollArea className="min-h-0">
              <div className="space-y-4">
                {activeTab === "plugins" ? (
                  <div className="sticky top-0 z-10 -mx-1 border-b border-app-border/70 bg-[color:color-mix(in_srgb,var(--app-bg)_88%,transparent)] px-1 py-3 backdrop-blur">
                    <div className="flex flex-wrap items-center gap-2">
                      {pluginSourceOptions.map((option) => (
                        <div key={option.key} className="group relative">
                          <button
                            type="button"
                            className={cn(
                              "inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-[12px] transition-colors",
                              pluginSourceFilter === option.key
                                ? "border-app-border-strong bg-app-canvas text-app-foreground"
                                : "border-app-border bg-app-surface-muted/70 text-app-muted hover:text-app-foreground",
                            )}
                            onClick={() => setPluginSourceFilter(option.key)}
                          >
                            <span>{option.label}</span>
                            <span className="rounded-full bg-app-surface px-1.5 py-0.5 text-[10px] text-app-subtle">
                              {option.count}
                            </span>
                          </button>
                          {option.source && !option.source.builtin ? (
                            <button
                              type="button"
                              aria-label={`Remove ${option.source.name}`}
                              title={`Remove ${option.source.name}`}
                              disabled={sourceActionLockedId === option.source.id}
                              onClick={(event) => {
                                event.preventDefault();
                                event.stopPropagation();
                                void handleOpenRemoveSourceDialog(option.source);
                              }}
                              className={cn(
                                "absolute -right-1 -top-1 z-10 flex size-5 items-center justify-center rounded-full border border-app-border bg-app-canvas text-app-subtle opacity-0 shadow-sm transition hover:text-app-foreground group-hover:opacity-100 group-focus-within:opacity-100",
                                sourceActionLockedId === option.source.id && "pointer-events-none opacity-40",
                              )}
                            >
                              <X className="size-3" />
                            </button>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  </div>
                ) : null}

                <div className="grid gap-4 px-1 pb-1 md:grid-cols-2">
                {activeTab === "plugins"
                  ? pluginItems.map((item) => (
                      <Card
                        key={item.key}
                        className={cn(
                          "cursor-pointer border transition-colors hover:border-app-border-strong",
                          selectedId === item.key && "border-app-border-strong bg-app-surface-hover",
                        )}
                        onClick={() => setSelectedId(item.key)}
                      >
                        <CardHeader className="gap-3 pb-3">
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex min-w-0 items-start gap-3">
                              <PluginIcon />
                              <div className="min-w-0">
                                <CardTitle className="truncate text-[15px]">{item.name}</CardTitle>
                                <CardDescription
                                  className="mt-1 line-clamp-2 min-h-[2.75rem] text-[13px] leading-5"
                                  title={item.description}
                                >
                                  {item.description}
                                </CardDescription>
                              </div>
                            </div>
                            <div className="flex shrink-0 items-start gap-2">
                              <div className="flex items-center gap-1">
                                <Button
                                  size="sm"
                                  variant={item.enabled ? "secondary" : "default"}
                                  className="h-8 rounded-lg px-3 text-[12px]"
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    if (!item.installed && item.marketplaceItem) {
                                      void runAction(() => props.onInstallMarketplaceItem(item.marketplaceItem!.id));
                                      return;
                                    }
                                    if (item.extensionId) {
                                      void runAction(() =>
                                        item.enabled
                                          ? props.onDisableExtension(item.extensionId!)
                                          : props.onEnableExtension(item.extensionId!),
                                      );
                                    }
                                  }}
                                  disabled={isActionPending || (item.installed && !item.extensionId)}
                                >
                                  {!item.installed ? t("extensions.install") : item.enabled ? t("extensions.disable") : t("extensions.enable")}
                                </Button>
                                {item.installed ? (
                                  <Button
                                    size="sm"
                                    variant="ghost"
                                    className="h-8 rounded-lg px-2.5 text-[12px]"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      if (item.extensionId) {
                                        void runAction(() => props.onUninstallExtension(item.extensionId!));
                                      }
                                    }}
                                    disabled={isActionPending || !item.extensionId}
                                  >
                                    {t("extensions.remove")}
                                  </Button>
                                ) : null}
                              </div>
                            </div>
                          </div>
                        </CardHeader>
                        <CardContent className="space-y-3 pt-0">
                          <div className="flex flex-wrap gap-2">
                            <Badge variant="outline">v{item.version}</Badge>
                            {item.tags.slice(0, 3).map((tag) => (
                              <Badge key={tag} variant="outline">
                                {tag}
                              </Badge>
                            ))}
                            {item.commands.length > 0 ? <Badge variant="outline">{item.commands.length} {t("extensions.commandsBadge")}</Badge> : null}
                            {item.mcpServers.length > 0 ? <Badge variant="outline">{item.mcpServers.length} {t("extensions.mcpsBadge")}</Badge> : null}
                            {item.skillNames.length > 0 ? <Badge variant="outline">{item.skillNames.length} {t("extensions.skillsBadge")}</Badge> : null}
                            {item.hooks.length > 0 ? <Badge variant="outline">{item.hooks.length} {t("extensions.hooksBadge")}</Badge> : null}
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <p className="truncate text-[12px] text-app-subtle" title={item.sourceLabel}>{item.sourceLabel}</p>
                            {getPluginStateLabel(item, t) ? (
                              <Badge className={cn("shrink-0 capitalize", getHealthBadgeClass(item.health))}>
                                {getPluginStateLabel(item, t)}
                              </Badge>
                            ) : null}
                          </div>
                        </CardContent>
                      </Card>
                    ))
                  : null}

                {activeTab === "mcps"
                  ? filteredMcpServers.map((server) => (
                      <Card
                        key={server.id}
                        className={cn(
                          "cursor-pointer border transition-colors hover:border-app-border-strong",
                          selectedId === server.id && "border-app-border-strong bg-app-surface-hover",
                        )}
                        onClick={() => setSelectedId(server.id)}
                      >
                        <CardHeader className="gap-3 pb-3">
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex min-w-0 items-start gap-3">
                              <div className="mt-0.5 flex size-10 min-w-10 shrink-0 items-center justify-center rounded-2xl border border-app-border bg-app-canvas text-app-foreground">
                                <LocalLlmIcon slug="mcp" className="size-4" title="MCP" />
                              </div>
                              <div className="min-w-0">
                                <CardTitle className="text-[15px]">{server.label}</CardTitle>
                                <CardDescription className="mt-1 truncate text-[13px] leading-5" title={getMcpServerDescription(server)}>
                                  {getMcpServerDescription(server)}
                                </CardDescription>
                              </div>
                            </div>
                            <div className="flex shrink-0 items-start gap-2">
                              <div className="flex items-center gap-1">
                                <Button
                                  size="sm"
                                  variant="ghost"
                                  className="h-8 rounded-lg px-3 text-[12px]"
                                  disabled={pendingMcpIds.has(server.id)}
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    handleOpenEditMcpDialog(server);
                                  }}
                                >
                                  {t("extensions.edit")}
                                </Button>
                                <Button
                                  size="sm"
                                  variant="secondary"
                                  className="h-8 rounded-lg px-3 text-[12px]"
                                  disabled={pendingMcpIds.has(server.id)}
                                  onClick={(event) => {
                                    event.stopPropagation();
                                    void runMcpAction(server.id, () => props.onRestartMcpServer(server.id));
                                  }}
                                >
                                  {pendingMcpIds.has(server.id) ? <Spinner className="size-3.5" /> : null}
                                  {t("extensions.restart")}
                                </Button>
                              </div>
                            </div>
                          </div>
                        </CardHeader>
                        <CardContent className="space-y-3 pt-0">
                          <div className="flex flex-wrap gap-2">
                            <Badge variant="outline">{server.tools.length} {t("extensions.toolsBadge")}</Badge>
                            <Badge variant="outline">{server.resources.length} {t("extensions.resourcesBadge")}</Badge>
                            {server.staleSnapshot ? <Badge variant="outline">{t("extensions.staleSnapshot")}</Badge> : null}
                          </div>
                        </CardContent>
                        <CardFooter className="justify-between gap-2">
                          <Switch
                            checked={server.config.enabled}
                            disabled={pendingMcpIds.has(server.id)}
                            onCheckedChange={(checked) =>
                              void runMcpAction(server.id, () =>
                                checked ? props.onEnableExtension(server.id) : props.onDisableExtension(server.id),
                              )
                            }
                            aria-label={`${server.label} enabled`}
                          />
                          <Badge className={cn("shrink-0", pendingMcpIds.has(server.id) ? "bg-app-surface-muted/80 text-app-subtle" : getStatusBadgeClass(server.status))}>
                            {pendingMcpIds.has(server.id) ? (
                              <span className="flex items-center gap-1.5"><Spinner className="size-3" />{t("extensions.connecting")}</span>
                            ) : (
                              getStatusBadgeLabel(server.status)
                            )}
                          </Badge>
                        </CardFooter>
                      </Card>
                    ))
                  : null}

                {activeTab === "skills"
                  ? filteredSkills.map((skill) => (
                      <Card
                        key={skill.id}
                        className={cn(
                          "cursor-pointer border transition-colors hover:border-app-border-strong",
                          selectedId === skill.id && "border-app-border-strong bg-app-surface-hover",
                        )}
                        onClick={() => setSelectedId(skill.id)}
                      >
                        <CardHeader className="gap-3 pb-3">
                          <div className="flex items-start justify-between gap-3">
                            <div className="flex min-w-0 items-start gap-3">
                              <SkillIcon />
                              <div className="min-w-0">
                                <CardTitle className="text-[15px]">{skill.name}</CardTitle>
                                <CardDescription
                                  className="mt-1 line-clamp-2 min-h-[2.5rem] text-[13px] leading-5"
                                  title={skill.description ?? "Skill record"}
                                >
                                  {skill.description ?? "Skill record"}
                                </CardDescription>
                              </div>
                            </div>
                            <div className="flex shrink-0 items-start gap-2">
                              <Button
                                size="sm"
                                variant={skill.enabled ? "secondary" : "default"}
                                className="h-8 rounded-lg px-3 text-[12px]"
                                onClick={(event) => {
                                  event.stopPropagation();
                                  void runAction(() =>
                                    skill.enabled ? props.onDisableSkill(skill.id) : props.onEnableSkill(skill.id),
                                  );
                                }}
                              >
                                {skill.enabled ? t("extensions.disable") : t("extensions.enable")}
                              </Button>
                            </div>
                          </div>
                        </CardHeader>
                        <CardContent className="space-y-3 pt-0">
                          <div className="flex flex-wrap gap-2">
                            <Badge variant="outline">{skill.triggers.length} {t("extensions.triggersBadge")}</Badge>
                            {skill.tools.length > 0 ? <Badge variant="outline">{skill.tools.length} {t("extensions.toolsBadge")}</Badge> : null}
                          </div>
                        </CardContent>
                        <CardFooter className="justify-end gap-3">
                          <div className="flex items-center justify-end gap-3">
                            <Badge className={cn("shrink-0", getSkillStatusBadgeClass(skill.enabled))}>
                              {getSkillStatusLabel(skill.enabled, t)}
                            </Badge>
                          </div>
                        </CardFooter>
                      </Card>
                    ))
                  : null}

                {((activeTab === "plugins" && pluginItems.length === 0) ||
                  (activeTab === "mcps" && filteredMcpServers.length === 0) ||
                  (activeTab === "skills" && filteredSkills.length === 0)) ? (
                  <Card className="md:col-span-2">
                    <CardContent className="flex min-h-40 items-center justify-center text-center text-sm text-app-muted">
                      {t("extensions.noMatch", { label: activeMeta.label.toLowerCase() })}
                    </CardContent>
                  </Card>
                ) : null}
                </div>
              </div>
            </ScrollArea>

            <aside className="min-h-0 min-w-0 overflow-hidden rounded-2xl border border-app-border bg-app-surface/80">
              <ScrollArea className="h-full">
                <div className="min-w-0 space-y-5 overflow-hidden p-5">
                  {activeTab === "plugins" ? (
                    selectedPluginItem ? (
                      <>
                        <div className="space-y-3">
                          <div className="flex items-center gap-2">
                            <Badge variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                              {t("extensions.pluginBadge")}
                            </Badge>
                            <Badge variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                              {selectedPluginItem.sourceLabel}
                            </Badge>
                            {getPluginStateLabel(selectedPluginItem, t) ? (
                              <Badge className={getHealthBadgeClass(selectedPluginItem.health)}>
                                {getPluginStateLabel(selectedPluginItem, t)}
                              </Badge>
                            ) : null}
                          </div>
                          <h2 className="text-lg font-semibold">{selectedPluginItem.name}</h2>
                          <p className="text-sm leading-6 text-app-muted">
                            {selectedPluginItem.marketplaceItem?.description
                              ?? selectedDetail?.summary.description
                              ?? selectedPluginItem.description}
                          </p>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              size="sm"
                              variant={selectedPluginItem.enabled ? "secondary" : "default"}
                              onClick={() => {
                                if (!selectedPluginItem.installed && selectedPluginItem.marketplaceItem) {
                                  void runAction(() => props.onInstallMarketplaceItem(selectedPluginItem.marketplaceItem!.id));
                                  return;
                                }
                                if (selectedPluginItem.extensionId) {
                                  void runAction(() =>
                                    selectedPluginItem.enabled
                                      ? props.onDisableExtension(selectedPluginItem.extensionId!)
                                      : props.onEnableExtension(selectedPluginItem.extensionId!),
                                  );
                                }
                              }}
                              disabled={isActionPending || (selectedPluginItem.installed && !selectedPluginItem.extensionId)}
                            >
                              {!selectedPluginItem.installed ? t("extensions.install") : selectedPluginItem.enabled ? t("extensions.disable") : t("extensions.enable")}
                            </Button>
                            {selectedPluginItem.installed ? (
                              <Button
                                size="sm"
                                variant="ghost"
                                onClick={() => {
                                  if (selectedPluginItem.extensionId) {
                                    void runAction(() => props.onUninstallExtension(selectedPluginItem.extensionId!));
                                  }
                                }}
                                disabled={isActionPending || !selectedPluginItem.extensionId}
                              >
                                {t("extensions.remove")}
                              </Button>
                            ) : null}
                          </div>
                        </div>

                        {selectedDetail?.plugin ? (
                          <div className="space-y-4">
                            <DetailSection title={t("extensions.capabilities")}>
                              {selectedDetail.plugin.capabilities.map((item) => (
                                <Badge key={item} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                  {item}
                                </Badge>
                              ))}
                            </DetailSection>
                            <DetailSection title={t("extensions.permissions")}>
                              {selectedDetail.plugin.permissions.map((item) => (
                                <Badge key={item} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                  {item}
                                </Badge>
                              ))}
                            </DetailSection>
                            <DetailSection title={t("extensions.tools")}>
                              {selectedDetail.plugin.tools.map((tool) => (
                                <div key={tool.name} className="rounded-xl border border-app-border bg-app-canvas/70 p-3">
                                  <div className="break-all text-sm font-medium">{tool.name}</div>
                                  <div className="mt-1 break-words text-xs text-app-muted">{tool.description}</div>
                                  <div className="mt-2 break-all text-[11px] text-app-subtle">
                                    {tool.command} {tool.args.join(" ")}
                                  </div>
                                </div>
                              ))}
                            </DetailSection>
                          </div>
                        ) : null}

                        <div className="space-y-4">
                          <DetailSection title={t("extensions.commands")}>
                            {(selectedDetail?.plugin?.commands.length ?? 0) > 0
                              ? selectedDetail!.plugin!.commands.map((command) => (
                                <Badge
                                  key={command.name}
                                  variant="outline"
                                  className={DETAIL_WRAP_BADGE_CLASS}
                                  title={command.description || undefined}
                                >
                                  /{command.name}
                                </Badge>
                                ))
                              : selectedPluginItem.commands.length > 0
                                ? selectedPluginItem.commands.map((command) => (
                                    <Badge
                                      key={command.name}
                                      variant="outline"
                                      className={DETAIL_WRAP_BADGE_CLASS}
                                      title={command.description ?? "Marketplace command bundle"}
                                    >
                                      /{command.name}
                                    </Badge>
                                  ))
                                : <p className="text-sm text-app-muted">{t("extensions.noBundledCommands")}</p>}
                          </DetailSection>
                          <DetailSection title={t("extensions.mcps")}>
                            {(selectedDetail?.plugin?.bundledMcpServers.length ?? 0) > 0
                              ? selectedDetail!.plugin!.bundledMcpServers.map((server) => (
                                  <Badge key={server} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                    {server}
                                  </Badge>
                                ))
                              : selectedPluginItem.mcpServers.length > 0
                                ? selectedPluginItem.mcpServers.map((server) => (
                                    <Badge key={server} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                      {server}
                                    </Badge>
                                  ))
                                : <p className="text-sm text-app-muted">{t("extensions.noBundledMcps")}</p>}
                          </DetailSection>
                          <DetailSection title={t("extensions.skills")}>
                            {(selectedDetail?.plugin?.bundledSkills.length ?? 0) > 0
                              ? selectedDetail!.plugin!.bundledSkills.map((skill) => (
                                  <Badge key={skill} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                    {skill}
                                  </Badge>
                                ))
                              : selectedPluginItem.skillNames.length > 0
                                ? selectedPluginItem.skillNames.map((skill) => (
                                    <Badge key={skill} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                      {skill}
                                    </Badge>
                                  ))
                                : <p className="text-sm text-app-muted">{t("extensions.noBundledSkills")}</p>}
                          </DetailSection>
                          <DetailSection title={t("extensions.hooks")}>
                            {(selectedDetail?.plugin?.hooks.length ?? 0) > 0
                              ? selectedDetail!.plugin!.hooks.map((hook) => (
                                  <div key={hook.event} className="rounded-xl border border-app-border bg-app-canvas/70 p-3">
                                    <div className="break-all text-sm font-medium">{hook.event}</div>
                                    <div className="mt-1 break-all text-xs text-app-muted">{hook.handlers.join(", ")}</div>
                                  </div>
                                ))
                              : selectedPluginItem.hooks.length > 0
                                ? selectedPluginItem.hooks.map((hook) => (
                                    <div key={hook.event} className="rounded-xl border border-app-border bg-app-canvas/70 p-3">
                                      <div className="break-all text-sm font-medium">{hook.event}</div>
                                      <div className="mt-1 break-all text-xs text-app-muted">{hook.handlers.join(", ")}</div>
                                    </div>
                                  ))
                                : <p className="text-sm text-app-muted">{t("extensions.noHooksDeclared")}</p>}
                          </DetailSection>
                          {selectedPluginSource ? (
                            <DetailSection title={t("extensions.source")}>
                              <p
                                className="break-all text-sm leading-6 text-app-muted"
                                title={`${selectedPluginSource.name} · ${selectedPluginSource.url}`}
                              >
                                {selectedPluginSource.url}
                              </p>
                            </DetailSection>
                          ) : null}
                        </div>
                      </>
                    ) : (
                      <div className="space-y-4">
                        <EmptyDetailState isLoading={Boolean(detailLoadingId)} label={activeMeta.label} />
                        <DetailSection title={t("extensions.marketplaceSources")}>
                          {props.marketplaceSources.length > 0 ? (
                            props.marketplaceSources.map((source) => (
                              <div
                                key={source.id}
                                className="w-full rounded-xl border border-app-border bg-app-canvas/70 p-3"
                              >
                                <div className="flex items-start justify-between gap-3">
                                  <div className="min-w-0">
                                    <div className="flex flex-wrap items-center gap-2">
                                      <div className="text-sm font-medium">{source.name}</div>
                                      <Badge variant="outline">{source.pluginCount} {t("extensions.pluginsBadge")}</Badge>
                                      <Badge
                                        className={cn(
                                          source.status === "ready"
                                            ? "bg-app-success/12 text-app-success"
                                            : source.status === "error"
                                              ? "bg-app-danger/12 text-app-danger"
                                              : "bg-app-surface-muted/80 text-app-subtle",
                                        )}
                                      >
                                        {source.status}
                                      </Badge>
                                    </div>
                                    <div className="mt-1 text-xs text-app-muted">{source.url}</div>
                                    {source.lastError ? (
                                      <div className="mt-2 text-xs text-app-danger">{source.lastError}</div>
                                    ) : null}
                                  </div>
                                  <div className="flex items-center gap-2">
                                    <Button
                                      size="sm"
                                      variant="outline"
                                      disabled={isActionPending || sourceActionLockedId === source.id}
                                      onClick={() => void runAction(() => props.onRefreshMarketplaceSource(source.id))}
                                    >
                                      {t("extensions.refresh")}
                                    </Button>
                                  </div>
                                </div>
                              </div>
                            ))
                          ) : (
                            <p className="text-sm text-app-muted">{t("extensions.noMarketplaceSources")}</p>
                          )}
                        </DetailSection>
                      </div>
                    )
                  ) : null}

                  {activeTab === "mcps" ? (
                    selectedDetail ? (
                      <>
                        <div className="space-y-2">
                          <div className="flex items-center gap-2">
                            <Badge variant="outline">{selectedDetail.summary.kind}</Badge>
                            <Badge className={getStatusBadgeClass(selectedDetail.mcp?.status ?? "unknown")}>
                              {getStatusBadgeLabel(selectedDetail.mcp?.status ?? "unknown")}
                            </Badge>
                          </div>
                          <h2 className="text-lg font-semibold">{selectedDetail.summary.name}</h2>
                          <p className="text-sm leading-6 text-app-muted">
                            {selectedDetail.mcp
                              ? getMcpServerDescription(selectedDetail.mcp)
                              : (selectedDetail.summary.description ?? "MCP server")}
                          </p>
                        </div>

                        {selectedDetail.mcp ? (
                          <div className="space-y-4">
                            <DetailSection title={t("extensions.actions")}>
                              <div className="flex flex-wrap gap-2">
                                <Button size="sm" variant="secondary" disabled={pendingMcpIds.has(selectedDetail.mcp!.id)} onClick={() => handleOpenEditMcpDialog(selectedDetail.mcp!)}>
                                  {t("extensions.edit")}
                                </Button>
                                <Button size="sm" variant="outline" disabled={pendingMcpIds.has(selectedDetail.mcp!.id)} onClick={() => void runMcpAction(selectedDetail.mcp!.id, () => props.onRestartMcpServer(selectedDetail.mcp!.id))}>
                                  {pendingMcpIds.has(selectedDetail.mcp!.id) ? <Spinner className="size-3.5" /> : null}
                                  {t("extensions.restart")}
                                </Button>
                                <Button size="sm" variant="ghost" disabled={pendingMcpIds.has(selectedDetail.mcp!.id)} onClick={() => void runMcpAction(selectedDetail.mcp!.id, () => props.onRemoveMcpServer(selectedDetail.mcp!.id))}>
                                  {t("extensions.remove")}
                                </Button>
                              </div>
                            </DetailSection>
                            <DetailSection title={t("extensions.tools")}>
                              {selectedDetail.mcp.tools.length > 0 ? (
                                selectedDetail.mcp.tools.map((tool) => (
                                  <div key={tool.qualifiedName} className="rounded-xl border border-app-border bg-app-canvas/70 p-3">
                                    <div className="text-sm font-medium" title={tool.qualifiedName}>
                                      {tool.name}
                                    </div>
                                    <div
                                      className="mt-1 overflow-hidden text-xs leading-5 text-app-muted"
                                      style={{
                                        WebkitBoxOrient: "vertical",
                                        WebkitLineClamp: 3,
                                        display: "-webkit-box",
                                      }}
                                      title={tool.description ?? "No description"}
                                    >
                                      {tool.description ?? "No description"}
                                    </div>
                                    {getMcpToolParameters(tool.inputSchema).length > 0 ? (
                                      <div className="mt-3 space-y-2">
                                        <div className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">
                                          {t("extensions.parameters")}
                                        </div>
                                        <div className="space-y-2">
                                          {getMcpToolParameters(tool.inputSchema).map((parameter) => (
                                            <div key={parameter.name} className="rounded-lg border border-app-border/80 bg-app-surface/50 px-2.5 py-2">
                                              <div className="flex flex-wrap items-center gap-2">
                                                <span className="text-xs font-medium text-app-foreground">{parameter.name}</span>
                                                {parameter.type ? (
                                                  <Badge variant="outline" className="h-5 px-1.5 text-[10px]">
                                                    {parameter.type}
                                                  </Badge>
                                                ) : null}
                                                {parameter.required ? (
                                                  <Badge variant="outline" className="h-5 px-1.5 text-[10px]">
                                                    {t("extensions.requiredBadge")}
                                                  </Badge>
                                                ) : null}
                                              </div>
                                              {parameter.description ? (
                                                <div
                                                  className="mt-1 overflow-hidden text-[11px] leading-5 text-app-muted"
                                                  style={{
                                                    WebkitBoxOrient: "vertical",
                                                    WebkitLineClamp: 3,
                                                    display: "-webkit-box",
                                                  }}
                                                  title={parameter.description}
                                                >
                                                  {parameter.description}
                                                </div>
                                              ) : null}
                                            </div>
                                          ))}
                                        </div>
                                      </div>
                                    ) : null}
                                  </div>
                                ))
                              ) : (
                                <p className="text-sm text-app-muted">{t("extensions.noDiscoveredTools")}</p>
                              )}
                            </DetailSection>
                          </div>
                        ) : null}

                      </>
                    ) : (
                      <EmptyDetailState isLoading={Boolean(detailLoadingId)} label={activeMeta.label} />
                    )
                  ) : null}

                  {activeTab === "skills" ? (
                    selectedDetail?.skill ? (
                      <>
                        <div className="min-w-0 space-y-3 overflow-hidden">
                          <div className="flex items-center gap-2">
                            <Badge variant="outline">{t("extensions.skillBadge")}</Badge>
                            <Badge className={getSkillStatusBadgeClass(selectedDetail.skill.enabled)}>
                              {getSkillStatusLabel(selectedDetail.skill.enabled, t)}
                            </Badge>
                          </div>
                          <div className="flex items-center justify-between gap-3">
                            <h2 className="min-w-0 text-lg font-semibold">{selectedDetail.skill.name}</h2>
                            <Button
                              size="icon-sm"
                              variant="ghost"
                              className="shrink-0"
                              onClick={() => setSkillPreviewDialogOpen(true)}
                              title={t("extensions.previewSkillMarkdown")}
                              aria-label={t("extensions.previewSkillMarkdown")}
                            >
                              <Eye className="size-4" />
                            </Button>
                          </div>
                          <p className="break-words text-sm leading-6 text-app-muted">
                            {selectedDetail.skill.description ?? "Skill record"}
                          </p>
                          <div className="flex flex-wrap gap-2">
                            <Button
                              size="sm"
                              variant={selectedDetail.skill.enabled ? "secondary" : "default"}
                              onClick={() =>
                                void runAction(() =>
                                  selectedDetail.skill!.enabled
                                    ? props.onDisableSkill(selectedDetail.skill!.id)
                                    : props.onEnableSkill(selectedDetail.skill!.id),
                                )
                              }
                            >
                              {selectedDetail.skill.enabled ? t("extensions.disable") : t("extensions.enable")}
                            </Button>
                          </div>
                        </div>

                        <div className="space-y-4">
                          <DetailSection title={t("extensions.triggersBadge")}>
                            {selectedDetail.skill.triggers.length > 0
                              ? selectedDetail.skill.triggers.map((trigger) => (
                                  <Badge key={trigger} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                    {trigger}
                                  </Badge>
                                ))
                              : <p className="text-sm text-app-muted">{t("extensions.noTriggersDefined")}</p>}
                          </DetailSection>
                          <DetailSection title={t("extensions.tools")}>
                            {selectedDetail.skill.tools.length > 0
                              ? selectedDetail.skill.tools.map((tool) => (
                                  <Badge key={tool} variant="outline" className={DETAIL_WRAP_BADGE_CLASS}>
                                    {tool}
                                  </Badge>
                                ))
                              : <p className="text-sm text-app-muted">{t("extensions.noToolRequirements")}</p>}
                          </DetailSection>
                        </div>
                      </>
                    ) : (
                      <EmptyDetailState isLoading={Boolean(detailLoadingId)} label={activeMeta.label} />
                    )
                  ) : null}
                </div>
              </ScrollArea>
            </aside>
          </div>
        </div>
      </div>

      <Dialog open={skillPreviewDialogOpen} onOpenChange={setSkillPreviewDialogOpen}>
        <DialogContent
          showCloseButton={false}
          className="flex h-[min(82vh,860px)] w-[min(calc(100vw-3rem),80rem)] max-w-[calc(100%-3rem)] flex-col overflow-hidden rounded-[24px] border border-app-border bg-app-surface p-0 shadow-[0_32px_96px_rgba(15,23,42,0.28)] sm:max-w-[min(calc(100vw-3rem),80rem)] dark:shadow-[0_32px_96px_rgba(0,0,0,0.56)]"
        >
          <DialogHeader className="shrink-0 border-b border-app-border px-5 py-4 text-left">
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0">
                <DialogTitle className="truncate">{selectedDetail?.skill?.name ?? t("extensions.skillPreview")}</DialogTitle>
                <DialogDescription className="mt-1">
                  {selectedDetail?.skill?.description ?? t("extensions.skillPreviewDesc")}
                </DialogDescription>
              </div>
              <DialogClose asChild>
                <button
                  type="button"
                  aria-label={t("extensions.closeSkillPreview")}
                  title={t("extensions.closeSkillPreview")}
                  className="flex size-8 shrink-0 items-center justify-center rounded-lg text-app-subtle transition-colors hover:bg-app-surface-hover hover:text-app-foreground"
                >
                  <CircleX className="size-4" />
                </button>
              </DialogClose>
            </div>
          </DialogHeader>
          <div className="shrink-0 border-b border-app-border bg-app-surface-muted/50 px-5 py-2 text-[11px] uppercase tracking-[0.12em] text-app-subtle">
            {t("extensions.skillMdPreview")}
          </div>
          <ScrollArea className="min-h-0 flex-1 bg-app-drawer">
            <div className="w-full min-w-0 px-5 py-5">
              <div className="w-full min-w-0 max-w-full space-y-4 overflow-hidden rounded-2xl border border-app-border bg-app-surface p-6 text-[14px] leading-7 text-app-muted shadow-[0_18px_48px_-36px_rgba(15,23,42,0.45)]">
                {selectedSkillPreview.entries.length > 0 ? (
                  <div className="rounded-xl border border-app-border bg-app-canvas/65 p-4">
                    <div className="mb-3 text-[11px] font-medium uppercase tracking-[0.14em] text-app-subtle">
                      {t("extensions.frontmatter")}
                    </div>
                    <div className="grid gap-3 md:grid-cols-2">
                      {selectedSkillPreview.entries.map((entry) => (
                        <div key={entry.key} className="min-w-0 rounded-lg border border-app-border/80 bg-app-surface/70 px-3 py-2.5">
                          <div className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">
                            {entry.key}
                          </div>
                          <div className="mt-1 break-words text-[13px] leading-6 text-app-foreground [overflow-wrap:anywhere]">
                            {entry.value}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : null}
                <Streamdown
                  className="w-full min-w-0 max-w-full break-words [&>*:first-child]:mt-0 [&>*:last-child]:mb-0 [&_a]:break-all [&_code]:break-all [&_p]:[overflow-wrap:anywhere] [&_li]:[overflow-wrap:anywhere] [&_blockquote]:[overflow-wrap:anywhere]"
                  controls={streamdownControls}
                  linkSafety={streamdownLinkSafety}
                  plugins={streamdownPlugins}
                >
                  {selectedSkillPreview.body}
                </Streamdown>
              </div>
            </div>
          </ScrollArea>
        </DialogContent>
      </Dialog>

      <Dialog open={mcpDialogOpen} onOpenChange={setMcpDialogOpen}>
        <DialogContent className="max-w-xl border-app-border bg-app-canvas">
          <DialogHeader>
            <DialogTitle>{mcpDialogMode === "create" ? t("extensions.addMcpServer") : t("extensions.editMcpServer")}</DialogTitle>
            <DialogDescription>
              {t("extensions.mcpDialogDesc")}
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-3">
            <div className="grid gap-2 sm:grid-cols-2">
              <Input
                value={mcpForm.id}
                onChange={(event) => setMcpForm((current) => ({ ...current, id: event.target.value }))}
                placeholder={t("extensions.serverIdPlaceholder")}
                disabled={mcpDialogMode === "edit"}
              />
              <Input
                value={mcpForm.label}
                onChange={(event) => setMcpForm((current) => ({ ...current, label: event.target.value }))}
                placeholder={t("extensions.displayNamePlaceholder")}
              />
            </div>

            <div className="grid gap-2 sm:grid-cols-2">
              <Select
                value={mcpForm.transport}
                onValueChange={(value) =>
                  setMcpForm((current) => ({ ...current, transport: value as McpFormState["transport"] }))
                }
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t("extensions.transportPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="stdio">stdio</SelectItem>
                  <SelectItem value="streamable-http">streamable-http</SelectItem>
                </SelectContent>
              </Select>
              <Input
                type="number"
                value={String(mcpForm.timeoutMs ?? 30_000)}
                onChange={(event) =>
                  setMcpForm((current) => ({ ...current, timeoutMs: Number(event.target.value || 30000) }))
                }
                placeholder={t("extensions.timeoutPlaceholder")}
              />
            </div>

            {mcpForm.transport === "stdio" ? (
              <>
                <Input
                  value={mcpForm.command ?? ""}
                  onChange={(event) => setMcpForm((current) => ({ ...current, command: event.target.value }))}
                  placeholder={t("extensions.commandPlaceholder")}
                />
                <Input
                  value={(mcpForm.args ?? []).join(" ")}
                  onChange={(event) =>
                    setMcpForm((current) => ({
                      ...current,
                      args: event.target.value
                        .split(" ")
                        .map((item) => item.trim())
                        .filter(Boolean),
                    }))
                  }
                  placeholder={t("extensions.argsPlaceholder")}
                />
              </>
            ) : (
              <>
                <Input
                  value={mcpForm.url ?? ""}
                  onChange={(event) => setMcpForm((current) => ({ ...current, url: event.target.value }))}
                  placeholder="https://example.com/mcp"
                />
                <Textarea
                  value={mcpHeadersText}
                  onChange={(event) => setMcpHeadersText(event.target.value)}
                  placeholder={"Authorization: Bearer <token>\nX-API-Key: <key>"}
                  className="min-h-28"
                />
                <div className="text-xs text-app-muted">
                  {t("extensions.headersHelper")}
                </div>
              </>
            )}

            <div className="flex items-center justify-between rounded-xl border border-app-border bg-app-surface p-3">
              <div>
                <div className="text-sm font-medium text-app-foreground">{t("extensions.enableOnSave")}</div>
                <div className="text-xs text-app-muted">{t("extensions.disabledEntryHint")}</div>
              </div>
              <Switch
                checked={mcpForm.enabled}
                onCheckedChange={(checked) => setMcpForm((current) => ({ ...current, enabled: checked }))}
              />
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setMcpDialogOpen(false)}>
              {t("extensions.cancelButton")}
            </Button>
            <Button onClick={() => void handleSubmitMcp()} disabled={isActionPending}>
              {mcpDialogMode === "create" ? t("extensions.createButton") : t("extensions.saveButton")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={marketSourceDialogOpen}
        onOpenChange={(open) => {
          if (isMarketSourceSubmitting) {
            return;
          }
          setMarketSourceDialogOpen(open);
          if (!open) {
            setMarketSourceError(null);
          }
        }}
      >
        <DialogContent className="max-w-xl border-app-border bg-app-canvas">
          <DialogHeader>
            <DialogTitle>{t("extensions.addMarketplaceSource")}</DialogTitle>
            <DialogDescription>
              {t("extensions.marketplaceSourceDesc")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            {marketSourceError ? (
              <div className="rounded-xl border border-app-danger/30 bg-app-danger/8 px-3 py-2 text-sm text-app-danger">
                {marketSourceError}
              </div>
            ) : null}
            <Input
              value={marketSourceForm.name}
              onChange={(event) => setMarketSourceForm((current) => ({ ...current, name: event.target.value }))}
              placeholder="Anthropic Official Plugins"
              disabled={isMarketSourceSubmitting}
            />
            <Input
              value={marketSourceForm.url}
              onChange={(event) => setMarketSourceForm((current) => ({ ...current, url: event.target.value }))}
              placeholder="https://github.com/anthropics/claude-plugins-official.git"
              disabled={isMarketSourceSubmitting}
            />
            <div className="text-xs text-app-muted">
              {isMarketSourceSubmitting
                ? t("extensions.addingSourceStatus")
                : t("extensions.addingSourceHelper")}
            </div>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setMarketSourceDialogOpen(false)}
              disabled={isMarketSourceSubmitting}
            >
              {t("extensions.cancelButton")}
            </Button>
            <Button
              onClick={() => void handleSubmitMarketplaceSource()}
              disabled={
                isMarketSourceSubmitting
                || marketSourceForm.name.trim().length === 0
                || marketSourceForm.url.trim().length === 0
              }
            >
              {isMarketSourceSubmitting ? (
                <>
                  <RefreshCw className="size-4 animate-spin" />
                  {t("extensions.addingSource")}
                </>
              ) : (
                t("extensions.addSource")
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={removeSourceDialogOpen}
        onOpenChange={(open) => {
          setRemoveSourceDialogOpen(open);
          if (!open) {
            setRemoveSourcePlan(null);
            setRemoveSourceTargetId(null);
            setRemoveSourceError(null);
            setRemoveSourcePreviewLoading(false);
            setRemoveSourceSubmitting(false);
          }
        }}
      >
        <DialogContent className="max-w-xl border-app-border bg-app-canvas">
          <DialogHeader>
            <DialogTitle>
              {removeSourcePlan && !removeSourcePlan.canRemove ? t("extensions.cantRemoveSource") : t("extensions.removeSource")}
            </DialogTitle>
            <DialogDescription>
              {isRemoveSourcePreviewLoading
                ? t("extensions.checkingRemovable")
                : removeSourcePlan && !removeSourcePlan.canRemove
                  ? t("extensions.hasEnabledPlugins")
                  : t("extensions.removeWarning")}
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-3">
            {removeSourceError ? (
              <div className="rounded-xl border border-app-danger/30 bg-app-danger/8 px-3 py-2 text-sm text-app-danger">
                {removeSourceError}
              </div>
            ) : null}

            {isRemoveSourcePreviewLoading ? (
              <div className="rounded-xl border border-app-border bg-app-surface px-4 py-5 text-sm text-app-muted">
                {t("extensions.loadingRemovalDetails")}
              </div>
            ) : null}

            {removeSourcePlan ? (
              <>
                <div className="rounded-xl border border-app-border bg-app-surface px-4 py-3">
                  <div className="text-sm font-medium text-app-foreground">{removeSourcePlan.source.name}</div>
                  <div className="mt-1 break-all text-xs text-app-muted">{removeSourcePlan.source.url}</div>
                  <div className="mt-2 text-xs text-app-muted">{removeSourcePlan.summary}</div>
                </div>

                {!removeSourcePlan.canRemove ? (
                  <div className="space-y-2">
                    <div className="text-sm font-medium text-app-foreground">{t("extensions.enabledPluginsBlocking")}</div>
                    <div className="space-y-2">
                      {removeSourcePlan.blockingPlugins.map((plugin) => (
                        <div
                          key={plugin.id}
                          className="flex items-center justify-between gap-3 rounded-xl border border-app-border bg-app-canvas/70 px-3 py-2"
                        >
                          <div className="min-w-0">
                            <div className="truncate text-sm font-medium text-app-foreground">{plugin.name}</div>
                            <div className="text-xs text-app-muted">v{plugin.version}</div>
                          </div>
                          <Badge className="bg-app-success/12 text-app-success">{t("extensions.enabledPluginBadge")}</Badge>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <div className="text-sm font-medium text-app-foreground">{t("extensions.pluginsToRemove")}</div>
                    {removeSourcePlan.removableInstalledPlugins.length > 0 ? (
                      <div className="space-y-2">
                        {removeSourcePlan.removableInstalledPlugins.map((plugin) => (
                          <div
                            key={plugin.id}
                            className="flex items-center justify-between gap-3 rounded-xl border border-app-border bg-app-canvas/70 px-3 py-2"
                          >
                            <div className="min-w-0">
                              <div className="truncate text-sm font-medium text-app-foreground">{plugin.name}</div>
                              <div className="text-xs text-app-muted">v{plugin.version}</div>
                            </div>
                            <Badge variant="outline">{t("extensions.installedBadge")}</Badge>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="rounded-xl border border-app-border bg-app-canvas/70 px-3 py-2 text-sm text-app-muted">
                        {t("extensions.noPluginsToRemove")}
                      </div>
                    )}
                  </div>
                )}
              </>
            ) : null}
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setRemoveSourceDialogOpen(false)}
              disabled={isRemoveSourcePreviewLoading || isRemoveSourceSubmitting}
            >
              {removeSourcePlan && !removeSourcePlan.canRemove ? t("extensions.closeButton") : t("extensions.cancelButton")}
            </Button>
            {removeSourcePlan?.canRemove ? (
              <Button onClick={() => void handleConfirmRemoveSource()} disabled={isRemoveSourceSubmitting}>
                {t("extensions.removeSourceConfirm")}
              </Button>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function DetailSection({ children, title }: { children: ReactNode; title: string }) {
  return (
    <section className="min-w-0 space-y-2">
      <h3 className="text-sm font-medium text-app-foreground">{title}</h3>
      <div className="min-w-0 flex flex-wrap gap-2">{children}</div>
    </section>
  );
}

function EmptyDetailState({ isLoading, label }: { isLoading: boolean; label: string }) {
  const t = useT();
  return (
    <div className="flex min-h-[220px] items-center justify-center rounded-2xl border border-dashed border-app-border bg-app-canvas/70 p-6 text-center text-sm text-app-muted">
      {isLoading ? t("extensions.loadingDetails", { label: label.toLowerCase() }) : t("extensions.selectToInspect", { label: label.toLowerCase() })}
    </div>
  );
}
