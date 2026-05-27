import type { ChatStatus, FileUIPart, SourceDocumentUIPart } from "ai";
import {
  Brain,
  BracesIcon,
  Bot,
  CheckCircle2,
  CheckIcon,
  FileCodeIcon,
  FileIcon,
  FileSearchIcon,
  FileTextIcon,
  ImageIcon,
  ListEnd,
  ListStart,
  LoaderCircle,
  PaperclipIcon,
  Settings,
  UserStar,
  XIcon,
} from "lucide-react";
import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode, type SyntheticEvent } from "react";
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
  PromptInputSelect,
  PromptInputSelectContent,
  PromptInputSelectItem,
  PromptInputSelectTrigger,
  PromptInputSelectValue,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
  usePromptInputAttachments,
  usePromptInputReferencedSources,
} from "@/components/ai-elements/prompt-input";
import { Suggestion, Suggestions } from "@/components/ai-elements/suggestion";
import {
  buildCommandEffectivePrompt,
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
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import type { AgentProfile, CommandEntry, CustomSubagent, ProviderEntry } from "@/modules/settings-center/model/types";
import type { updateAgentProfile as updateAgentProfileAction } from "@/modules/settings-center/model/settings-ipc-actions";
import { sortAgentProfilesByName } from "@/modules/settings-center/model/profile-utils";
import { profileSubagentAccessGet } from "@/services/bridge/subagent-commands";
import type { SkillRecord } from "@/shared/types/extensions";
import type { RunMode, RuntimeQueueMessageKind } from "@/shared/types/api";
import { indexFilterFiles, type FileFilterMatch } from "@/services/bridge";
import type { SerializableAttachment } from "@/modules/workbench-shell/model/composer-store";
import { useT } from "@/i18n";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { ModelBrandIcon } from "@/shared/ui/model-brand-icon";

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
  allowMissingActiveProfile?: boolean;
  canSubmitWhenAttachmentsOnly?: boolean;
  canSubmitWhileRunning?: boolean;
  className?: string;
  commands?: ReadonlyArray<CommandEntry>;
  composerShellClassName?: string;
  customSubagents?: ReadonlyArray<CustomSubagent>;
  enabledSkills?: ReadonlyArray<Pick<SkillRecord, "id" | "name" | "description" | "scope" | "source" | "tags" | "triggers" | "contentPreview">>;
  error?: string | null;
  onErrorMessageChange?: (message: string | null) => void;
  onOpenProfileSettings?: () => void;
  onRuntimeQueueSubmitModeChange?: (mode: RuntimeQueueMessageKind) => void;
  onSelectAgentProfile: (id: string) => void;
  onUpdateAgentProfile?: typeof updateAgentProfileAction;
  onStop: () => void;
  onSubmit: (submission: ComposerSubmission) => void | Promise<void>;
  placeholder: string;
  providers: ReadonlyArray<ProviderEntry>;
  runtimeQueueSubmitMode?: RuntimeQueueMessageKind | null;
  showRuntimeQueueSubmitMode?: boolean;
  status: ChatStatus;
  suggestions?: ReadonlyArray<string>;
  textareaClassName?: string;
  value: string;
  workspaceId?: string | null;
  onValueChange: (value: string) => void;
  /** Pre-populate @file references on mount (restored from draft). */
  initialReferencedFiles?: ReadonlyArray<ComposerReferencedFile>;
  /** Pre-populate attachments on mount (restored from serialized draft). */
  initialAttachmentData?: ReadonlyArray<SerializableAttachment>;
  /** Increment to restore local composer state from current initial* props. */
  restoreSignal?: number;
  /** Increment to clear all local composer state (attachments, referenced files, etc.). */
  clearSignal?: number;
  /** Callback to persist referenced files back to the draft store. */
  onReferencedFilesChange?: (files: ReadonlyArray<ComposerReferencedFile>) => void;
  /** Callback to persist serialized attachment data back to the draft store. */
  onAttachmentDataChange?: (data: ReadonlyArray<SerializableAttachment>) => void;
};

function getFileExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/**
 * Text length above which slash-command parsing and mention scanning are
 * short-circuited. No realistic slash command or mention query reaches this
 * size, so skipping avoids expensive O(n) string copies on large pastes.
 */
const LARGE_TEXT_THRESHOLD = 10_240;

function isSlashCommandActive(value: string) {
  if (value.length > LARGE_TEXT_THRESHOLD) {
    return false;
  }
  // Find the first non-whitespace character without allocating a trimmed copy.
  for (let i = 0; i < value.length; i++) {
    const ch = value.charCodeAt(i);
    // space, tab, newline, carriage-return
    if (ch === 32 || ch === 9 || ch === 10 || ch === 13) {
      continue;
    }
    return ch === 47; // '/'
  }
  return false;
}

function buildSubmissionFromPromptInput(
  message: PromptInputMessage,
  registry: ReadonlyArray<ComposerCommandDescriptor>,
  runMode: RunMode,
  referencedFiles: ReadonlyArray<ComposerReferencedFile>,
): ComposerSubmission {
  const rawText = message.text ?? "";
  const trimmedText = rawText.trim();
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
    const effectivePrompt = rawText;
    return {
      kind: "plain",
      displayText: rawText,
      effectivePrompt,
      rawMessage: message,
      attachments,
      metadata: referencedFilesMetadata.length > 0
        ? {
            composer: {
              kind: "plain",
              displayText: rawText,
              effectivePrompt,
              referencedFiles: referencedFilesMetadata,
            },
          }
        : null,
      runMode,
    };
  }

  const effectivePrompt = buildCommandEffectivePrompt(parsedCommand.command, parsedCommand.argumentsText);

  return {
    kind: "command",
    displayText: rawText,
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
        displayText: rawText,
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

/**
 * Maximum number of characters before the cursor to scan for mention triggers.
 * Limits string operations to a small window instead of the full value, which
 * avoids O(n) scans when large text (hundreds of KB) is pasted.
 */
const MENTION_SCAN_WINDOW = 500;

function getActiveMentionMatch(value: string, cursorPosition: number | null): MentionMatch | null {
  if (cursorPosition == null) {
    return null;
  }

  const safeCursor = Math.max(0, Math.min(cursorPosition, value.length));
  const scanStart = Math.max(0, safeCursor - MENTION_SCAN_WINDOW);
  const window = value.slice(scanStart, safeCursor);

  const triggers: Array<"@" | "$"> = ["@", "$"];
  let tokenStart = -1;
  let token: "@" | "$" | null = null;

  for (const candidate of triggers) {
    const windowIndex = window.lastIndexOf(candidate);
    if (windowIndex >= 0) {
      const originalIndex = scanStart + windowIndex;
      if (originalIndex > tokenStart) {
        tokenStart = originalIndex;
        token = candidate;
      }
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
  if (current.length === 0) {
    return current;
  }
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

/**
 * Convert a base64 data URL back into a File object for re-attaching.
 */
function dataUrlToFile(dataUrl: string, name: string, mediaType: string): File {
  const parts = dataUrl.split(",");
  const mime = parts[0]?.match(/:(.*?);/)?.[1] ?? mediaType;
  const bstr = atob(parts[1] ?? "");
  const n = bstr.length;
  const u8arr = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    u8arr[i] = bstr.charCodeAt(i);
  }
  return new File([u8arr], name, { type: mime });
}

export type ComposerInitialAttachmentRestoreState = {
  hasRestored: boolean;
};

/**
 * Marks initial attachment restoration as attempted and returns whether the
 * current initial data should be restored. This intentionally mutates `state`
 * on the first call, including empty initial data, so later draft-sync updates
 * from the same mounted composer are not treated as initial state.
 */
export const markAndShouldRestoreInitialAttachmentData = (
  state: ComposerInitialAttachmentRestoreState,
  initialAttachmentData: ReadonlyArray<SerializableAttachment> | undefined,
): initialAttachmentData is ReadonlyArray<SerializableAttachment> => {
  if (state.hasRestored) {
    return false;
  }

  state.hasRestored = true;
  return Boolean(initialAttachmentData && initialAttachmentData.length > 0);
};

/**
 * Restores initial attachment data on mount by converting data URLs
 * into File objects and calling the PromptInput add() API.
 */
function ComposerInitialStateRestorer({
  initialAttachmentData,
}: {
  initialAttachmentData: ReadonlyArray<SerializableAttachment> | undefined;
}) {
  const attachments = usePromptInputAttachments();
  const restoreStateRef = useRef<ComposerInitialAttachmentRestoreState>({
    hasRestored: false,
  });

  useEffect(() => {
    if (
      !markAndShouldRestoreInitialAttachmentData(
        restoreStateRef.current,
        initialAttachmentData,
      )
    ) {
      return;
    }

    const files: File[] = [];
    for (const entry of initialAttachmentData) {
      try {
        files.push(dataUrlToFile(entry.dataUrl, entry.name, entry.mediaType));
      } catch {
        // Silently skip corrupted attachment data.
      }
    }

    if (files.length > 0) {
      attachments.add(files);
    }
  }, [attachments, initialAttachmentData]);

  return null;
}

/**
 * Clears PromptInput attachments when `signal` changes (non-zero initial value).
 */
function ComposerClearSignalHandler({
  signal,
}: {
  signal: number;
}) {
  const attachments = usePromptInputAttachments();
  const prevSignalRef = useRef(signal);

  useEffect(() => {
    if (prevSignalRef.current !== signal && prevSignalRef.current !== 0) {
      attachments.clear();
    }
    prevSignalRef.current = signal;
  }, [attachments, signal]);

  return null;
}

/**
 * Serializes PromptInput attachments to serializable data URLs and
 * reports them via `onChange` for draft persistence.
 */
function ComposerAttachmentDraftSync({
  onChange,
}: {
  onChange: (data: ReadonlyArray<SerializableAttachment>) => void;
}) {
  const attachments = usePromptInputAttachments();
  const syncedRef = useRef<string>("");

  useEffect(() => {
    // We use a snapshot-based approach: only sync when the files array
    // reference changes, not on every render.
    const { files } = attachments;
    const key = files.map((f) => f.id).join(",");
    if (key === syncedRef.current) {
      return;
    }
    syncedRef.current = key;

    if (files.length === 0) {
      onChange([]);
      return;
    }

    // Serialize each attachment to a data URL.
    void (async () => {
      const serialized: SerializableAttachment[] = [];
      for (const file of files) {
        try {
          let dataUrl = file.url;
          // If the URL is a blob:, fetch and convert to data URL.
          if (dataUrl.startsWith("blob:")) {
            const response = await fetch(dataUrl);
            const blob = await response.blob();
            dataUrl = await new Promise<string>((resolve, reject) => {
              const reader = new FileReader();
              reader.onloadend = () => resolve(reader.result as string);
              reader.onerror = reject;
              reader.readAsDataURL(blob);
            });
          }
          serialized.push({
            id: file.id,
            name: file.filename ?? "attachment",
            mediaType: file.mediaType ?? "application/octet-stream",
            dataUrl,
          });
        } catch {
          // Silently skip files that can't be serialized.
        }
      }
      onChange(serialized);
    })();
  }, [attachments, onChange]);

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

/**
 * O(1) fast-path check for whether a string contains any non-whitespace
 * character. Falls back to a full scan only when both the first and last
 * characters are whitespace (extremely rare for real user input).
 */
function hasNonWhitespace(s: string): boolean {
  if (s.length === 0) return false;
  if (s.charCodeAt(0) > 32 || s.charCodeAt(s.length - 1) > 32) return true;
  for (let i = 0; i < s.length; i++) {
    if (s.charCodeAt(i) > 32) return true;
  }
  return false;
}

function PromptInputSubmitButton({
  activeProfile,
  allowAttachmentsOnly,
  canSubmitWhileRunning = false,
  composerValue,
  hasMissingActiveProfile = false,
  onStop,
  status,
}: {
  activeProfile: AgentProfile | null;
  allowAttachmentsOnly: boolean;
  canSubmitWhileRunning?: boolean;
  composerValue: string;
  hasMissingActiveProfile?: boolean;
  onStop: () => void;
  status: ChatStatus;
}) {
  const attachments = usePromptInputAttachments();
  const hasText = hasNonWhitespace(composerValue);
  const hasAttachments = attachments.files.length > 0;
  const isStopping = status === "submitted" || status === "streaming";
  const canSubmit = Boolean(activeProfile) && !hasMissingActiveProfile && (
    canSubmitWhileRunning
      ? hasText && !hasAttachments
      : hasText || (allowAttachmentsOnly && hasAttachments)
  );
  const shouldSubmitWhileRunning = isStopping && canSubmitWhileRunning && hasText && !hasAttachments;
  const effectiveStatus = shouldSubmitWhileRunning ? "ready" : status;
  const isEffectiveStopping = effectiveStatus === "submitted" || effectiveStatus === "streaming";

  return <PromptInputSubmit disabled={isEffectiveStopping ? false : !canSubmit} onStop={onStop} status={effectiveStatus} />;
}

function RuntimeQueueModeSelect({
  mode,
  onModeChange,
}: {
  mode: RuntimeQueueMessageKind;
  onModeChange: (mode: RuntimeQueueMessageKind) => void;
}) {
  const t = useT();
  const isSteer = mode === "steer";

  return (
    <PromptInputSelect value={mode} onValueChange={(value) => onModeChange(value as RuntimeQueueMessageKind)}>
      <PromptInputSelectTrigger
        aria-label={t("composer.queueMode.ariaLabel")}
        className={cn(
          "h-8 rounded-full border-none bg-transparent px-1 text-xs shadow-none [&>svg:last-child]:hidden",
          "hover:bg-app-surface-hover",
          isSteer
            ? "text-app-warning hover:text-app-warning aria-expanded:text-app-warning"
            : "text-app-info hover:text-app-info aria-expanded:text-app-info",
        )}
        size="sm"
        title={t("composer.queueMode.tooltip")}
      >
        <PromptInputSelectValue>
          <span className="flex items-center">
            {isSteer ? <ListStart className="size-3.5" /> : <ListEnd className="size-3.5" />}
          </span>
        </PromptInputSelectValue>
      </PromptInputSelectTrigger>
      <PromptInputSelectContent align="end" className="min-w-[260px]">
        <div className="px-3 pb-1.5 pt-2">
          <span className="text-xs font-medium text-app-subtle">{t("composer.queueMode.panelTitle")}</span>
        </div>
        <PromptInputSelectItem value="steer">
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="flex items-center gap-2 font-medium text-app-foreground">
              <ListStart className="size-3.5 text-app-warning" />
              {t("composer.queueMode.steerLabel")}
            </span>
            <span className="text-[11px] text-app-subtle">{t("composer.queueMode.steerDesc")}</span>
          </span>
        </PromptInputSelectItem>
        <PromptInputSelectItem value="follow_up">
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="flex items-center gap-2 font-medium text-app-foreground">
              <ListEnd className="size-3.5 text-app-info" />
              {t("composer.queueMode.followUpLabel")}
            </span>
            <span className="text-[11px] text-app-subtle">{t("composer.queueMode.followUpDesc")}</span>
          </span>
        </PromptInputSelectItem>
      </PromptInputSelectContent>
    </PromptInputSelect>
  );
}

function ProfileInlineIdentity({
  badge = true,
  muted = false,
  profile,
  providers,
}: {
  badge?: boolean;
  muted?: boolean;
  profile: AgentProfile;
  providers: ReadonlyArray<ProviderEntry>;
}) {
  const t = useT();
  const modelId = getProfilePrimaryModelId(profile, providers) || profile.name;
  const modelLabel = getProfilePrimaryModelLabel(profile, providers);
  const displayModelLabel = modelLabel || modelId || profile.name;
  const thinkingLevelLabel = getThinkingLevelLabel(profile.thinkingLevel, t);

  return (
    <div className="flex min-w-0 items-center gap-2">
      <span
        className={cn(
          "flex shrink-0 items-center justify-center",
          badge ? "size-6 rounded-lg bg-app-surface-muted/75 ring-1 ring-app-border/45" : "size-4 rounded-none bg-transparent ring-0",
          muted && "opacity-80",
        )}
      >
        <ModelBrandIcon
          className={cn("size-3.5 shrink-0", muted ? "text-app-muted" : "text-app-foreground")}
          displayName={displayModelLabel}
          modelId={modelId}
        />
      </span>
      <span className="flex min-w-0 flex-col items-start gap-0.5">
        <span
          className={cn("min-w-0 max-w-full truncate text-sm font-semibold leading-none", muted ? "text-app-foreground/78" : "text-app-foreground")}
          title={profile.name}
        >
          {profile.name}
        </span>
        <span
          className={cn("inline-flex max-w-full items-center gap-1 text-[11px] font-normal leading-none", muted ? "text-app-subtle" : "text-app-muted")}
          title={`${displayModelLabel} · ${thinkingLevelLabel}`}
        >
          <span className="min-w-0 truncate">{displayModelLabel}</span>
          <span aria-hidden="true" className="shrink-0 text-app-subtle">·</span>
          <span className="shrink-0">{thinkingLevelLabel}</span>
        </span>
      </span>
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
  const primaryModelId = getProfilePrimaryModelId(profile, providers) || profile.name;
  const primaryModelLabel = getProfilePrimaryModelLabel(profile, providers);
  const displayPrimaryModelLabel = primaryModelLabel || primaryModelId || profile.name;

  return (
    <ModelSelectorItem
      className="items-center gap-3 rounded-lg px-3 py-2"
      onSelect={onSelect}
      value={profile.id}
    >
      <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-app-surface-muted/70 text-app-foreground ring-1 ring-app-border/45">
        <UserStar className="size-3.5" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium text-app-foreground">{profile.name}</div>
        <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-app-muted">
          <ModelBrandIcon
            className="size-3.5 shrink-0 text-app-muted"
            displayName={displayPrimaryModelLabel}
            modelId={primaryModelId}
          />
          <span className="min-w-0 truncate">{displayPrimaryModelLabel}</span>
        </div>
      </div>
      {isActive ? <CheckIcon className="size-4 shrink-0 text-app-info" /> : <span className="size-4 shrink-0" />}
    </ModelSelectorItem>
  );
}

export type ProfileModelTier = "primary" | "assistant" | "lite";
type ProfileQuickEditPatch = Parameters<typeof updateAgentProfileAction>[1];
export type AvailableProfileModelOption = {
  providerId: string;
  providerName: string;
  modelRecordId: string;
  modelId: string;
  displayName: string;
};

const THINKING_LEVEL_OPTIONS: ReadonlyArray<AgentProfile["thinkingLevel"]> = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
];

function getProfileTierProviderId(profile: AgentProfile, tier: ProfileModelTier) {
  if (tier === "primary") {
    return profile.primaryProviderId;
  }
  if (tier === "assistant") {
    return profile.assistantProviderId;
  }
  return profile.liteProviderId;
}

function getProfileTierModelId(profile: AgentProfile, tier: ProfileModelTier) {
  if (tier === "primary") {
    return profile.primaryModelId;
  }
  if (tier === "assistant") {
    return profile.assistantModelId;
  }
  return profile.liteModelId;
}

export function getProfileTierPatch(
  tier: ProfileModelTier,
  providerId: string,
  modelRecordId: string,
): ProfileQuickEditPatch {
  if (tier === "primary") {
    return { primaryProviderId: providerId, primaryModelId: modelRecordId };
  }
  if (tier === "assistant") {
    return { assistantProviderId: providerId, assistantModelId: modelRecordId };
  }
  return { liteProviderId: providerId, liteModelId: modelRecordId };
}

export function getAvailableProfileModelOptions(
  providers: ReadonlyArray<ProviderEntry>,
): Array<AvailableProfileModelOption> {
  return providers.flatMap((provider) => {
    if (!provider.enabled) {
      return [];
    }

    return provider.models
      .filter((model) => model.enabled)
      .map((model) => ({
        providerId: provider.id,
        providerName: provider.displayName,
        modelRecordId: model.id,
        modelId: model.modelId,
        displayName: model.displayName || model.modelId,
      }));
  });
}

const MODEL_SELECT_NONE_VALUE = "__none__";

export function getModelSelectValue(providerId: string, modelRecordId: string) {
  if (!providerId || !modelRecordId) {
    return MODEL_SELECT_NONE_VALUE;
  }

  return JSON.stringify([providerId, modelRecordId]);
}

export function parseModelSelectValue(value: string) {
  if (value === MODEL_SELECT_NONE_VALUE) {
    return { providerId: "", modelRecordId: "" };
  }

  try {
    const parsed = JSON.parse(value) as unknown;
    if (
      Array.isArray(parsed) &&
      parsed.length === 2 &&
      typeof parsed[0] === "string" &&
      typeof parsed[1] === "string"
    ) {
      return { providerId: parsed[0], modelRecordId: parsed[1] };
    }
  } catch {
    // Fall through to the empty selection below.
  }

  return { providerId: "", modelRecordId: "" };
}

function getShortModelId(modelId: string) {
  return modelId.split("/").pop() || modelId;
}

function getResponseStyleLabel(responseStyle: AgentProfile["responseStyle"], t: ReturnType<typeof useT>) {
  if (responseStyle === "concise") {
    return t("composer.responseStyle.concise");
  }
  if (responseStyle === "guide") {
    return t("composer.responseStyle.guide");
  }
  return t("composer.responseStyle.balanced");
}

function getThinkingLevelLabel(thinkingLevel: AgentProfile["thinkingLevel"], t: ReturnType<typeof useT>) {
  if (thinkingLevel === "minimal") return t("settings.thinkingLevel.minimal");
  if (thinkingLevel === "low") return t("settings.thinkingLevel.low");
  if (thinkingLevel === "medium") return t("settings.thinkingLevel.medium");
  if (thinkingLevel === "high") return t("settings.thinkingLevel.high");
  if (thinkingLevel === "xhigh") return t("settings.thinkingLevel.xhigh");
  return t("settings.thinkingLevel.off");
}

function getThinkingLevelDescription(thinkingLevel: AgentProfile["thinkingLevel"], t: ReturnType<typeof useT>) {
  if (thinkingLevel === "minimal") return t("settings.thinkingLevel.minimalDesc");
  if (thinkingLevel === "low") return t("settings.thinkingLevel.lowDesc");
  if (thinkingLevel === "medium") return t("settings.thinkingLevel.mediumDesc");
  if (thinkingLevel === "high") return t("settings.thinkingLevel.highDesc");
  if (thinkingLevel === "xhigh") return t("settings.thinkingLevel.xhighDesc");
  return t("settings.thinkingLevel.offDesc");
}

function ProfileDetailCard({
  className,
  icon,
  label,
  value,
  description,
}: {
  className?: string;
  icon?: ReactNode;
  label: string;
  value: string;
  description?: string;
}) {
  return (
    <div className={cn("min-w-0 rounded-xl border border-app-border/65 bg-app-surface-muted/55 px-3 py-2.5", className)}>
      <div className="flex min-w-0 items-center gap-2">
        {icon ? <span className="shrink-0 text-app-info">{icon}</span> : null}
        <span className="truncate text-[10px] font-medium uppercase tracking-[0.12em] text-app-subtle">{label}</span>
      </div>
      <p className="mt-1 truncate text-[12px] font-semibold text-app-foreground">{value}</p>
      {description ? (
        <p className="mt-0.5 line-clamp-2 text-[11px] leading-4 text-app-muted">{description}</p>
      ) : null}
    </div>
  );
}

type QuickThinkingLevelControlProps = {
  disabled: boolean;
  isPending: boolean;
  onChange: (value: AgentProfile["thinkingLevel"]) => void;
  value: AgentProfile["thinkingLevel"];
};

function QuickThinkingLevelControl({
  disabled,
  isPending,
  onChange,
  value,
}: QuickThinkingLevelControlProps) {
  const t = useT();
  const selectedIndex = THINKING_LEVEL_OPTIONS.indexOf(value);

  return (
    <div className="rounded-xl border border-app-border/65 bg-app-surface-muted/55 px-3 py-2.5">
      <div className="flex min-w-0 items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <Brain className="size-3.5 shrink-0 text-app-info" />
          <span className="truncate text-[10px] font-medium uppercase tracking-[0.12em] text-app-subtle">
            {t("composer.profileThinkingLevel")}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {isPending ? <LoaderCircle className="size-3.5 animate-spin text-app-subtle" /> : null}
          <span className="max-w-32 truncate text-[11px] font-semibold text-app-info">
            {getThinkingLevelLabel(value, t)}
          </span>
        </div>
      </div>
      <div className="relative mt-4">
        <div className="absolute left-[8.333333%] right-[8.333333%] top-2 h-px bg-app-border/70" />
        <div
          className="absolute left-[8.333333%] top-2 h-px bg-app-info/70 transition-all"
          style={{ width: selectedIndex > 0 ? `calc((100% - 16.666667%) * ${selectedIndex / (THINKING_LEVEL_OPTIONS.length - 1)})` : 0 }}
        />
        <div className="relative grid grid-cols-6">
          {THINKING_LEVEL_OPTIONS.map((option) => {
            const isSelected = option === value;
            return (
              <button
                aria-current={isSelected ? "true" : undefined}
                aria-label={`${t("composer.profileThinkingLevel")}: ${getThinkingLevelLabel(option, t)}`}
                className="group flex min-w-0 flex-col items-center gap-1 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-60"
                disabled={disabled || isPending}
                key={option}
                onClick={() => {
                  if (!isSelected) {
                    onChange(option);
                  }
                }}
                title={getThinkingLevelDescription(option, t)}
                type="button"
              >
                <span
                  className={cn(
                    "relative z-10 size-4 rounded-full border bg-app-surface transition-all group-hover:border-app-info/55 group-hover:bg-app-info/12",
                    isSelected
                      ? "scale-110 border-app-info bg-app-info shadow-[0_0_0_4px_rgba(59,130,246,0.14)]"
                      : "border-app-border",
                  )}
                />
                <span className={cn("truncate text-[10px] leading-4", isSelected ? "font-semibold text-app-info" : "text-app-subtle")}>
                  {getThinkingLevelLabel(option, t)}
                </span>
              </button>
            );
          })}
        </div>
      </div>
      <p className="mt-2 line-clamp-2 text-[11px] leading-4 text-app-muted">
        {getThinkingLevelDescription(value, t)}
      </p>
    </div>
  );
}

type QuickTierModelSelectProps = {
  disabled: boolean;
  isPending: boolean;
  label: string;
  modelRecordId: string;
  models: ReadonlyArray<AvailableProfileModelOption>;
  onChange: (providerId: string, modelRecordId: string) => void;
  providerId: string;
  tier: ProfileModelTier;
};

function QuickTierModelSelect({
  disabled,
  isPending,
  label,
  modelRecordId,
  models,
  onChange,
  providerId,
  tier,
}: QuickTierModelSelectProps) {
  const t = useT();
  const selectedModel = models.find((model) => model.providerId === providerId && model.modelRecordId === modelRecordId) ?? null;
  const selectedValue = getModelSelectValue(providerId, modelRecordId);
  const groupedModels = useMemo(() => {
    const grouped = new Map<string, { providerId: string; providerName: string; models: Array<AvailableProfileModelOption> }>();
    for (const model of models) {
      const group = grouped.get(model.providerId) ?? {
        providerId: model.providerId,
        providerName: model.providerName,
        models: [],
      };
      group.models.push(model);
      grouped.set(model.providerId, group);
    }
    return [...grouped.values()];
  }, [models]);
  const isPrimary = tier === "primary";
  const placeholder = modelRecordId ? `${t("composer.profileTier.notConfigured")} · ${modelRecordId}` : t("composer.profileTier.notConfigured");

  return (
    <div className="rounded-xl border border-app-border/65 bg-app-surface-muted/55 px-3 py-2.5">
      <div className="mb-2 flex min-w-0 items-center justify-between gap-3">
        <span className="truncate text-[10px] font-medium uppercase tracking-[0.12em] text-app-subtle">{label}</span>
        <div className="flex min-w-0 shrink-0 items-center gap-1.5">
          {isPending ? <LoaderCircle className="size-3.5 shrink-0 animate-spin text-app-subtle" /> : null}
          {selectedModel ? (
            <span className="max-w-36 truncate text-[11px] font-medium text-app-muted">{selectedModel.providerName}</span>
          ) : null}
        </div>
      </div>
      <PromptInputSelect
        disabled={disabled || isPending}
        onValueChange={(nextValue) => {
          const next = parseModelSelectValue(nextValue);
          if (isPrimary && (!next.providerId || !next.modelRecordId)) {
            return;
          }
          if (next.providerId === providerId && next.modelRecordId === modelRecordId) {
            return;
          }
          onChange(next.providerId, next.modelRecordId);
        }}
        value={selectedValue}
      >
        <PromptInputSelectTrigger
          className={cn(
            "h-auto min-h-9 w-full justify-between rounded-lg border border-app-border/65 bg-app-surface px-2.5 py-2 text-left text-[12px] text-app-foreground shadow-none hover:bg-app-surface-hover",
            !selectedModel && "text-app-muted",
          )}
        >
          <PromptInputSelectValue>
            <span className="flex min-w-0 items-center gap-2">
              {selectedModel ? (
                <ModelBrandIcon className="size-4 shrink-0" displayName={selectedModel.displayName} modelId={selectedModel.modelId} />
              ) : null}
              <span className="truncate">{selectedModel?.displayName ?? placeholder}</span>
            </span>
          </PromptInputSelectValue>
        </PromptInputSelectTrigger>
        <PromptInputSelectContent align="end" className="max-h-72 min-w-[320px]">
          {!isPrimary ? (
            <PromptInputSelectItem value={MODEL_SELECT_NONE_VALUE}>
              <span className="text-app-muted">{t("settings.general.notSet")}</span>
            </PromptInputSelectItem>
          ) : null}
          {groupedModels.map((group) => (
            <div key={group.providerId}>
              <div className="px-2 py-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-app-subtle">
                {group.providerName}
              </div>
              {group.models.map((model) => (
                <PromptInputSelectItem key={`${model.providerId}:${model.modelRecordId}`} value={getModelSelectValue(model.providerId, model.modelRecordId)}>
                  <span className="flex min-w-0 items-center gap-2">
                    <ModelBrandIcon className="size-4 shrink-0" displayName={model.displayName} modelId={model.modelId} />
                    <span className="min-w-0 truncate">{model.displayName}</span>
                    <span className="ml-auto shrink-0 truncate font-mono text-[10px] text-app-subtle">
                      {getShortModelId(model.modelId)}
                    </span>
                  </span>
                </PromptInputSelectItem>
              ))}
            </div>
          ))}
          {models.length === 0 ? (
            <div className="px-3 py-4 text-center text-[12px] text-app-muted">
              {t("settings.general.noModelsAvailable")}
            </div>
          ) : null}
        </PromptInputSelectContent>
      </PromptInputSelect>
      {models.length === 0 ? (
        <p className="mt-1.5 text-[11px] text-app-muted">{t("settings.general.noModelsAvailable")}</p>
      ) : null}
    </div>
  );
}

const BUILT_IN_PROFILE_AGENTS = [
  {
    id: "builtin-explore",
    icon: FileSearchIcon,
    nameKey: "settings.profileAgentAccess.builtIn.explore.name",
  },
  {
    id: "builtin-review",
    icon: CheckCircle2,
    nameKey: "settings.profileAgentAccess.builtIn.review.name",
  },
] as const;

function AgentTeamSection({
  customSubagents,
  profileId,
}: {
  customSubagents: ReadonlyArray<CustomSubagent>;
  profileId: string;
}) {
  const t = useT();
  const [accessIds, setAccessIds] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState(customSubagents.length > 0);
  const [hasError, setHasError] = useState(false);

  useEffect(() => {
    if (customSubagents.length === 0) {
      setAccessIds([]);
      setIsLoading(false);
      setHasError(false);
      return;
    }

    let cancelled = false;
    setIsLoading(true);
    setHasError(false);

    profileSubagentAccessGet(profileId)
      .then((ids) => {
        if (cancelled) return;
        setAccessIds(ids);
        setIsLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setAccessIds([]);
        setHasError(true);
        setIsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [profileId, customSubagents.length]);

  const allowedCustomAgents = customSubagents.filter((agent) => agent.isEnabled && accessIds.includes(agent.id));

  return (
    <div className="rounded-2xl border border-app-border/65 bg-app-surface/55 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="flex size-7 shrink-0 items-center justify-center rounded-lg border border-app-info/25 bg-app-info/10 text-app-info">
            <Bot className="size-3.5" />
          </span>
          <div className="min-w-0">
            <p className="text-[12px] font-semibold text-app-foreground">{t("composer.profileAgentTeam")}</p>
            <p className="text-[11px] text-app-muted">{t("composer.profileAgentTeamDesc")}</p>
          </div>
        </div>
      </div>

      <div className="space-y-2">
        <div>
          <p className="mb-1.5 text-[10px] font-medium uppercase tracking-[0.12em] text-app-subtle">
            {t("composer.profileAgentTeamBuiltIn")}
          </p>
          <div className="flex flex-wrap gap-1.5">
            {BUILT_IN_PROFILE_AGENTS.map((agent) => {
              const Icon = agent.icon;
              return (
                <span
                  key={agent.id}
                  className="inline-flex items-center gap-1.5 rounded-full border border-app-info/25 bg-app-info/10 px-2 py-1 text-[11px] font-medium text-app-info"
                >
                  <Icon className="size-3" />
                  {t(agent.nameKey)}
                </span>
              );
            })}
          </div>
        </div>

        <div>
          <p className="mb-1.5 text-[10px] font-medium uppercase tracking-[0.12em] text-app-subtle">
            {t("composer.profileAgentTeamCustom")}
          </p>
          {isLoading ? (
            <p className="rounded-lg border border-app-border/60 bg-app-surface-muted/45 px-2 py-1.5 text-[11px] text-app-muted">
              {t("composer.profileAgentTeamLoading")}
            </p>
          ) : allowedCustomAgents.length > 0 ? (
            <div className="flex flex-wrap gap-1.5">
              {allowedCustomAgents.map((agent) => (
                <span
                  key={agent.id}
                  className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-app-border/70 bg-app-surface-muted px-2 py-1 text-[11px] font-medium text-app-foreground"
                >
                  <Bot className="size-3 shrink-0 text-app-subtle" />
                  <span className="truncate">{agent.name}</span>
                </span>
              ))}
            </div>
          ) : (
            <p className="rounded-lg border border-dashed border-app-border/60 bg-app-surface-muted/35 px-2 py-1.5 text-[11px] text-app-muted">
              {hasError ? t("composer.profileAgentTeamUnavailable") : t("composer.profileAgentTeamEmpty")}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function ProfileDetailsPanel({
  customSubagents,
  onUpdateAgentProfile,
  profile,
  providers,
}: {
  customSubagents: ReadonlyArray<CustomSubagent>;
  onUpdateAgentProfile?: typeof updateAgentProfileAction;
  profile: AgentProfile | null;
  providers: ReadonlyArray<ProviderEntry>;
}) {
  const t = useT();
  const [pendingProfilePatchKey, setPendingProfilePatchKey] = useState<string | null>(null);
  const [quickEditError, setQuickEditError] = useState<string | null>(null);
  const activeProfileIdRef = useRef<string | null>(profile?.id ?? null);
  const quickEditRequestIdRef = useRef(0);
  const availableModels = useMemo(() => getAvailableProfileModelOptions(providers), [providers]);

  useEffect(() => {
    activeProfileIdRef.current = profile?.id ?? null;
    quickEditRequestIdRef.current += 1;
    setPendingProfilePatchKey(null);
    setQuickEditError(null);
  }, [profile?.id]);

  if (!profile) {
    return <p className="px-3 pb-3 pt-2 text-[11px] text-app-muted">{t("composer.noProfileAvailable")}</p>;
  }

  const canQuickEdit = Boolean(onUpdateAgentProfile);
  const isQuickEditDisabled = !canQuickEdit || Boolean(pendingProfilePatchKey);
  const tiers: Array<{ label: string; tier: ProfileModelTier }> = [
    { label: t("composer.profileTier.primary"), tier: "primary" },
    { label: t("composer.profileTier.auxiliary"), tier: "assistant" },
    { label: t("composer.profileTier.lightweight"), tier: "lite" },
  ];
  const runQuickEdit = async (key: string, patch: ProfileQuickEditPatch) => {
    if (!onUpdateAgentProfile || pendingProfilePatchKey) {
      return;
    }

    const targetProfileId = profile.id;
    const requestId = quickEditRequestIdRef.current + 1;
    quickEditRequestIdRef.current = requestId;

    setQuickEditError(null);
    setPendingProfilePatchKey(key);
    try {
      await onUpdateAgentProfile(targetProfileId, patch);
    } catch (error) {
      if (activeProfileIdRef.current === targetProfileId && quickEditRequestIdRef.current === requestId) {
        setQuickEditError(error instanceof Error ? error.message : t("composer.profileQuickEditFailed"));
      }
    } finally {
      if (activeProfileIdRef.current === targetProfileId && quickEditRequestIdRef.current === requestId) {
        setPendingProfilePatchKey(null);
      }
    }
  };

  return (
    <div className="max-h-[430px] space-y-3 overflow-y-auto rounded-2xl border border-app-border/65 bg-app-surface/45 p-3 [scrollbar-width:thin]">
      <div>
        <div className="mb-2 flex items-center justify-between gap-2">
          <p className="text-[11px] font-medium uppercase tracking-[0.12em] text-app-subtle">
            {t("composer.profileDetailsTitle")}
          </p>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          <ProfileDetailCard
            label={t("composer.profileResponseStyle")}
            value={getResponseStyleLabel(profile.responseStyle, t)}
          />
          <ProfileDetailCard
            label={t("composer.profileResponseLanguage")}
            value={profile.responseLanguage || t("composer.profileTier.notConfigured")}
          />
          <div className="sm:col-span-2">
            <QuickThinkingLevelControl
              disabled={isQuickEditDisabled}
              isPending={pendingProfilePatchKey === "thinkingLevel"}
              onChange={(thinkingLevel) => {
                void runQuickEdit("thinkingLevel", { thinkingLevel });
              }}
              value={profile.thinkingLevel}
            />
          </div>
        </div>
      </div>

      <div className="grid gap-2">
        {tiers.map(({ label, tier }) => {
          const providerId = getProfileTierProviderId(profile, tier);
          const modelRecordId = getProfileTierModelId(profile, tier);

          return (
            <QuickTierModelSelect
              disabled={isQuickEditDisabled}
              isPending={pendingProfilePatchKey === `model:${tier}`}
              key={tier}
              label={label}
              modelRecordId={modelRecordId}
              models={availableModels}
              onChange={(nextProviderId, nextModelRecordId) => {
                void runQuickEdit(
                  `model:${tier}`,
                  getProfileTierPatch(tier, nextProviderId, nextModelRecordId),
                );
              }}
              providerId={providerId}
              tier={tier}
            />
          );
        })}
      </div>

      {quickEditError ? (
        <p className="rounded-lg border border-app-danger/25 bg-app-danger/8 px-3 py-2 text-[11px] leading-4 text-app-danger">
          {quickEditError}
        </p>
      ) : null}

      <AgentTeamSection customSubagents={customSubagents} profileId={profile.id} />
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
  allowMissingActiveProfile = false,
  canSubmitWhenAttachmentsOnly = true,
  canSubmitWhileRunning = false,
  className,
  commands = [],
  composerShellClassName,
  customSubagents = [],
  enabledSkills = [],
  error,
  onErrorMessageChange,
  onOpenProfileSettings,
  onRuntimeQueueSubmitModeChange,
  onSelectAgentProfile,
  onUpdateAgentProfile,
  onStop,
  onSubmit,
  placeholder,
  providers,
  runtimeQueueSubmitMode,
  showRuntimeQueueSubmitMode = false,
  status,
  suggestions,
  textareaClassName,
  value,
  workspaceId,
  onValueChange,
  initialReferencedFiles,
  initialAttachmentData,
  restoreSignal,
  clearSignal,
  onReferencedFilesChange,
  onAttachmentDataChange,
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
  const activeProfile = useMemo(() => {
    const matchedProfile = agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? null;
    if (matchedProfile) {
      return matchedProfile;
    }
    if (allowMissingActiveProfile) {
      return null;
    }
    return agentProfiles[0] ?? null;
  }, [activeAgentProfileId, agentProfiles, allowMissingActiveProfile]);
  const sortedAgentProfiles = useMemo(() => sortAgentProfilesByName(agentProfiles), [agentProfiles]);
  const hasMissingActiveProfile =
    allowMissingActiveProfile && Boolean(activeAgentProfileId) && activeProfile === null;
  const canSwitchProfiles = agentProfiles.length > 0;
  const hasComposerText = hasNonWhitespace(value);
  const shouldShowRuntimeQueueModeSelect =
    showRuntimeQueueSubmitMode && hasComposerText && Boolean(onRuntimeQueueSubmitModeChange);
  const selectedRuntimeQueueSubmitMode = runtimeQueueSubmitMode ?? "follow_up";
  const commandRegistry = useMemo(
    () => buildComposerCommandRegistry(commands),
    [commands],
  );
  // Defer value for derived computations so the textarea stays responsive
  // while mention/command/referenced-files logic catches up asynchronously.
  const deferredValue = useDeferredValue(value);
  const slashActive = shouldShowCommandPicker(deferredValue);
  const mentionMatch = useMemo(
    () => getActiveMentionMatch(deferredValue, cursorPosition),
    [cursorPosition, deferredValue],
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
    () => getFilteredCommands(commandRegistry, deferredValue),
    [commandRegistry, deferredValue],
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
    setReferencedFiles((current) => removeUnmentionedReferencedFiles(current, deferredValue));
  }, [deferredValue]);

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

  // ── Restore initial state on mount ──
  const mountRestoredRef = useRef(false);
  useEffect(() => {
    if (mountRestoredRef.current) {
      return;
    }
    mountRestoredRef.current = true;
    if (initialReferencedFiles && initialReferencedFiles.length > 0) {
      setReferencedFiles(initialReferencedFiles);
    }
  }, [initialReferencedFiles]);

  // ── Clear local state when clearSignal changes ──
  useEffect(() => {
    if (clearSignal === undefined || clearSignal === 0) {
      return;
    }
    setReferencedFiles([]);
    setClearReferencedSourcesSignal((current) => current + 1);
    setSelectedCommandKey(null);
    setSelectedFileIndex(0);
    setSelectedSkillIndex(0);
    setFileSearchResults([]);
    setFileSearchLoading(false);
    setFileSearchError(null);
  }, [clearSignal]);

  // ── Restore local state when parent explicitly restores a draft ──
  useEffect(() => {
    if (restoreSignal === undefined || restoreSignal === 0) {
      return;
    }
    setReferencedFiles(initialReferencedFiles ?? []);
    referencedSourceBridgeRef.current?.syncFiles(initialReferencedFiles ?? []);
    setSelectedCommandKey(null);
    setSelectedFileIndex(0);
    setSelectedSkillIndex(0);
    setFileSearchResults([]);
    setFileSearchLoading(false);
    setFileSearchError(null);
  }, [initialReferencedFiles, restoreSignal]);

  // ── Sync referenced files to parent (draft persistence) ──
  const syncedRefFilesRef = useRef<string>("");
  useEffect(() => {
    const key = JSON.stringify(referencedFiles);
    if (key !== syncedRefFilesRef.current) {
      syncedRefFilesRef.current = key;
      onReferencedFilesChange?.(referencedFiles);
    }
  }, [referencedFiles, onReferencedFilesChange]);

  // ── Serialize attachments to parent (draft persistence) ──
  // We use a child component for this because usePromptInputAttachments()
  // must be called inside a PromptInput descendant.

  const handlePromptSubmit = async (message: PromptInputMessage) => {
    const submission = buildSubmissionFromPromptInput(message, commandRegistry, "default", referencedFiles);
    await onSubmit(submission);
    // Only clear referenced files and sources after a successful submit.
    // If onSubmit throws (e.g. thread has an active run), the composer
    // retains all state so the user doesn't lose their composed content.
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
        void handlePromptSubmit({ text: nextValue, files: [] }).catch(() => { /* rejection handled via composerError */ });
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
            <ComposerInitialStateRestorer
              initialAttachmentData={initialAttachmentData}
            />
            <ComposerClearSignalHandler
              signal={clearSignal ?? 0}
            />
            <ComposerAttachmentDraftSync
              onChange={(data) => onAttachmentDataChange?.(data)}
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
                  value.length > LARGE_TEXT_THRESHOLD && "overflow-y-auto",
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
                style={value.length > LARGE_TEXT_THRESHOLD ? { fieldSizing: "fixed" } as React.CSSProperties : undefined}
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
                                    void handlePromptSubmit({ text: nextValue, files: [] }).catch(() => { /* rejection handled via composerError */ });
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

              <div className="flex items-center gap-2">
                {canSwitchProfiles ? (
                  <ModelSelector onOpenChange={setProfileSelectorOpen} open={isProfileSelectorOpen}>
                    <ModelSelectorTrigger asChild>
                      <PromptInputButton className="h-auto min-w-0 max-w-[360px] justify-start gap-3 px-3 py-2" size="sm">
                        {activeProfile ? (
                          <ProfileInlineIdentity badge={false} profile={activeProfile} providers={providers} />
                        ) : (
                          <div className="flex min-w-0 items-center gap-2 text-app-danger">
                            <UserStar className="size-4 shrink-0" />
                            <span className="truncate text-sm font-medium">{t("composer.profileDeleted")}</span>
                          </div>
                        )}
                      </PromptInputButton>
                    </ModelSelectorTrigger>
                    <ModelSelectorContent
                      className="sm:max-w-[760px]"
                      commandProps={{ value: activeAgentProfileId ?? undefined }}
                      showCloseButton={false}
                      title={t("composer.agentProfilesTitle")}
                    >
                      <div className="flex items-center justify-between gap-3 border-b border-app-border/55 px-4 py-2.5">
                        <div className="min-w-0">
                          <p className="text-[12px] font-semibold text-app-foreground">{t("composer.agentProfilesTitle")}</p>
                          <p className="mt-0.5 text-[11px] text-app-muted">{t("composer.profileSelectLabel")}</p>
                        </div>
                        <Button
                          aria-label={t("composer.editProfiles")}
                          className="size-7 shrink-0 rounded-full text-app-muted hover:text-app-foreground"
                          disabled={!onOpenProfileSettings}
                          onClick={() => {
                            setProfileSelectorOpen(false);
                            onOpenProfileSettings?.();
                          }}
                          size="icon-sm"
                          title={t("composer.editProfiles")}
                          type="button"
                          variant="ghost"
                        >
                          <Settings className="size-3.5" />
                        </Button>
                      </div>
                      <div className="grid gap-3 p-3 md:grid-cols-[minmax(220px,0.85fr)_minmax(0,1.35fr)] md:items-start">
                        <ModelSelectorList className="max-h-[430px] rounded-2xl border border-app-border/55 bg-app-surface/45 p-1.5 [scrollbar-width:thin]">
                          <ModelSelectorEmpty>{t("composer.noProfileAvailable")}</ModelSelectorEmpty>
                          <ModelSelectorGroup className="p-0">
                            {sortedAgentProfiles.map((profile) => (
                              <ProfileSelectorItem
                                isActive={profile.id === activeAgentProfileId}
                                key={profile.id}
                                onSelect={() => {
                                  onSelectAgentProfile(profile.id);
                                }}
                                profile={profile}
                                providers={providers}
                              />
                            ))}
                          </ModelSelectorGroup>
                        </ModelSelectorList>
                        <ProfileDetailsPanel
                          customSubagents={customSubagents}
                          onUpdateAgentProfile={onUpdateAgentProfile}
                          profile={activeProfile}
                          providers={providers}
                        />
                      </div>
                    </ModelSelectorContent>
                  </ModelSelector>
                ) : (
                  <PromptInputButton className="h-auto min-w-0 max-w-[360px] justify-start gap-3 px-3 py-2" disabled size="sm">
                    {activeProfile ? (
                      <ProfileInlineIdentity badge={false} muted profile={activeProfile} providers={providers} />
                    ) : (
                      <div className="flex min-w-0 items-center gap-2 text-app-danger/80">
                        <UserStar className="size-4 shrink-0" />
                        <span className="truncate text-sm font-medium">{t("composer.profileDeleted")}</span>
                      </div>
                    )}
                  </PromptInputButton>
                )}

                
              </div>
            </PromptInputTools>

            <div className="flex items-center gap-2">
              {shouldShowRuntimeQueueModeSelect && onRuntimeQueueSubmitModeChange ? (
                <RuntimeQueueModeSelect
                  mode={selectedRuntimeQueueSubmitMode}
                  onModeChange={onRuntimeQueueSubmitModeChange}
                />
              ) : null}
              <PromptInputSubmitButton
                activeProfile={activeProfile}
                allowAttachmentsOnly={canSubmitWhenAttachmentsOnly}
                canSubmitWhileRunning={canSubmitWhileRunning}
                composerValue={value}
                hasMissingActiveProfile={hasMissingActiveProfile}
                onStop={onStop}
                status={status}
              />
            </div>
          </PromptInputFooter>
        </PromptInput>
      </div>

      {error ? <p className="mt-2 text-xs text-app-danger">{error}</p> : null}
      {hasMissingActiveProfile ? (
        <p className="mt-2 text-xs text-app-danger">{t("composer.profileDeletedHint")}</p>
      ) : !activeProfile ? (
        <p className="mt-2 text-xs text-app-danger">No active profile is available for the composer right now.</p>
      ) : null}
    </div>
  );
}
