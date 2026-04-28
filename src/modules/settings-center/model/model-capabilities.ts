import type { ProviderModel, ProviderModelCapabilities } from "@/modules/settings-center/model/types";

export function inferModelCapabilities(modelId: string): ProviderModelCapabilities {
  const normalized = modelId.toLowerCase();
  const embedding = /\bembed|embedding\b/.test(normalized);
  const imageOutput = /\b(image|images|gpt-image|flux|sdxl|seedream|dall-e)\b/.test(normalized);
  const vision = /\b(vision|vl|gpt-4o|gpt-4\.1|claude|gemini|pixtral|llava)\b/.test(normalized);
  const reasoning = /\b(gpt-5|o1|o3|o4|r1|reason|reasoner|reasoning|thinking|claude-3\.7|gemini-2\.5|step-3)\b/.test(normalized);
  const toolCalling = !embedding && !imageOutput && /\b(gpt|claude|gemini|deepseek|moonshot|qwen|llama|mistral|step|openai|anthropic|doubao)\b/.test(normalized);

  return {
    vision,
    imageOutput,
    toolCalling,
    reasoning,
    embedding,
  };
}

export function getEffectiveModelCapabilities(model: ProviderModel): ProviderModelCapabilities {
  return {
    ...inferModelCapabilities(model.modelId),
    ...model.capabilityOverrides,
  };
}
