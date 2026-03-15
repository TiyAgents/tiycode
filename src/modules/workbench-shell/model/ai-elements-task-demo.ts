import type { AgentProfile, ProviderEntry } from "@/modules/settings-center/model/types";
import { matchModelIcon } from "@/shared/lib/llm-brand-matcher";

export type DemoQueueItem = {
  id: string;
  title: string;
  description: string;
  status: "pending" | "completed";
};

export const AI_ELEMENTS_THREAD_TITLE = "使用 AI Elements 原生组件重构任务 Demo 页面";

export const AI_ELEMENTS_REQUEST =
  "使用 AI Elements 原生组件重新构建任务的 Demo 页面，并把 Plan、Chain of Thought、Confirmation、Queue、Reasoning、Sources、Suggestion、Tool 都加进现有会话流里。";

export const AI_ELEMENTS_PLAN = {
  description:
    "保留现有 workbench 外壳，只把已有会话区替换成一条单线程的 AI Elements 任务流，并让 composer 支持基于 Settings 中 profile 的切换。",
  overview:
    "实现重点是三件事：把静态正文改成真实的任务叙事，把 composer 重组为 AI Elements PromptInput，并让 profile 切换只影响下一轮本地模拟执行。",
  title: "Replace the current task demo with an AI Elements thread",
  steps: [
    "Install the official AI Elements primitives without overwriting the existing shared button, card, input, and textarea.",
    "Swap the current static conversation content for a single-threaded task flow using Plan, Queue, Reasoning, Chain of Thought, Tool, Confirmation, and Sources.",
    "Rebuild the follow-up composer with PromptInput and a profile-aware selector that reads the current Settings state.",
    "Keep New Thread, the workspace chrome, and the right-hand drawer intact while validating the new demo path with typecheck and a web build.",
  ],
} as const;

export const AI_ELEMENTS_INITIAL_QUEUE: ReadonlyArray<DemoQueueItem> = [
  {
    id: "install-primitives",
    title: "Install the official AI Elements primitives",
    description: "Add the native Plan, Queue, PromptInput, Tool, Confirmation, Sources, Suggestion, and reasoning components.",
    status: "pending",
  },
  {
    id: "replace-thread-body",
    title: "Replace the existing conversation surface",
    description: "Swap the current static cards for a single-threaded AI Elements narrative without touching New Thread.",
    status: "pending",
  },
  {
    id: "validate-profile-follow-up",
    title: "Validate profile-aware follow-up behavior",
    description: "Send one more message after the switch and confirm the active profile changes the next simulated response.",
    status: "pending",
  },
  {
    id: "verify-shell",
    title: "Verify the workbench shell stays stable",
    description: "Run typecheck and a web build after the UI swap to confirm the shell, drawer, and terminal still hold together.",
    status: "pending",
  },
];

export const AI_ELEMENTS_REASONING_TEXT =
  "I need to preserve the current workbench chrome while replacing only the existing thread surface. That means the new flow should live inside the non-New Thread branch, reuse the existing Settings-managed profiles, and keep the old New Thread composer untouched. AI Elements already gives us native building blocks for the plan, queue, reasoning, chain-of-thought, tool states, sources, suggestions, and prompt input, so the safest path is to compose those primitives directly instead of recreating them locally.";

export const AI_ELEMENTS_CHAIN_STEPS = [
  {
    id: "inspect-thread",
    label: "Inspect the current non-New Thread surface and isolate the exact replacement boundary.",
    status: "complete" as const,
  },
  {
    id: "map-components",
    label: "Map the requested experience to native AI Elements components instead of local replicas.",
    status: "complete" as const,
  },
  {
    id: "preserve-profiles",
    label: "Reuse Settings-managed agent profiles and keep profile switching inside the new PromptInput footer.",
    status: "complete" as const,
  },
  {
    id: "finalize-thread",
    label: "Finalize the task demo so approval, queue updates, sources, and profile-sensitive follow-ups all live in one thread.",
    status: "active" as const,
  },
];

export const AI_ELEMENTS_TOOL_INPUT = {
  command:
    "npx ai-elements@latest add conversation message prompt-input model-selector plan queue reasoning chain-of-thought tool confirmation sources suggestion",
  preserve: [
    "Keep existing shared button, card, input, and textarea primitives intact",
    "Do not touch the New Thread empty state",
    "Reuse Settings-managed profiles for composer switching",
  ],
  targetSurface: "existing thread content + composer",
};

export const AI_ELEMENTS_TOOL_SUCCESS_OUTPUT = {
  next: "Replace the static conversation body with an AI Elements task flow.",
  reusedPrimitives: ["button", "card", "input", "textarea"],
  status: "installed",
};

export const AI_ELEMENTS_SOURCES = [
  {
    href: "https://ai-sdk.dev/elements/components/plan",
    title: "AI Elements Plan",
  },
  {
    href: "https://ai-sdk.dev/elements/components/prompt-input",
    title: "AI Elements Prompt Input",
  },
  {
    href: "https://ai-sdk.dev/elements/components/tool",
    title: "AI Elements Tool",
  },
  {
    href: "https://ai-sdk.dev/elements/components/reasoning",
    title: "AI Elements Reasoning",
  },
  {
    href: "https://ai-sdk.dev/elements/components/sources",
    title: "AI Elements Sources",
  },
  {
    href: "https://ai-sdk.dev/elements/components/chain-of-thought",
    title: "AI Elements Chain of Thought",
  },
];

export const AI_ELEMENTS_SUGGESTIONS = [
  "让 Queue 在批准后实时更新",
  "把 Sources 压缩成更紧凑的引用块",
  "给 Profile 切换补充状态提示",
];

const RESPONSE_STYLE_LABELS: Record<AgentProfile["responseStyle"], string> = {
  balanced: "Balanced",
  concise: "Concise",
  guide: "Guide",
};

export function getProfileToneLabel(profile: AgentProfile) {
  return RESPONSE_STYLE_LABELS[profile.responseStyle];
}

function resolveProviderModel(
  providers: ReadonlyArray<ProviderEntry>,
  value: string,
) {
  const normalized = value.trim().toLowerCase();

  for (const provider of providers) {
    for (const model of provider.models) {
      const candidates = [
        model.modelId,
        `${provider.name}/${model.modelId}`,
      ].map((entry) => entry.trim().toLowerCase());

      if (candidates.includes(normalized)) {
        return {
          displayName: model.displayName || model.modelId,
          modelId: model.modelId,
        };
      }
    }
  }

  return null;
}

function getFallbackProviderModel(providers: ReadonlyArray<ProviderEntry>) {
  for (const provider of providers) {
    if (!provider.enabled) {
      continue;
    }

    for (const model of provider.models) {
      if (!model.enabled) {
        continue;
      }

      return {
        displayName: model.displayName || model.modelId,
        modelId: model.modelId,
      };
    }
  }

  return null;
}

function resolveProfilePrimaryModel(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  const modelId = profile.primaryModel || profile.assistantModel || profile.liteModel || "";

  if (modelId) {
    const providerModel = resolveProviderModel(providers, modelId);
    if (providerModel) {
      return providerModel;
    }

    const compactName = modelId.includes("/") ? modelId.split("/").pop() ?? modelId : modelId;
    return {
      displayName: compactName.trim() || modelId,
      modelId,
    };
  }

  const fallbackProviderModel = getFallbackProviderModel(providers);
  if (fallbackProviderModel) {
    return fallbackProviderModel;
  }

  return {
    displayName: profile.name || "当前 Profile",
    modelId: profile.name || "当前 Profile",
  };
}

export function getProfilePrimaryModelId(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  return resolveProfilePrimaryModel(profile, providers).modelId;
}

export function getProfilePrimaryModelLabel(
  profile: AgentProfile,
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  return resolveProfilePrimaryModel(profile, providers).displayName;
}

export function getProfileSelectorProvider(profile: AgentProfile) {
  return matchModelIcon(getProfilePrimaryModelId(profile) || profile.name) ?? "zenmux";
}

export function buildProfileAwareFollowUp(
  profile: AgentProfile,
  userText: string,
  attachmentNames: Array<string> = [],
  providers: ReadonlyArray<ProviderEntry> = [],
) {
  const requestSummary = userText.trim().replace(/\s+/gu, " ");
  const attachmentSummary = attachmentNames.length > 0 ? attachmentNames.join("、") : "";
  const modelLabel = getProfilePrimaryModelLabel(profile, providers);

  if (profile.responseStyle === "concise") {
    return {
      body: [
        `已切换到 **${profile.name}**，我会按更精简的方式处理这条 follow-up。`,
        `- 聚焦请求：${requestSummary}`,
        ...(attachmentSummary ? [`- 附件上下文：${attachmentSummary}`] : []),
        "- 先落具体 UI 变化，再补最小验证",
        "- 保持会话流、Queue 联动和 Profile 入口都不分叉",
      ].join("\n"),
      label: `${profile.name} · ${modelLabel}`,
    };
  }

  if (profile.responseStyle === "guide") {
    return {
      body: [
        `已切换到 **${profile.name}**，这轮我会更偏引导式地推进。`,
        "",
        `1. 先界定“${requestSummary}”会落到哪一段会话 UI。`,
        ...(attachmentSummary ? [`2. 把附件“${attachmentSummary}”并入同一条任务上下文，确认上传演示是原生 PromptInput 能力。`] : []),
        `${attachmentSummary ? "3" : "2"}. 再确认它会不会影响 Tool、Queue 或 Sources 的状态表达。`,
        `${attachmentSummary ? "4" : "3"}. 最后补一轮验证，确保已有会话和 New Thread 的职责边界仍然清晰。`,
      ].join("\n"),
      label: `${profile.name} · ${modelLabel}`,
    };
  }

  return {
    body: [
      `已切换到 **${profile.name}**。我会用更均衡的方式处理“${requestSummary}”。`,
      "",
      ...(attachmentSummary ? [`这次还会把附件 ${attachmentSummary} 一起纳入演示上下文，确保上传入口、预览和提交后的线程回显都能对齐。`, ""] : []),
      "重点仍然会放在三件事：让会话叙事连续、让 profile 入口在 composer 里自然可见，以及让后续执行状态继续通过 queue 和 tool call 同步反馈。",
    ].join("\n"),
    label: `${profile.name} · ${modelLabel}`,
  };
}
