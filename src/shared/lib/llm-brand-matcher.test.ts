import { describe, expect, it } from "vitest";
import { matchModelIcon, matchProviderIcon } from "@/shared/lib/llm-brand-matcher";

// ---------------------------------------------------------------------------
// matchProviderIcon
// ---------------------------------------------------------------------------

describe("matchProviderIcon", () => {
  it("matches exact provider names", () => {
    expect(matchProviderIcon("openai")).toBe("openai");
    expect(matchProviderIcon("anthropic")).toBe("anthropic");
    expect(matchProviderIcon("deepseek")).toBe("deepseek");
    expect(matchProviderIcon("google")).toBe("google");
  });

  it("is case-insensitive", () => {
    expect(matchProviderIcon("OpenAI")).toBe("openai");
    expect(matchProviderIcon("ANTHROPIC")).toBe("anthropic");
    expect(matchProviderIcon("DeepSeek")).toBe("deepseek");
  });

  it("matches compound names", () => {
    expect(matchProviderIcon("Azure OpenAI")).toBe("azure");
    expect(matchProviderIcon("Google Gemini")).toBe("gemini");
    expect(matchProviderIcon("Alibaba Cloud")).toBe("alibabacloud");
    expect(matchProviderIcon("Silicon Cloud")).toBe("siliconcloud");
  });

  it("matches Chinese characters", () => {
    expect(matchProviderIcon("智谱")).toBe("zhipu");
  });

  it("returns null for unknown provider", () => {
    expect(matchProviderIcon("")).toBeNull();
    expect(matchProviderIcon("my-custom-api")).toBeNull();
    expect(matchProviderIcon("totally-unknown-provider-xyz")).toBeNull();
  });

  it("matches specific brands", () => {
    expect(matchProviderIcon("Claude")).toBe("claude");
    expect(matchProviderIcon("Moonshot")).toBe("moonshot");
    expect(matchProviderIcon("kimi")).toBe("moonshot");
    expect(matchProviderIcon("qwen")).toBe("qwen");
    expect(matchProviderIcon("ollama")).toBe("ollama");
    expect(matchProviderIcon("groq")).toBe("groq");
    expect(matchProviderIcon("mistral")).toBe("mistral");
  });

  it("matches brands with spaces and special chars", () => {
    expect(matchProviderIcon("LM Studio")).toBe("lmstudio");
    expect(matchProviderIcon("Hugging Face")).toBe("huggingface");
    expect(matchProviderIcon("Vertex AI")).toBe("vertexai");
  });
});

// ---------------------------------------------------------------------------
// matchModelIcon
// ---------------------------------------------------------------------------

describe("matchModelIcon", () => {
  it("matches model IDs", () => {
    expect(matchModelIcon("claude-3-sonnet")).toBe("claude");
    expect(matchModelIcon("gpt-4o")).toBe("openai");
    expect(matchModelIcon("gemini-1.5-pro")).toBe("gemini");
    expect(matchModelIcon("deepseek-coder")).toBe("deepseek");
    expect(matchModelIcon("qwen-72b")).toBe("qwen");
  });

  it("matches model display names as fallback", () => {
    expect(matchModelIcon("some-random-id", "Claude 3.5 Sonnet")).toBe("claude");
    expect(matchModelIcon("unknown", "GPT-4o")).toBe("openai");
  });

  it("returns null for unrecognized model", () => {
    expect(matchModelIcon("custom-model-123")).toBeNull();
    expect(matchModelIcon("", "")).toBeNull();
  });

  it("matches OpenAI model patterns including regex", () => {
    expect(matchModelIcon("o1-preview")).toBe("openai");
    expect(matchModelIcon("o3-mini")).toBe("openai");
    expect(matchModelIcon("o4-mini")).toBe("openai");
    expect(matchModelIcon("chatgpt-4o")).toBe("openai");
  });

  it("matches various cloud providers", () => {
    // Note: "bedrock-claude" matches "claude" first, not "aws" (rule order matters)
    expect(matchModelIcon("bedrock-claude")).toBe("claude");
    expect(matchModelIcon("copilot-gpt")).toBe("github");
    expect(matchModelIcon("sonar-small")).toBe("perplexity");
    expect(matchModelIcon("nemotron-mini")).toBe("nvidia");
    // Direct aws/bedrock match
    expect(matchModelIcon("bedrock-titan")).toBe("aws");
  });

  it("matches Chinese model brands", () => {
    expect(matchModelIcon("doubao-pro")).toBe("doubao");
    expect(matchModelIcon("hunyuan-turbo")).toBe("hunyuan");
    expect(matchModelIcon("ernie-4.0")).toBe("wenxin");
    expect(matchModelIcon("glm-4")).toBe("chatglm");
  });

  it("prefers modelId match over displayName", () => {
    // modelId matches "claude", so should get "claude" not whatever displayName says
    expect(matchModelIcon("claude-3-opus", "Some OpenAI model")).toBe("claude");
  });

  it("uses displayName when modelId has no match", () => {
    expect(matchModelIcon("xyz-123", "Mistral Large")).toBe("mistral");
  });

  it("maps hunyuan short model aliases like hy3 and hy4 to the hunyuan icon slug", () => {
    expect(matchModelIcon("hy3")).toBe("hunyuan");
    expect(matchModelIcon("hy4")).toBe("hunyuan");
    expect(matchModelIcon("foo/hy5-chat")).toBe("hunyuan");
    expect(matchModelIcon("model-hy12-preview")).toBe("hunyuan");
  });

  it("does not match unrelated names that only contain hy without digits", () => {
    expect(matchModelIcon("hyperbolic")).toBeNull();
    expect(matchModelIcon("my-hybrid-model")).toBeNull();
    expect(matchModelIcon("hy-model")).toBeNull();
  });
});
