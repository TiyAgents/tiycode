import type { ChatStatus, FileUIPart } from "ai";
import { Bot, CheckIcon, FileIcon, ImageIcon, PaperclipIcon, XIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
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
  getProfilePrimaryModelId,
  getProfilePrimaryModelLabel,
} from "@/modules/workbench-shell/model/ai-elements-task-demo";
import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { ModelBrandIcon } from "@/shared/ui/model-brand-icon";

type ComposerAttachment = {
  id: string;
  mediaType?: string;
  name: string;
};

type WorkbenchPromptComposerProps = {
  activeAgentProfileId: string;
  agentProfiles: ReadonlyArray<AgentProfile>;
  canSubmitWhenAttachmentsOnly?: boolean;
  error?: string | null;
  onErrorMessageChange?: (message: string | null) => void;
  onSelectAgentProfile: (id: string) => void;
  onStop: () => void;
  onSubmit: (message: PromptInputMessage) => void;
  placeholder: string;
  providers: ReadonlyArray<ProviderEntry>;
  status: ChatStatus;
  suggestions?: ReadonlyArray<string>;
  textareaClassName?: string;
  value: string;
  onValueChange: (value: string) => void;
};

function getAttachmentGlyph(mediaType?: string) {
  if (mediaType?.startsWith("image/")) {
    return <ImageIcon className="size-3.5" />;
  }

  return <FileIcon className="size-3.5" />;
}

function AttachmentChip({
  attachment,
  onRemove,
  tone = "message",
}: {
  attachment: ComposerAttachment;
  onRemove?: (id: string) => void;
  tone?: "composer" | "message";
}) {
  return (
    <div
      className={cn(
        "inline-flex max-w-full items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium",
        tone === "composer"
          ? "border-app-border/55 bg-app-surface-muted/80 text-app-foreground"
          : "border-app-border/45 bg-app-surface/60 text-app-muted",
      )}
    >
      <span className={cn("shrink-0", tone === "composer" ? "text-app-foreground" : "text-app-subtle")}>
        {getAttachmentGlyph(attachment.mediaType)}
      </span>
      <span className="truncate">{attachment.name}</span>
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
          <AttachmentChip
            attachment={{
              id: attachment.id,
              mediaType: attachment.mediaType,
              name: attachment.filename?.trim() || "未命名附件",
            }}
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
  const canSubmit = Boolean(activeProfile) && (hasText || (allowAttachmentsOnly && hasAttachments));

  return <PromptInputSubmit disabled={!canSubmit} onStop={onStop} status={status} />;
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

export function mapComposerAttachments(files: Array<FileUIPart>) {
  return files.map((file, index) => ({
    id: file.url || `${file.filename || "attachment"}-${index}`,
    mediaType: file.mediaType,
    name: file.filename?.trim() || `附件 ${index + 1}`,
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

  return (
    <div className="mb-3 flex flex-wrap gap-2">
      {attachments.map((attachment) => (
        <AttachmentChip attachment={attachment} key={attachment.id} />
      ))}
    </div>
  );
}

export function WorkbenchPromptComposer({
  activeAgentProfileId,
  agentProfiles,
  canSubmitWhenAttachmentsOnly = true,
  error,
  onErrorMessageChange,
  onSelectAgentProfile,
  onStop,
  onSubmit,
  placeholder,
  providers,
  status,
  suggestions,
  textareaClassName,
  value,
  onValueChange,
}: WorkbenchPromptComposerProps) {
  const [isProfileSelectorOpen, setProfileSelectorOpen] = useState(false);
  const activeProfile = useMemo(
    () => agentProfiles.find((profile) => profile.id === activeAgentProfileId) ?? agentProfiles[0] ?? null,
    [activeAgentProfileId, agentProfiles],
  );
  const canSwitchProfiles = agentProfiles.length > 1 && Boolean(activeProfile);

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
          className="[&_[data-slot=input-group]]:shadow-none [&_[data-slot=input-group]:focus-within]:!border-app-border/60 [&_[data-slot=input-group]:focus-within]:!ring-0"
          maxFileSize={10 * 1024 * 1024}
          maxFiles={4}
          onError={(nextError) => onErrorMessageChange?.(nextError.message)}
          onSubmit={onSubmit}
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
              placeholder={placeholder}
              value={value}
            />
          </PromptInputBody>
          <PromptInputFooter>
            <PromptInputTools>
              <ComposerAttachmentTrigger />

              {activeProfile ? (
                canSwitchProfiles ? (
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
                )
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
