import type { ChatStatus, FileUIPart } from "ai";
import {
  Bot,
  BracesIcon,
  CheckIcon,
  FileCodeIcon,
  FileIcon,
  FileTextIcon,
  ImageIcon,
  PaperclipIcon,
  XIcon,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type SyntheticEvent } from "react";
import {
  ModelSelector,
  ModelSelectorContent,
  ModelSelectorEmpty,
  ModelSelectorGroup,
  ModelSelectorInput,
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
} from "@/components/ai-elements/prompt-input";
import { Suggestion, Suggestions } from "@/components/ai-elements/suggestion";
import {
  buildComposerCommandRegistry,
  filterComposerCommands,
  parseSlashCommandInput,
  shouldSmartSendCommand,
  type ComposerCommandDescriptor,
  type ComposerSubmission,
} from "@/modules/workbench-shell/model/composer-commands";
import {
  getProfilePrimaryModelId,
  getProfilePrimaryModelLabel,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import type { AgentProfile, CommandEntry, ProviderEntry } from "@/modules/settings-center/model/types";
import type { RunMode } from "@/shared/types/api";
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

type WorkbenchPromptComposerProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  canSubmitWhenAttachmentsOnly?: boolean;
  commands?: ReadonlyArray<CommandEntry>;
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
): ComposerSubmission {
  const trimmedText = message.text?.trim() ?? "";
  const parsedCommand = trimmedText ? parseSlashCommandInput(trimmedText, registry) : null;

  if (!parsedCommand?.command) {
    return {
      kind: "plain",
      displayText: trimmedText,
      effectivePrompt: trimmedText,
      rawMessage: message,
      metadata: null,
      runMode,
    };
  }

  const effectivePrompt = parsedCommand.command.prompt.replace(/{{\s*arguments\s*}}/g, parsedCommand.argumentsText).replace(/{{\s*command\s*}}/g, parsedCommand.command.name).trim();

  return {
    kind: "command",
    displayText: trimmedText,
    effectivePrompt,
    rawMessage: message,
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
          aria-label={`移除附件 ${attachment.name}`}
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
          aria-label={`移除附件 ${attachment.name}`}
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
  const attachments = usePromptInputAttachments();

  if (attachments.files.length === 0) {
    return null;
  }

  return (
    <PromptInputHeader className="border-b border-app-border/45 bg-app-surface-muted/35 px-3 py-2">
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
        <Badge className="rounded-full px-2 py-0.5" variant="secondary">
          <PaperclipIcon className="size-3" />
          {attachments.files.length} 个附件
        </Badge>
        {attachments.files.map((attachment) => (
          <AttachmentCard
            attachment={{
              id: attachment.id,
              mediaType: attachment.mediaType,
              name: attachment.filename?.trim() || "未命名附件",
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

function ComposerAttachmentTrigger() {
  const attachments = usePromptInputAttachments();

  return (
    <PromptInputButton
      aria-label="上传文件或图片"
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
        <Bot className="size-3.5" />
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
  return (
    <ModelSelectorItem onSelect={onSelect} value={profile.id}>
      <div className="min-w-0 flex flex-1 items-center">
        <ProfileInlineIdentity profile={profile} providers={providers} />
      </div>
      {isActive ? <CheckIcon className="ml-auto size-4" /> : <span className="ml-auto size-4" />}
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
  commands = [],
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
  onValueChange,
}: WorkbenchPromptComposerProps) {
  const [isProfileSelectorOpen, setProfileSelectorOpen] = useState(false);
  const [selectedCommandKey, setSelectedCommandKey] = useState<string | null>(null);
  const commandPanelRef = useRef<HTMLDivElement | null>(null);
  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );
  const canSwitchProfiles = agentProfiles.length > 1 && Boolean(activeProfile);
  const commandRegistry = useMemo(
    () => buildComposerCommandRegistry(commands),
    [commands],
  );
  const slashActive = shouldShowCommandPicker(value);
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

  const handlePromptSubmit = (message: PromptInputMessage) => {
    const submission = buildSubmissionFromPromptInput(message, commandRegistry, runMode);
    onSubmit(submission);
  };

  const handleTextareaKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
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
    <div className="mx-auto flex max-w-4xl flex-col gap-3">
      {suggestions && suggestions.length > 0 ? (
        <Suggestions className="gap-2">
          {suggestions.map((suggestion) => (
            <Suggestion key={suggestion} onClick={(nextSuggestion) => onValueChange(nextSuggestion)} suggestion={suggestion} variant="secondary" />
          ))}
        </Suggestions>
      ) : null}

      <div className="rounded-[26px] border border-app-border/60 bg-app-surface/82 p-1.5 shadow-[0_22px_50px_-42px_rgba(15,23,42,0.38)] backdrop-blur-sm">
        <PromptInput
          accept="image/*,.pdf,.md,.txt,.json,.ts,.tsx"
          className="[&_[data-slot=input-group]]:overflow-visible [&_[data-slot=input-group]]:shadow-none [&_[data-slot=input-group]:focus-within]:!border-app-border/60 [&_[data-slot=input-group]:focus-within]:!ring-0"
          maxFileSize={10 * 1024 * 1024}
          maxFiles={4}
          onError={(nextError) => onErrorMessageChange?.(nextError.message)}
          onSubmit={handlePromptSubmit}
        >
          <PromptInputBody>
            <ComposerAttachmentStateSync
              onHasAttachmentsChange={(hasAttachments) => {
                if (hasAttachments) {
                  onErrorMessageChange?.(null);
                }
              }}
            />
            <ComposerAttachmentHeader />
            <PromptInputTextarea
              className={cn("min-h-[88px]", textareaClassName)}
              onChange={(event) => onValueChange(event.currentTarget.value)}
              onKeyDown={handleTextareaKeyDown}
              placeholder={placeholder}
              value={value}
            />
            {slashActive && filteredCommands.length > 0 ? (
              <div
                className="absolute inset-x-3 bottom-[calc(100%+0.5rem)] z-20 min-w-0"
                ref={commandPanelRef}
              >
                <PromptInputCommand
                  className="w-full min-w-0 overflow-hidden rounded-t-[24px] rounded-b-none border border-b-0 border-app-border/70 bg-app-surface/96 p-2 shadow-[0_26px_70px_-42px_rgba(15,23,42,0.45)]"
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
                      <ModelSelectorContent title="Profile Selector">
                        <ModelSelectorInput placeholder="Search profiles..." />
                        <ModelSelectorList>
                          <ModelSelectorEmpty>未找到可用的 profile。</ModelSelectorEmpty>
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

      {error ? <p className="text-xs text-app-danger">{error}</p> : null}
      {!activeProfile ? (
        <p className="text-xs text-app-danger">No active profile is available for the composer right now.</p>
      ) : null}
    </div>
  );
}
