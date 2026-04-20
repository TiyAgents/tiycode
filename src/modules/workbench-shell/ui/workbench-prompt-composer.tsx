import type { ChatStatus, FileUIPart, SourceDocumentUIPart } from "ai";
import {
  BracesIcon,
  CheckIcon,
  FileCodeIcon,
  FileIcon,
  FileSearchIcon,
  FileTextIcon,
  ImageIcon,
  LoaderCircle,
  PaperclipIcon,
  UserStar,
  XIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type SyntheticEvent } from "react";
import {
  ModelSelector,
  ModelSelectorContent,
  ModelSelectorEmpty,
  ModelSelectorGroup,
  ModelSelectorItem,
  ModelSelectorList,
  ModelSelectorTrigger,
} from "@/components/ai-elements/model-selector";
import {
  PromptInput,
  PromptInputBody,
  PromptInputButton,
  PromptInputCommand,
  PromptInputCommandGroup,
  PromptInputCommandItem,
  PromptInputCommandList,
  PromptInputFooter,
  PromptInputHeader,
  type PromptInputMessage,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  usePromptInputAttachments,
  usePromptInputReferencedSources,
} from "@/components/ai-elements/prompt-input";
import { Suggestion, Suggestions } from "@/components/ai-elements/suggestion";
import {
  buildComposerCommandRegistry,
  filterComposerCommands,
  parseSlashCommandInput,
  shouldSmartSendCommand,
  SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT,
  SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS,
  type ComposerCommandDescriptor,
  type ComposerReferencedFile,
  type ComposerSubmission,
} from "@/modules/workbench-shell/model/composer-commands";
import {
  getProfilePrimaryModelId,
  getProfilePrimaryModelLabel,
  resolveProfileModelByTier,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import type { AgentProfile, CommandEntry, ProviderEntry } from "@/modules/settings-center/model/types";
import type { SkillRecord } from "@/shared/types/extensions";
import type { RunMode } from "@/shared/types/api";
import { indexFilterFiles, type FileFilterMatch } from "@/services/bridge";
import { useT } from "@/i18n";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { ModelBrandIcon } from "@/shared/ui/model-brand-icon";
import { Switch } from "@/shared/ui/switch";

type ComposerAttachment = {
  id: string;
  mediaType?: string;
  name: string;
  url?: string;
};

type MentionMatch = {
  query: string;
  token: "@" | "$";
  tokenEnd: number;
  tokenStart: number;
};

type ReferencedSourceBridgeProps = {
  clearSignal: number;
  onBridgeReady: (bridge: ReferencedSourceBridgeHandle) => void;
};

type ReferencedSourceBridgeHandle = {
  clear: () => void;
  syncFiles: (files: ReadonlyArray<ComposerReferencedFile>) => void;
};

const FILE_SEARCH_RESULT_LIMIT = 200;
const FILE_SEARCH_DEBOUNCE_MS = 120;

type WorkbenchPromptComposerProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  canSubmitWhenAttachmentsOnly?: boolean;
  className?: string;
  commands?: ReadonlyArray<CommandEntry>;
  composerShellClassName?: string;
  enabledSkills?: ReadonlyArray<Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">>;
  error?: string | null;
  onErrorMessageChange?: (message: string | null) => void;
  onRunModeChange?: (mode: RunMode) => void;
  onSelectAgentProfile: (id: string) => void;
  onStop: () => void;
  onSubmit: (submission: ComposerSubmission) => void;
  placeholder: string;
  providers: ReadonlyArray<ProviderEntry>;
  runMode?: RunMode;
  runModeDisabled?: boolean;
  showRunModeToggle?: boolean;
  status: ChatStatus;
  suggestions?: ReadonlyArray<string>;
  textareaClassName?: string;
  value: string;
  workspaceId?: string | null;
  onValueChange: (value: string) => void;
};

function getFileExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function isSlashCommandActive(value: string) {
  return value.trimStart().startsWith("/");
}

function buildSubmissionFromPromptInput(
  message: PromptInputMessage,
  registry: ReadonlyArray<ComposerCommandDescriptor>,
  runMode: RunMode,
  referencedFiles: ReadonlyArray<ComposerReferencedFile>,
): ComposerSubmission {
  const trimmedText = message.text?.trim() ?? "";
  const attachments = mapComposerAttachments(message.files);
  const parsedCommand = trimmedText ? parseSlashCommandInput(trimmedText, registry) : null;
  const referencedFilesMetadata = referencedFiles.length > 0
    ? referencedFiles.map((file) => ({
        name: file.name,
        path: file.path,
        parentPath: file.parentPath,
      }))
    : [];

  if (!parsedCommand?.command) {
    const effectivePrompt = trimmedText;
    return {
      kind: "plain",
      displayText: trimmedText,
      effectivePrompt,
      rawMessage: message,
      attachments,
      metadata: referencedFilesMetadata.length > 0
        ? {
            composer: {
              kind: "plain",
              displayText: trimmedText,
              effectivePrompt,
              referencedFiles: referencedFilesMetadata,
            },
          }
        : null,
      runMode,
    };
  }

  const effectivePrompt = parsedCommand.command.prompt
    .replace(/{{\s*arguments\s*}}/g, parsedCommand.argumentsText)
    .replace(/{{\s*command\s*}}/g, parsedCommand.command.name)
    .trim();

  return {
    kind: "command",
    displayText: trimmedText,
    effectivePrompt,
    rawMessage: message,
    attachments,
    runMode,
    command: {
      source: parsedCommand.command.source,
      name: parsedCommand.command.name,
      path: parsedCommand.command.path,
      description: parsedCommand.command.description,
      argumentHint: parsedCommand.command.argumentHint,
      argumentsText: parsedCommand.argumentsText,
      prompt: effectivePrompt,
      behavior: parsedCommand.command.behavior,
    },
    metadata: {
      composer: {
        kind: "command",
        displayText: trimmedText,
        effectivePrompt,
        referencedFiles: referencedFilesMetadata,
        command: {
          source: parsedCommand.command.source,
          name: parsedCommand.command.name,
          path: parsedCommand.command.path,
          description: parsedCommand.command.description,
          argumentHint: parsedCommand.command.argumentHint,
          argumentsText: parsedCommand.argumentsText,
          prompt: effectivePrompt,
          behavior: parsedCommand.command.behavior,
        },
      },
    },
  };
}

function getCommandDisplayPath(command: ComposerCommandDescriptor) {
  return command.source === "builtin" ? `/${command.name}` : command.path;
}

function getCommandCompletionValue(command: ComposerCommandDescriptor) {
  return getCommandDisplayPath(command);
}

function getDefaultSelectedCommand(
  commands: ReadonlyArray<ComposerCommandDescriptor>,
) {
  return commands[0] ?? null;
}

function getCommandItemKey(command: ComposerCommandDescriptor) {
  return `${command.source}:${command.name}`;
}

function getNextCommandIndex(currentIndex: number, commandCount: number, delta: number) {
  if (commandCount === 0) {
    return -1;
  }

  if (currentIndex < 0) {
    return delta > 0 ? 0 : commandCount - 1;
  }

  return (currentIndex + delta + commandCount) % commandCount;
}

function findSelectedCommandIndex(
  commands: ReadonlyArray<ComposerCommandDescriptor>,
  selectedCommand: ComposerCommandDescriptor | null,
) {
  if (!selectedCommand) {
    return -1;
  }

  return commands.findIndex((command) => command.source === selectedCommand.source && command.name === selectedCommand.name);
}

function buildCommandInputValue(command: ComposerCommandDescriptor) {
  return getCommandDisplayPath(command);
}

function shouldShowCommandPicker(value: string) {
  return isSlashCommandActive(value);
}

function shouldAutoSubmitCommand(command: ComposerCommandDescriptor, value: string) {
  const parsed = parseSlashCommandInput(value, [command]);
  return shouldSmartSendCommand(command, parsed?.argumentsText ?? "");
}

function getParsedActiveCommand(
  value: string,
  registry: ReadonlyArray<ComposerCommandDescriptor>,
) {
  return parseSlashCommandInput(value, registry);
}

function getFilteredCommands(
  registry: ReadonlyArray<ComposerCommandDescriptor>,
  value: string,
) {
  const parsed = getParsedActiveCommand(value, registry);
  return filterComposerCommands(registry, parsed?.query ?? "");
}

function getSelectedCommandFromFiltered(
  filteredCommands: ReadonlyArray<ComposerCommandDescriptor>,
  selectedCommand: ComposerCommandDescriptor | null,
) {
  if (selectedCommand) {
    const matched = filteredCommands.find((command) => command.source === selectedCommand.source && command.name === selectedCommand.name);
    if (matched) {
      return matched;
    }
  }

  return getDefaultSelectedCommand(filteredCommands);
}

function getActiveMentionMatch(value: string, cursorPosition: number | null): MentionMatch | null {
  if (cursorPosition == null) {
    return null;
  }

  const safeCursor = Math.max(0, Math.min(cursorPosition, value.length));
  const triggers: Array<"@" | "$"> = ["@", "$"];
  let tokenStart = -1;
  let token: "@" | "$" | null = null;

  for (const candidate of triggers) {
    const index = value.lastIndexOf(candidate, safeCursor - 1);
    if (index > tokenStart) {
      tokenStart = index;
      token = candidate;
    }
  }

  if (tokenStart < 0 || !token) {
    return null;
  }

  const beforeToken = tokenStart === 0 ? "" : value[tokenStart - 1] ?? "";
  if (beforeToken && !/\s|[(\[{]/.test(beforeToken)) {
    return null;
  }

  const between = value.slice(tokenStart + 1, safeCursor);
  if (between.length === 0) {
    return {
      query: "",
      token,
      tokenEnd: safeCursor,
      tokenStart,
    };
  }

  if (/\s/.test(between)) {
    return null;
  }

  const tokenEndsEmailOrWord = token === "@" && tokenStart > 0 && /[A-Za-z0-9._-]/.test(beforeToken);
  if (tokenEndsEmailOrWord) {
    return null;
  }

  return {
    query: between,
    token,
    tokenEnd: safeCursor,
    tokenStart,
  };
}

function getNextFileIndex(currentIndex: number, resultCount: number, delta: number) {
  if (resultCount === 0) {
    return -1;
  }

  if (currentIndex < 0) {
    return delta > 0 ? 0 : resultCount - 1;
  }

  return (currentIndex + delta + resultCount) % resultCount;
}

function buildMentionSourceDocument(file: ComposerReferencedFile): SourceDocumentUIPart {
  return {
    type: "source-document",
    sourceId: file.path,
    title: file.name,
    filename: file.name,
    mediaType: "text/plain",
    providerMetadata: {
      tiy: {
        parentPath: file.parentPath,
        path: file.path,
      },
    },
  };
}

function mergeReferencedFiles(
  current: ReadonlyArray<ComposerReferencedFile>,
  nextFile: ComposerReferencedFile,
) {
  const withoutDuplicate = current.filter((file) => file.path !== nextFile.path);
  return [...withoutDuplicate, nextFile];
}

function removeUnmentionedReferencedFiles(
  current: ReadonlyArray<ComposerReferencedFile>,
  value: string,
) {
  return current.filter((file) => value.includes(`@${file.path}`));
}

function filterReferencedSkills(
  skills: ReadonlyArray<Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">>,
  query: string,
) {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return skills.slice(0, 20);
  }

  return skills
    .filter((skill) => {
      const haystacks = [
        skill.name,
        skill.description ?? "",
        skill.contentPreview,
        skill.scope,
        skill.source,
        ...skill.tags,
        ...skill.triggers,
      ];
      return haystacks.some((value) => value.toLowerCase().includes(normalizedQuery));
    })
    .slice(0, 20);
}


function getExtensionColor(ext: string): string {
  const colorMap: Record<string, string> = {
    pdf: "bg-red-500/15 text-red-400",
    md: "bg-purple-500/15 text-purple-400",
    json: "bg-yellow-500/15 text-yellow-400",
    ts: "bg-blue-500/15 text-blue-400",
    tsx: "bg-blue-500/15 text-blue-400",
    txt: "bg-gray-500/15 text-gray-400",
    png: "bg-emerald-500/15 text-emerald-400",
    jpg: "bg-emerald-500/15 text-emerald-400",
    jpeg: "bg-emerald-500/15 text-emerald-400",
    gif: "bg-emerald-500/15 text-emerald-400",
    webp: "bg-emerald-500/15 text-emerald-400",
    svg: "bg-emerald-500/15 text-emerald-400",
  };
  return colorMap[ext] || "bg-gray-500/15 text-gray-400";
}

function getAttachmentGlyph(mediaType?: string, name?: string) {
  const ext = name ? getFileExtension(name) : "";

  if (mediaType?.startsWith("image/")) {
    return <ImageIcon className="size-3.5" />;
  }
  if (mediaType === "application/pdf" || ext === "pdf") {
    return <FileTextIcon className="size-3.5" />;
  }
  if (mediaType === "text/markdown" || ext === "md") {
    return <FileCodeIcon className="size-3.5" />;
  }
  if (mediaType === "application/json" || ext === "json") {
    return <BracesIcon className="size-3.5" />;
  }
  if (ext === "ts" || ext === "tsx") {
    return <FileCodeIcon className="size-3.5" />;
  }
  if (mediaType === "text/plain" || ext === "txt") {
    return <FileTextIcon className="size-3.5" />;
  }

  return <FileIcon className="size-3.5" />;
}

function isImageAttachment(attachment: ComposerAttachment): boolean {
  return Boolean(attachment.mediaType?.startsWith("image/"));
}

function AttachmentImageCard({
  attachment,
  onRemove,
  compact = false,
}: {
  attachment: ComposerAttachment;
  onRemove?: (id: string) => void;
  compact?: boolean;
}) {
  const t = useT();
  const [imgFailed, setImgFailed] = useState(false);

  const handleImgError = (event: SyntheticEvent<HTMLImageElement>) => {
    event.currentTarget.style.display = "none";
    setImgFailed(true);
  };

  return (
    <div
      className={cn(
        "group relative overflow-hidden rounded-xl border border-app-border/45 bg-app-surface/60",
        compact ? "h-[52px] w-[52px]" : "h-[120px] w-[160px]",
      )}
    >
      {attachment.url && !imgFailed ? (
        <img
          alt={attachment.name}
          className="size-full object-cover"
          decoding="async"
          onError={handleImgError}
          src={attachment.url}
        />
      ) : (
        <div className="flex size-full items-center justify-center bg-app-surface-muted/60">
          <ImageIcon className={cn(compact ? "size-5" : "size-8", "text-app-subtle")} />
        </div>
      )}
      {/* Filename overlay (message mode, non-compact) */}
      {!onRemove && !compact ? (
        <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/60 to-transparent px-2 pb-1.5 pt-4">
          <span className="block truncate text-[11px] font-medium text-white/90">
            {attachment.name}
          </span>
        </div>
      ) : null}
      {/* Remove button (composer mode) */}
      {onRemove ? (
        <button
          aria-label={t("composer.removeAttachment", { name: attachment.name })}
          className="absolute right-1 top-1 flex size-5 items-center justify-center rounded-full bg-black/50 text-white/90 opacity-0 backdrop-blur-sm transition group-hover:opacity-100"
          onClick={(event) => {
            event.preventDefault();
            onRemove(attachment.id);
          }}
          type="button"
        >
          <XIcon className="size-3" />
        </button>
      ) : null}
    </div>
  );
}

function AttachmentFileCard({
  attachment,
  onRemove,
  tone = "message",
}: {
  attachment: ComposerAttachment;
  onRemove?: (id: string) => void;
  tone?: "composer" | "message";
}) {
  const t = useT();
  const ext = getFileExtension(attachment.name);

  return (
    <div
      className={cn(
        "inline-flex max-w-[220px] items-center gap-2.5 rounded-xl border px-3 py-2 text-xs font-medium",
        tone === "composer"
          ? "border-app-border/55 bg-app-surface-muted/80 text-app-foreground"
          : "border-app-border/45 bg-app-surface/60 text-app-muted",
      )}
    >
      {/* Extension badge + icon */}
      <div className="flex shrink-0 items-center gap-1.5">
        {ext ? (
          <span
            className={cn(
              "inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-bold uppercase leading-none",
              getExtensionColor(ext),
            )}
          >
            {ext}
          </span>
        ) : (
          <span className={cn("shrink-0", tone === "composer" ? "text-app-foreground" : "text-app-subtle")}>
            {getAttachmentGlyph(attachment.mediaType, attachment.name)}
          </span>
        )}
      </div>
      {/* Filename */}
      <span className="min-w-0 truncate">{attachment.name}</span>
      {/* Remove button */}
      {onRemove ? (
        <button
          aria-label={t("composer.removeAttachment", { name: attachment.name })}
          className="inline-flex size-4 shrink-0 items-center justify-center rounded-full text-app-subtle transition hover:bg-app-surface-hover hover:text-app-foreground"
          onClick={(event) => {
            event.preventDefault();
            onRemove(attachment.id);
          }}
          type="button"
        >
          <XIcon className="size-3" />
        </button>
      ) : null}
    </div>
  );
}

function AttachmentCard({
  attachment,
  onRemove,
  compact = false,
  tone = "message",
}: {
  attachment: ComposerAttachment;
  onRemove?: (id: string) => void;
  compact?: boolean;
  tone?: "composer" | "message";
}) {
  if (isImageAttachment(attachment)) {
    return <AttachmentImageCard attachment={attachment} compact={compact} onRemove={onRemove} />;
  }
  return <AttachmentFileCard attachment={attachment} onRemove={onRemove} tone={tone} />;
}

function ComposerAttachmentHeader() {
  const t = useT();
  const attachments = usePromptInputAttachments();

  if (attachments.files.length === 0) {
    return null;
  }

  return (
    <PromptInputHeader className="border-b border-app-border/45 bg-app-surface-muted/35 px-3 py-2">
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
        <Badge className="rounded-full px-2 py-0.5" variant="secondary">
          <PaperclipIcon className="size-3" />
          {t("composer.attachmentCount", { count: attachments.files.length })}
        </Badge>
        {attachments.files.map((attachment) => (
          <AttachmentCard
            attachment={{
              id: attachment.id,
              mediaType: attachment.mediaType,
              name: attachment.filename?.trim() || t("composer.unnamedAttachment"),
              url: attachment.url,
            }}
            compact
            key={attachment.id}
            onRemove={attachments.remove}
            tone="composer"
          />
        ))}
      </div>
    </PromptInputHeader>
  );
}

function ComposerAttachmentStateSync({
  onHasAttachmentsChange,
}: {
  onHasAttachmentsChange: (hasAttachments: boolean) => void;
}) {
  const attachments = usePromptInputAttachments();

  return (
    <ComposerAttachmentStateSyncInner
      attachmentCount={attachments.files.length}
      onHasAttachmentsChange={onHasAttachmentsChange}
    />
  );
}

function ComposerAttachmentStateSyncInner({
  attachmentCount,
  onHasAttachmentsChange,
}: {
  attachmentCount: number;
  onHasAttachmentsChange: (hasAttachments: boolean) => void;
}) {
  useEffect(() => {
    onHasAttachmentsChange(attachmentCount > 0);
  }, [attachmentCount, onHasAttachmentsChange]);

  return null;
}

function ComposerReferencedSourcesBridge({
  clearSignal,
  onBridgeReady,
}: ReferencedSourceBridgeProps) {
  const referencedSources = usePromptInputReferencedSources();
  const previousFilesRef = useRef<ReadonlyArray<ComposerReferencedFile>>([]);
  const referencedSourcesRef = useRef(referencedSources);
  referencedSourcesRef.current = referencedSources;

  useEffect(() => {
    onBridgeReady({
      clear: () => {
        referencedSourcesRef.current.clear();
        previousFilesRef.current = [];
      },
      syncFiles: (files) => {
        const ctx = referencedSourcesRef.current;
        const previousPaths = new Set(previousFilesRef.current.map((file) => file.path));
        const nextPaths = new Set(files.map((file) => file.path));

        for (const source of ctx.sources) {
          const sourcePath = source.sourceId;
          if (sourcePath && !nextPaths.has(sourcePath)) {
            ctx.remove(source.id);
          }
        }

        const newFiles = files.filter((file) => !previousPaths.has(file.path));
        if (newFiles.length > 0) {
          ctx.add(newFiles.map((file) => buildMentionSourceDocument(file)));
        }

        previousFilesRef.current = files;
      },
    });
  }, [onBridgeReady]);

  useEffect(() => {
    referencedSourcesRef.current.clear();
    previousFilesRef.current = [];
  }, [clearSignal]);

  return null;
}

function ComposerReferencedFilesHeader({
  files,
  onRemove,
}: {
  files: ReadonlyArray<ComposerReferencedFile>;
  onRemove: (path: string) => void;
}) {
  const t = useT();

  if (files.length === 0) {
    return null;
  }

  return (
    <PromptInputHeader className="border-b border-app-border/45 bg-app-surface-muted/35 px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge className="gap-1.5 rounded-full border-app-info/30 bg-app-info/10 px-2.5 py-1 text-[11px] text-app-info">
          <FileSearchIcon className="size-3" />
          {t("composer.fileReferenceCount", { count: files.length })}
        </Badge>
        {files.map((file) => (
          <Badge
            className="gap-1 rounded-full border border-app-border/55 bg-app-surface px-2.5 py-1 text-[11px] text-app-foreground"
            key={file.path}
            variant="outline"
          >
            <span className="max-w-[240px] truncate" title={file.path}>{file.path}</span>
            <button
              aria-label={t("composer.removeFileReference", { path: file.path })}
              className="inline-flex size-4 items-center justify-center rounded-full text-app-subtle transition-colors hover:bg-app-surface-muted hover:text-app-foreground"
              onClick={(event) => {
                event.preventDefault();
                onRemove(file.path);
              }}
              type="button"
            >
              <XIcon className="size-3" />
            </button>
          </Badge>
        ))}
      </div>
    </PromptInputHeader>
  );
}

function ComposerAttachmentTrigger() {
  const t = useT();
  const attachments = usePromptInputAttachments();

  return (
    <PromptInputButton
      aria-label={t("composer.uploadFileOrImage")}
      className="px-2.5"
      onClick={(event) => {
        event.preventDefault();
        attachments.openFileDialog();
      }}
      type="button"
    >
      <PaperclipIcon className="size-4" />
    </PromptInputButton>
  );
}

function PromptInputSubmitButton({
  activeProfile,
  allowAttachmentsOnly,
  composerValue,
  onStop,
  status,
}: {
  activeProfile: AgentProfile | null;
  allowAttachmentsOnly: boolean;
  composerValue: string;
  onStop: () => void;
  status: ChatStatus;
}) {
  const attachments = usePromptInputAttachments();
  const hasText = Boolean(composerValue.trim());
  const hasAttachments = attachments.files.length > 0;
  const isStopping = status === "submitted" || status === "streaming";
  const canSubmit = Boolean(activeProfile) && (hasText || (allowAttachmentsOnly && hasAttachments));

  return <PromptInputSubmit disabled={isStopping ? false : !canSubmit} onStop={onStop} status={status} />;
}

function ProfileInlineIdentity({
  badge = true,
  muted = false,
  profile,
  providers,
  showModel = true,
}: {
  badge?: boolean;
  muted?: boolean;
  profile: AgentProfile;
  providers: ReadonlyArray<ProviderEntry>;
  showModel?: boolean;
}) {
  const modelId = getProfilePrimaryModelId(profile, providers) || profile.name;
  const modelLabel = getProfilePrimaryModelLabel(profile, providers);

  return (
    <div className="flex min-w-0 items-center gap-2">
      <span
        className={cn(
          "flex shrink-0 items-center justify-center",
          badge ? "size-6 rounded-lg bg-app-surface-muted/75 ring-1 ring-app-border/45" : "size-4 rounded-none bg-transparent ring-0",
          muted ? "text-app-muted" : "text-app-foreground",
        )}
      >
        <UserStar className="size-3.5" />
      </span>
      <span className={cn("shrink-0 text-sm font-medium", muted ? "text-app-foreground/88" : "text-app-foreground")}>
        {profile.name}
      </span>
      {showModel ? (
        <>
          <span aria-hidden="true" className="shrink-0 text-app-subtle">
            {" · "}
          </span>
          <ModelBrandIcon
            className={cn("size-4 shrink-0", muted ? "text-app-muted" : "text-app-foreground")}
            displayName={modelLabel}
            modelId={modelId}
          />
          <span className={cn("min-w-0 truncate text-xs", muted ? "text-muted-foreground" : "text-app-muted")}>
            {modelLabel}
          </span>
        </>
      ) : null}
    </div>
  );
}

function ProfileSelectorItem({
  isActive,
  onSelect,
  profile,
  providers,
}: {
  isActive: boolean;
  onSelect: () => void;
  profile: AgentProfile;
  providers: ReadonlyArray<ProviderEntry>;
}) {
  const t = useT();
  const primaryModel = resolveProfileModelByTier("primary", profile, providers);
  const assistantModel = resolveProfileModelByTier("assistant", profile, providers);
  const liteModel = resolveProfileModelByTier("lite", profile, providers);

  const tiers: Array<{ label: string; model: { displayName: string; modelId: string } | null }> = [
    { label: t("composer.profileTier.primary"), model: primaryModel },
    { label: t("composer.profileTier.auxiliary"), model: assistantModel },
    { label: t("composer.profileTier.lightweight"), model: liteModel },
  ];

  return (
    <ModelSelectorItem onSelect={onSelect} value={profile.id}>
      <div className="min-w-0 flex flex-1 flex-col gap-2">
        {/* Profile header */}
        <ProfileInlineIdentity profile={profile} providers={providers} showModel={false} />
        {/* Three-tier model rows */}
        <div className="flex flex-col gap-1 pl-8">
          {tiers.map(({ label, model }) => (
            <div className="flex items-center gap-1.5" key={label}>
              <span className="w-[54px] shrink-0 text-[10px] font-medium text-app-subtle">{label}</span>
              {model ? (
                <>
                  <ModelBrandIcon
                    className="size-3 shrink-0 text-app-muted"
                    displayName={model.displayName}
                    modelId={model.modelId}
                  />
                  <span className="min-w-0 truncate text-[11px] text-app-muted">{model.displayName}</span>
                </>
              ) : (
                <span className="text-[11px] italic text-app-subtle/60">{t("composer.profileTier.notConfigured")}</span>
              )}
            </div>
          ))}
        </div>
      </div>
      {isActive ? <CheckIcon className="ml-auto size-4 shrink-0" /> : <span className="ml-auto size-4 shrink-0" />}
    </ModelSelectorItem>
  );
}

function RunModeToggle({
  disabled = false,
  onChange,
  runMode,
}: {
  disabled?: boolean;
  onChange: (mode: RunMode) => void;
  runMode: RunMode;
}) {
  const checked = runMode === "plan";

  return (
    <div className="inline-flex items-center gap-2">
      <Switch
        aria-label="Toggle plan mode"
        checked={checked}
        disabled={disabled}
        onCheckedChange={(nextChecked) => onChange(nextChecked ? "plan" : "default")}
        size="sm"
      />
      <span
        className={cn(
          "min-w-[6.75rem] text-left text-sm font-medium",
          checked ? "text-app-info" : "text-app-muted",
        )}
      >
        {checked ? "Plan mode" : "Default mode"}
      </span>
    </div>
  );
}

export function mapComposerAttachments(files: Array<FileUIPart>) {
  return files.map((file, index) => ({
    id: file.url || `${file.filename || "attachment"}-${index}`,
    mediaType: file.mediaType,
    name: file.filename?.trim() || `附件 ${index + 1}`,
    url: file.url,
  }));
}

export function ComposerMessageAttachments({
  attachments,
}: {
  attachments: ReadonlyArray<ComposerAttachment>;
}) {
  if (attachments.length === 0) {
    return null;
  }

  const imageAttachments = attachments.filter((a) => isImageAttachment(a));
  const fileAttachments = attachments.filter((a) => !isImageAttachment(a));

  return (
    <div className="mb-3 flex flex-col gap-2">
      {/* Image attachments — grid layout */}
      {imageAttachments.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {imageAttachments.map((attachment) => (
            <AttachmentCard attachment={attachment} key={attachment.id} />
          ))}
        </div>
      ) : null}
      {/* File attachments — inline chips */}
      {fileAttachments.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {fileAttachments.map((attachment) => (
            <AttachmentCard attachment={attachment} key={attachment.id} />
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function WorkbenchPromptComposer({
  activeAgentProfileId,
  agentProfiles,
  canSubmitWhenAttachmentsOnly = true,
  className,
  commands = [],
  composerShellClassName,
  enabledSkills = [],
  error,
  onErrorMessageChange,
  onRunModeChange = () => undefined,
  onSelectAgentProfile,
  onStop,
  onSubmit,
  placeholder,
  providers,
  runMode = "default",
  runModeDisabled = false,
  showRunModeToggle = false,
  status,
  suggestions,
  textareaClassName,
  value,
  workspaceId,
  onValueChange,
}: WorkbenchPromptComposerProps) {
  const t = useT();
  const [isProfileSelectorOpen, setProfileSelectorOpen] = useState(false);
  const [selectedCommandKey, setSelectedCommandKey] = useState<string | null>(null);
  const [selectedFileIndex, setSelectedFileIndex] = useState(0);
  const [selectedSkillIndex, setSelectedSkillIndex] = useState(0);
  const [fileSearchResults, setFileSearchResults] = useState<ReadonlyArray<FileFilterMatch>>([]);
  const [isFileSearchLoading, setFileSearchLoading] = useState(false);
  const [fileSearchError, setFileSearchError] = useState<string | null>(null);
  const [cursorPosition, setCursorPosition] = useState<number | null>(value.length);
  const [referencedFiles, setReferencedFiles] = useState<ReadonlyArray<ComposerReferencedFile>>([]);
  const [clearReferencedSourcesSignal, setClearReferencedSourcesSignal] = useState(0);
  const commandPanelRef = useRef<HTMLDivElement | null>(null);
  const filePanelRef = useRef<HTMLDivElement | null>(null);
  const skillPanelRef = useRef<HTMLDivElement | null>(null);
  const requestSequenceRef = useRef(0);
  const referencedSourceBridgeRef = useRef<ReferencedSourceBridgeHandle | null>(null);
  const registerReferencedSourceBridge = useCallback((bridge: ReferencedSourceBridgeHandle) => {
    referencedSourceBridgeRef.current = bridge;
  }, []);
  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );
  const canSwitchProfiles = Boolean(activeProfile);
  const commandRegistry = useMemo(
    () => buildComposerCommandRegistry(commands),
    [commands],
  );
  const slashActive = shouldShowCommandPicker(value);
  const mentionMatch = useMemo(
    () => getActiveMentionMatch(value, cursorPosition),
    [cursorPosition, value],
  );
  const mentionActive = !slashActive && Boolean(mentionMatch);
  const mentionQuery = mentionMatch?.query.trim() ?? "";
  const fileMentionActive = mentionActive && mentionMatch?.token === "@";
  const skillMentionActive = mentionActive && mentionMatch?.token === "$";
  const filteredSkills = useMemo(
    () => (skillMentionActive ? filterReferencedSkills(enabledSkills, mentionQuery) : []),
    [enabledSkills, mentionQuery, skillMentionActive],
  );
  const selectedSkillResult = skillMentionActive
    ? filteredSkills[selectedSkillIndex] ?? null
    : null;
  const filteredCommands = useMemo(
    () => getFilteredCommands(commandRegistry, value),
    [commandRegistry, value],
  );
  const selectedCommand = useMemo(() => {
    const keyedSelection = selectedCommandKey
      ? filteredCommands.find((command) => getCommandItemKey(command) === selectedCommandKey) ?? null
      : null;
    return getSelectedCommandFromFiltered(filteredCommands, keyedSelection);
  }, [filteredCommands, selectedCommandKey]);
  const selectedFileResult = fileMentionActive
    ? fileSearchResults[selectedFileIndex] ?? null
    : null;

  useEffect(() => {
    if (!slashActive) {
      setSelectedCommandKey(null);
      return;
    }

    if (!selectedCommand && filteredCommands.length > 0) {
      setSelectedCommandKey(getCommandItemKey(filteredCommands[0]));
    }
  }, [filteredCommands, selectedCommand, slashActive]);

  useEffect(() => {
    if (!slashActive || !selectedCommandKey) {
      return;
    }

    const frame = requestAnimationFrame(() => {
      const selectedItem = commandPanelRef.current?.querySelector<HTMLElement>(
        `[data-command-key="${selectedCommandKey}"]`,
      );
      selectedItem?.scrollIntoView({ block: "nearest" });
    });

    return () => cancelAnimationFrame(frame);
  }, [selectedCommandKey, slashActive]);

  useEffect(() => {
    setReferencedFiles((current) => removeUnmentionedReferencedFiles(current, value));
  }, [value]);

  useEffect(() => {
    referencedSourceBridgeRef.current?.syncFiles(referencedFiles);
  }, [referencedFiles]);

  useEffect(() => {
    if (!fileMentionActive) {
      setFileSearchResults([]);
      setFileSearchError(null);
      setFileSearchLoading(false);
      setSelectedFileIndex(0);
      return;
    }

    if (!workspaceId) {
      setFileSearchResults([]);
      setFileSearchError(null);
      setFileSearchLoading(false);
      setSelectedFileIndex(0);
      return;
    }

    if (!mentionQuery) {
      setFileSearchResults([]);
      setFileSearchError(null);
      setFileSearchLoading(false);
      setSelectedFileIndex(0);
      return;
    }

    const nextRequestId = requestSequenceRef.current + 1;
    requestSequenceRef.current = nextRequestId;
    setFileSearchLoading(true);
    setFileSearchError(null);

    const timer = window.setTimeout(() => {
      void indexFilterFiles(workspaceId, mentionQuery, FILE_SEARCH_RESULT_LIMIT)
        .then((response) => {
          if (requestSequenceRef.current !== nextRequestId) {
            return;
          }
          setFileSearchResults(response.results);
          setSelectedFileIndex(0);
          setFileSearchLoading(false);
        })
        .catch((searchError) => {
          if (requestSequenceRef.current !== nextRequestId) {
            return;
          }
          setFileSearchResults([]);
          setSelectedFileIndex(0);
          setFileSearchLoading(false);
          setFileSearchError(searchError instanceof Error ? searchError.message : t("composer.fileSearchError"));
        });
    }, FILE_SEARCH_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [fileMentionActive, mentionQuery, workspaceId]);

  useEffect(() => {
    if (!skillMentionActive) {
      setSelectedSkillIndex(0);
      return;
    }

    setSelectedSkillIndex(0);
  }, [mentionQuery, skillMentionActive]);

  useEffect(() => {
    if (!fileMentionActive || fileSearchResults.length === 0) {
      return;
    }

    const frame = requestAnimationFrame(() => {
      const selectedItem = filePanelRef.current?.querySelector<HTMLElement>(
        `[data-file-index="${selectedFileIndex}"]`,
      );
      selectedItem?.scrollIntoView({ block: "nearest" });
    });

    return () => cancelAnimationFrame(frame);
  }, [fileMentionActive, fileSearchResults, selectedFileIndex]);

  useEffect(() => {
    if (!skillMentionActive || filteredSkills.length === 0) {
      return;
    }

    const frame = requestAnimationFrame(() => {
      const selectedItem = skillPanelRef.current?.querySelector<HTMLElement>(
        `[data-skill-index="${selectedSkillIndex}"]`,
      );
      selectedItem?.scrollIntoView({ block: "nearest" });
    });

    return () => cancelAnimationFrame(frame);
  }, [filteredSkills, selectedSkillIndex, skillMentionActive]);

  const handlePromptSubmit = (message: PromptInputMessage) => {
    const submission = buildSubmissionFromPromptInput(message, commandRegistry, runMode, referencedFiles);
    onSubmit(submission);
    setReferencedFiles([]);
    setClearReferencedSourcesSignal((current) => current + 1);
  };

  const insertReferencedFile = (match: FileFilterMatch) => {
    if (!mentionMatch) {
      return;
    }

    const mentionText = `@${match.path}`;
    const nextValue = `${value.slice(0, mentionMatch.tokenStart)}${mentionText} ${value.slice(mentionMatch.tokenEnd)}`;
    const nextCursorPosition = mentionMatch.tokenStart + mentionText.length + 1;
    const referencedFile: ComposerReferencedFile = {
      name: match.name,
      path: match.path,
      parentPath: match.parentPath,
    };

    onValueChange(nextValue);
    setCursorPosition(nextCursorPosition);
    setReferencedFiles((current) => mergeReferencedFiles(current, referencedFile));
    setSelectedFileIndex(0);
    setFileSearchResults([]);
    setFileSearchError(null);
    setFileSearchLoading(false);
  };

  const insertReferencedSkill = (
    skill: Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">,
  ) => {
    if (!mentionMatch || mentionMatch.token !== "$") {
      return;
    }

    const mentionText = `$${skill.name}`;
    const nextValue = `${value.slice(0, mentionMatch.tokenStart)}${mentionText} ${value.slice(mentionMatch.tokenEnd)}`;
    const nextCursorPosition = mentionMatch.tokenStart + mentionText.length + 1;

    onValueChange(nextValue);
    setCursorPosition(nextCursorPosition);
    setSelectedSkillIndex(0);
  };

  const handleTextareaKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) {
      return;
    }

    if (mentionActive) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const resultCount = fileMentionActive ? fileSearchResults.length : filteredSkills.length;
        if (resultCount === 0) {
          return;
        }
        event.preventDefault();
        if (fileMentionActive) {
          setSelectedFileIndex((currentIndex) => getNextFileIndex(
            currentIndex,
            fileSearchResults.length,
            event.key === "ArrowDown" ? 1 : -1,
          ));
        } else if (skillMentionActive) {
          setSelectedSkillIndex((currentIndex) => getNextFileIndex(
            currentIndex,
            filteredSkills.length,
            event.key === "ArrowDown" ? 1 : -1,
          ));
        }
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        setCursorPosition(null);
        setFileSearchResults([]);
        setFileSearchError(null);
        setFileSearchLoading(false);
        setSelectedFileIndex(0);
        setSelectedSkillIndex(0);
        return;
      }

      if ((event.key === "Enter" || event.key === "Tab")) {
        if (fileMentionActive && selectedFileResult) {
          event.preventDefault();
          insertReferencedFile(selectedFileResult);
          return;
        }

        if (skillMentionActive && selectedSkillResult) {
          event.preventDefault();
          insertReferencedSkill(selectedSkillResult);
          return;
        }
      }
    }

    if (!slashActive) {
      return;
    }

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const currentIndex = findSelectedCommandIndex(filteredCommands, selectedCommand);
      const nextIndex = getNextCommandIndex(
        currentIndex,
        filteredCommands.length,
        event.key === "ArrowDown" ? 1 : -1,
      );
      const nextCommand = filteredCommands[nextIndex] ?? null;
      setSelectedCommandKey(nextCommand ? getCommandItemKey(nextCommand) : null);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      setSelectedCommandKey(null);
      return;
    }

    if ((event.key === "Enter" || event.key === "Tab") && selectedCommand) {
      // When the command is already fully resolved in the input (exact match),
      // Enter should submit instead of re-inserting the same command.
      const parsedActive = getParsedActiveCommand(value, commandRegistry);
      if (event.key === "Enter" && parsedActive?.command) {
        return;
      }

      event.preventDefault();
      const nextValue = buildCommandInputValue(selectedCommand);
      onValueChange(nextValue);
      setSelectedCommandKey(getCommandItemKey(selectedCommand));
      if (shouldAutoSubmitCommand(selectedCommand, nextValue)) {
        handlePromptSubmit({ text: nextValue, files: [] });
        onValueChange("");
      }
    }
  };

  return (
    <div className={cn("mx-auto flex max-w-4xl flex-col gap-3", className)}>
      {suggestions && suggestions.length > 0 ? (
        <Suggestions className="gap-2">
          {suggestions.map((suggestion) => (
            <Suggestion key={suggestion} onClick={(nextSuggestion) => onValueChange(nextSuggestion)} suggestion={suggestion} variant="secondary" />
          ))}
        </Suggestions>
      ) : null}

      <div className={cn(
        "[--composer-shell-border:1px] [--composer-shell-gap:6px] [--composer-shell-radius:26px] rounded-[var(--composer-shell-radius)] border border-app-border/60 bg-app-surface/82 p-[var(--composer-shell-gap)] shadow-[0_22px_50px_-42px_rgba(15,23,42,0.38)] backdrop-blur-sm",
        composerShellClassName,
      )}>
        <PromptInput
          accept={SUPPORTED_COMPOSER_ATTACHMENT_ACCEPT}
          className="[&_[data-slot=input-group]]:overflow-visible [&_[data-slot=input-group]]:rounded-[calc(var(--composer-shell-radius)-var(--composer-shell-border)-var(--composer-shell-gap))] [&_[data-slot=input-group]]:shadow-none [&_[data-slot=input-group]:focus-within]:!border-app-border/60 [&_[data-slot=input-group]:focus-within]:!ring-0"
          dialogFilters={SUPPORTED_COMPOSER_ATTACHMENT_DIALOG_FILTERS.map((filter) => ({
            extensions: [...filter.extensions],
            name: filter.name,
          }))}
          maxFileSize={10 * 1024 * 1024}
          maxFiles={4}
          onError={(nextError) => onErrorMessageChange?.(nextError.message)}
          onSubmit={handlePromptSubmit}
        >
          <PromptInputBody>
            <ComposerReferencedSourcesBridge
              clearSignal={clearReferencedSourcesSignal}
              onBridgeReady={registerReferencedSourceBridge}
            />
            <ComposerAttachmentStateSync
              onHasAttachmentsChange={(hasAttachments) => {
                if (hasAttachments) {
                  onErrorMessageChange?.(null);
                }
              }}
            />
            <ComposerAttachmentHeader />
            <ComposerReferencedFilesHeader
              files={referencedFiles}
              onRemove={(path) => {
                const nextFiles = referencedFiles.filter((file) => file.path !== path);
                setReferencedFiles(nextFiles);
                onValueChange(value.split(`@${path}`).join("").replace(/\s{2,}/g, " ").trimStart());
              }}
            />
            <div className="relative flex w-full items-start self-stretch">
              <PromptInputTextarea
                className={cn(
                  "block min-h-[88px] w-full self-stretch bg-transparent px-3 py-3 text-left align-top text-app-foreground caret-app-foreground selection:bg-app-info/20",
                  textareaClassName,
                )}
                onChange={(event) => {
                  onValueChange(event.currentTarget.value);
                  setCursorPosition(event.currentTarget.selectionStart ?? event.currentTarget.value.length);
                }}
                onClick={(event) => {
                  setCursorPosition(event.currentTarget.selectionStart ?? event.currentTarget.value.length);
                }}
                onKeyDown={handleTextareaKeyDown}
                onKeyUp={(event) => {
                  setCursorPosition(event.currentTarget.selectionStart ?? event.currentTarget.value.length);
                }}
                onSelect={(event) => {
                  setCursorPosition(event.currentTarget.selectionStart ?? event.currentTarget.value.length);
                }}
                placeholder={placeholder}
                value={value}
              />
            </div>
            {fileMentionActive ? (
              <div
                className="absolute inset-x-3 bottom-[calc(100%+0.5rem)] z-20 min-w-0"
                ref={filePanelRef}
              >
                <div className="w-full min-w-0 overflow-hidden rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/80 bg-app-menu p-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl dark:bg-app-menu/98">
                  <div className="max-h-[320px] overflow-y-auto">
                    {!workspaceId ? (
                      <div className="px-3 py-3 text-sm text-app-subtle">{t("composer.noWorkspaceBound")}</div>
                    ) : !mentionQuery ? (
                      <div className="px-3 py-3 text-sm text-app-subtle">{t("composer.typeFileNameHint")}</div>
                    ) : isFileSearchLoading ? (
                      <div className="flex items-center gap-2 px-3 py-3 text-sm text-app-subtle">
                        <LoaderCircle className="size-4 animate-spin" />
                        <span>{t("composer.searchingFiles")}</span>
                      </div>
                    ) : fileSearchError ? (
                      <div className="px-3 py-3 text-sm text-app-danger">{fileSearchError}</div>
                    ) : fileSearchResults.length === 0 ? (
                      <div className="px-3 py-3 text-sm text-app-subtle">{t("composer.noMatchingFiles")}</div>
                    ) : (
                      <div className="flex flex-col gap-1">
                        {fileSearchResults.map((match, index) => {
                          const isSelected = index === selectedFileIndex;
                          return (
                            <button
                              className={cn(
                                "flex w-full items-start gap-3 rounded-xl px-3 py-2 text-left transition-colors",
                                isSelected
                                  ? "bg-app-info/14 text-app-foreground dark:bg-app-info/18"
                                  : "text-app-foreground/90 hover:bg-app-accent/8",
                              )}
                              data-file-index={index}
                              key={match.path}
                              onClick={() => insertReferencedFile(match)}
                              onMouseEnter={() => {
                                if (selectedFileIndex !== index) {
                                  setSelectedFileIndex(index);
                                }
                              }}
                              onMouseDown={(event) => event.preventDefault()}
                              type="button"
                            >
                              <span className={cn(
                                "mt-0.5 inline-flex size-8 shrink-0 items-center justify-center rounded-lg border",
                                isSelected
                                  ? "border-app-info/30 bg-app-info/10 text-app-info"
                                  : "border-app-border/55 bg-app-surface-muted/70 text-app-subtle",
                              )}>
                                <FileSearchIcon className="size-4" />
                              </span>
                              <span className="min-w-0 flex-1">
                                <span className="block truncate text-sm font-medium text-inherit">{match.name}</span>
                                <span className={cn(
                                  "mt-1 block truncate text-[11px]",
                                  isSelected ? "text-app-foreground/75" : "text-app-subtle",
                                )}>
                                  {match.path}
                                </span>
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            ) : null}
            {skillMentionActive ? (
              <div
                className="absolute inset-x-3 bottom-[calc(100%+0.5rem)] z-20 min-w-0"
                ref={skillPanelRef}
              >
                <div className="w-full min-w-0 overflow-hidden rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/80 bg-app-menu p-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl dark:bg-app-menu/98">
                  <div className="max-h-[320px] overflow-y-auto">
                    {!mentionQuery ? (
                      <div className="px-3 py-3 text-sm text-app-subtle">{t("composer.typeSkillHint")}</div>
                    ) : filteredSkills.length === 0 ? (
                      <div className="px-3 py-3 text-sm text-app-subtle">{t("composer.noMatchingSkills")}</div>
                    ) : (
                      <div className="flex flex-col gap-1">
                        {filteredSkills.map((skill, index) => {
                          const isSelected = index === selectedSkillIndex;
                          const summary = skill.description?.trim() || skill.contentPreview?.trim() || skill.name;
                          return (
                            <button
                              className={cn(
                                "flex w-full items-start gap-3 rounded-xl px-3 py-2 text-left transition-colors",
                                isSelected
                                  ? "bg-app-info/14 text-app-foreground dark:bg-app-info/18"
                                  : "text-app-foreground/90 hover:bg-app-accent/8",
                              )}
                              data-skill-index={index}
                              key={skill.id}
                              onClick={() => insertReferencedSkill(skill)}
                              onMouseEnter={() => {
                                if (selectedSkillIndex !== index) {
                                  setSelectedSkillIndex(index);
                                }
                              }}
                              onMouseDown={(event) => event.preventDefault()}
                              type="button"
                            >
                              <span className={cn(
                                "mt-0.5 inline-flex h-8 shrink-0 items-center justify-center rounded-lg border px-2 text-[11px] font-medium",
                                isSelected
                                  ? "border-app-info/30 bg-app-info/10 text-app-info"
                                  : "border-app-border/55 bg-app-surface-muted/70 text-app-subtle",
                              )}>
                                $SKILL
                              </span>
                              <span className="min-w-0 flex-1">
                                <span className="flex min-w-0 items-center gap-2">
                                  <span className="truncate text-sm font-medium text-inherit">{skill.name}</span>
                                  <span className={cn(
                                    "shrink-0 text-[11px]",
                                    isSelected ? "text-app-foreground/75" : "text-app-subtle",
                                  )}>
                                    {skill.scope}
                                  </span>
                                </span>
                                <span className={cn(
                                  "mt-1 block line-clamp-2 text-[11px] leading-5",
                                  isSelected ? "text-app-foreground/75" : "text-app-subtle",
                                )}>
                                  {summary}
                                </span>
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            ) : null}
            {slashActive && filteredCommands.length > 0 ? (
              <div
                className="absolute inset-x-3 bottom-[calc(100%+0.5rem)] z-20 min-w-0"
                ref={commandPanelRef}
              >
                <PromptInputCommand
                  className="w-full min-w-0 overflow-hidden rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/80 bg-app-menu p-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)] backdrop-blur-xl dark:bg-app-menu/98"
                  value={selectedCommand ? getCommandDisplayPath(selectedCommand) : ""}
                  onValueChange={() => {/* controlled by selectedCommandKey state */}}
                  disablePointerSelection
                >
                  <PromptInputCommandList className="w-full min-w-0 max-h-[320px]">
                    {["builtin", "settings"].map((source) => {
                      const groupCommands = filteredCommands.filter((command) => command.source === source);
                      if (groupCommands.length === 0) {
                        return null;
                      }

                      return (
                        <PromptInputCommandGroup
                          className="w-full min-w-0 p-1"
                          key={source}
                        >
                          {groupCommands.map((command) => {
                            const isSelected = selectedCommand ? getCommandItemKey(selectedCommand) === getCommandItemKey(command) : false;
                            const commandKey = getCommandItemKey(command);
                            return (
                              <PromptInputCommandItem
                                className={cn(
                                  "w-full items-start gap-0 overflow-hidden rounded-xl px-3 py-2 text-left transition-colors data-[selected=true]:!bg-app-info/14 data-[selected=true]:!text-app-foreground dark:data-[selected=true]:!bg-app-info/18",
                                  !isSelected && "text-app-foreground/90",
                                )}
                                data-command-key={commandKey}
                                key={commandKey}
                                onFocus={() => {
                                  if (selectedCommandKey !== commandKey) {
                                    setSelectedCommandKey(commandKey);
                                  }
                                }}
                                onMouseDown={(event) => event.preventDefault()}
                                onMouseMove={() => {
                                  if (selectedCommandKey !== commandKey) {
                                    setSelectedCommandKey(commandKey);
                                  }
                                }}
                                onSelect={() => {
                                  const nextValue = getCommandCompletionValue(command);
                                  onValueChange(nextValue);
                                  setSelectedCommandKey(commandKey);
                                  if (shouldAutoSubmitCommand(command, nextValue)) {
                                    handlePromptSubmit({ text: nextValue, files: [] });
                                    onValueChange("");
                                  }
                                }}
                                value={getCommandDisplayPath(command)}
                              >
                                <div className="flex min-w-0 w-full flex-col gap-1">
                                  <div className="flex min-w-0 items-center gap-2">
                                    <span
                                      className="shrink-0 text-sm font-medium text-inherit"
                                      title={getCommandDisplayPath(command)}
                                    >
                                      {getCommandDisplayPath(command)}
                                    </span>
                                    {command.argumentHint ? (
                                      <span
                                        className={cn(
                                          "min-w-0 flex-1 truncate text-[11px]",
                                          isSelected ? "text-app-foreground/75" : "text-app-subtle",
                                        )}
                                        title={command.argumentHint}
                                      >
                                        {command.argumentHint}
                                      </span>
                                    ) : null}
                                  </div>
                                  <p
                                    className={cn(
                                      "truncate text-[11px] leading-5",
                                      isSelected ? "text-app-foreground/75" : "text-app-subtle",
                                    )}
                                    title={command.description || getCommandDisplayPath(command)}
                                  >
                                    {command.description || getCommandDisplayPath(command)}
                                  </p>
                                </div>
                              </PromptInputCommandItem>
                            );
                          })}
                        </PromptInputCommandGroup>
                      );
                    })}
                  </PromptInputCommandList>
                </PromptInputCommand>
              </div>
            ) : null}
          </PromptInputBody>
          <PromptInputFooter>
            <PromptInputTools>
              <ComposerAttachmentTrigger />

              {activeProfile ? (
                <div className="flex items-center gap-2">
                  {canSwitchProfiles ? (
                    <ModelSelector onOpenChange={setProfileSelectorOpen} open={isProfileSelectorOpen}>
                      <ModelSelectorTrigger asChild>
                        <PromptInputButton className="h-auto max-w-[360px] justify-start gap-3 px-3 py-2" size="sm">
                          <ProfileInlineIdentity badge={false} profile={activeProfile} providers={providers} showModel={false} />
                        </PromptInputButton>
                      </ModelSelectorTrigger>
                      <ModelSelectorContent
                        commandProps={{ value: activeAgentProfileId ?? undefined }}
                        showCloseButton={false}
                        title="Profile Selector"
                      >
                        <ModelSelectorList>
                          <ModelSelectorEmpty>{t("composer.noProfileAvailable")}</ModelSelectorEmpty>
                          <ModelSelectorGroup heading="Agent Profiles">
                            {agentProfiles.map((profile) => (
                              <ProfileSelectorItem
                                isActive={profile.id === activeAgentProfileId}
                                key={profile.id}
                                onSelect={() => {
                                  onSelectAgentProfile(profile.id);
                                  setProfileSelectorOpen(false);
                                }}
                                profile={profile}
                                providers={providers}
                              />
                            ))}
                          </ModelSelectorGroup>
                        </ModelSelectorList>
                      </ModelSelectorContent>
                    </ModelSelector>
                  ) : (
                    <PromptInputButton className="h-auto max-w-[360px] justify-start gap-3 px-3 py-2" disabled size="sm">
                      <ProfileInlineIdentity badge={false} muted profile={activeProfile} providers={providers} showModel={false} />
                    </PromptInputButton>
                  )}

                  {showRunModeToggle ? (
                    <>
                      <span aria-hidden="true" className="h-4 w-px bg-app-border/55" />
                      <RunModeToggle
                        disabled={runModeDisabled}
                        onChange={onRunModeChange}
                        runMode={runMode}
                      />
                    </>
                  ) : null}
                </div>
              ) : null}
            </PromptInputTools>

            <PromptInputSubmitButton
              activeProfile={activeProfile}
              allowAttachmentsOnly={canSubmitWhenAttachmentsOnly}
              composerValue={value}
              onStop={onStop}
              status={status}
            />
          </PromptInputFooter>
        </PromptInput>
      </div>

      {error ? <p className="mt-2 text-xs text-app-danger">{error}</p> : null}
      {!activeProfile ? (
        <p className="mt-2 text-xs text-app-danger">No active profile is available for the composer right now.</p>
      ) : null}
    </div>
  );
}
