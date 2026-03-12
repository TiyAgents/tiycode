import { useEffect, useState } from "react";

export type SettingsCategory = "account" | "general" | "prompts" | "approval-policy";
export type PromptResponseStyle = "balanced" | "concise" | "guide";
export type CommandExecutionPolicy = "ask-every-time" | "auto-safe" | "full-auto";
export type AccessPolicy = "ask-first" | "block" | "allow";
export type RiskyCommandConfirmationPolicy = "always-confirm" | "block";

export type PromptSettings = {
  systemPrompt: string;
  responseStyle: PromptResponseStyle;
  includeProjectContext: boolean;
  promptNotes: string;
};

export type ApprovalPolicySettings = {
  commandExecution: CommandExecutionPolicy;
  fileWriteOutsideWorkspace: AccessPolicy;
  networkAccess: AccessPolicy;
  riskyCommandConfirmation: RiskyCommandConfirmationPolicy;
};

type WorkbenchSettingsState = {
  prompts: PromptSettings;
  approvalPolicy: ApprovalPolicySettings;
};

const STORAGE_KEY = "tiy-agent-workbench-settings";

const DEFAULT_PROMPT_SETTINGS: PromptSettings = {
  systemPrompt:
    "You are Tiy Agent, a desktop coding partner. Keep answers crisp, grounded in the local workspace, and explicit about risks before taking action.",
  responseStyle: "balanced",
  includeProjectContext: true,
  promptNotes:
    "Prefer existing project conventions, summarize tradeoffs before larger changes, and keep implementation notes useful for follow-up threads.",
};

const DEFAULT_APPROVAL_POLICY_SETTINGS: ApprovalPolicySettings = {
  commandExecution: "auto-safe",
  fileWriteOutsideWorkspace: "ask-first",
  networkAccess: "ask-first",
  riskyCommandConfirmation: "always-confirm",
};

const DEFAULT_SETTINGS: WorkbenchSettingsState = {
  prompts: DEFAULT_PROMPT_SETTINGS,
  approvalPolicy: DEFAULT_APPROVAL_POLICY_SETTINGS,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isPromptResponseStyle(value: unknown): value is PromptResponseStyle {
  return value === "balanced" || value === "concise" || value === "guide";
}

function isCommandExecutionPolicy(value: unknown): value is CommandExecutionPolicy {
  return value === "ask-every-time" || value === "auto-safe" || value === "full-auto";
}

function isAccessPolicy(value: unknown): value is AccessPolicy {
  return value === "ask-first" || value === "block" || value === "allow";
}

function isRiskyCommandConfirmationPolicy(value: unknown): value is RiskyCommandConfirmationPolicy {
  return value === "always-confirm" || value === "block";
}

function getStoredSettings(): WorkbenchSettingsState {
  if (typeof window === "undefined") {
    return DEFAULT_SETTINGS;
  }

  const rawValue = window.localStorage.getItem(STORAGE_KEY);

  if (!rawValue) {
    return DEFAULT_SETTINGS;
  }

  try {
    const parsed = JSON.parse(rawValue) as unknown;

    if (!isRecord(parsed)) {
      return DEFAULT_SETTINGS;
    }

    const prompts = isRecord(parsed.prompts) ? parsed.prompts : {};
    const approvalPolicy = isRecord(parsed.approvalPolicy) ? parsed.approvalPolicy : {};

    return {
      prompts: {
        systemPrompt:
          typeof prompts.systemPrompt === "string"
            ? prompts.systemPrompt
            : DEFAULT_PROMPT_SETTINGS.systemPrompt,
        responseStyle: isPromptResponseStyle(prompts.responseStyle)
          ? prompts.responseStyle
          : DEFAULT_PROMPT_SETTINGS.responseStyle,
        includeProjectContext:
          typeof prompts.includeProjectContext === "boolean"
            ? prompts.includeProjectContext
            : DEFAULT_PROMPT_SETTINGS.includeProjectContext,
        promptNotes:
          typeof prompts.promptNotes === "string" ? prompts.promptNotes : DEFAULT_PROMPT_SETTINGS.promptNotes,
      },
      approvalPolicy: {
        commandExecution: isCommandExecutionPolicy(approvalPolicy.commandExecution)
          ? approvalPolicy.commandExecution
          : DEFAULT_APPROVAL_POLICY_SETTINGS.commandExecution,
        fileWriteOutsideWorkspace: isAccessPolicy(approvalPolicy.fileWriteOutsideWorkspace)
          ? approvalPolicy.fileWriteOutsideWorkspace
          : DEFAULT_APPROVAL_POLICY_SETTINGS.fileWriteOutsideWorkspace,
        networkAccess: isAccessPolicy(approvalPolicy.networkAccess)
          ? approvalPolicy.networkAccess
          : DEFAULT_APPROVAL_POLICY_SETTINGS.networkAccess,
        riskyCommandConfirmation: isRiskyCommandConfirmationPolicy(approvalPolicy.riskyCommandConfirmation)
          ? approvalPolicy.riskyCommandConfirmation
          : DEFAULT_APPROVAL_POLICY_SETTINGS.riskyCommandConfirmation,
      },
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function useWorkbenchSettings() {
  const [settings, setSettings] = useState<WorkbenchSettingsState>(() => getStoredSettings());

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  }, [settings]);

  const updatePromptSetting = <Key extends keyof PromptSettings>(key: Key, value: PromptSettings[Key]) => {
    setSettings((current) => ({
      ...current,
      prompts: {
        ...current.prompts,
        [key]: value,
      },
    }));
  };

  const updateApprovalPolicySetting = <Key extends keyof ApprovalPolicySettings>(
    key: Key,
    value: ApprovalPolicySettings[Key],
  ) => {
    setSettings((current) => ({
      ...current,
      approvalPolicy: {
        ...current.approvalPolicy,
        [key]: value,
      },
    }));
  };

  return {
    prompts: settings.prompts,
    approvalPolicy: settings.approvalPolicy,
    updatePromptSetting,
    updateApprovalPolicySetting,
  };
}
