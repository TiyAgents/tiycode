type MatcherPattern = RegExp | string;

type BrandRule = {
  slug: string;
  providerPatterns?: MatcherPattern[];
  modelPatterns?: MatcherPattern[];
};

type PreparedText = {
  normalized: string;
  raw: string;
};

const BRAND_RULES: ReadonlyArray<BrandRule> = [
  { slug: "claude", providerPatterns: ["claude", "claude code", "claudecode"], modelPatterns: ["claude", "claude code", "claudecode"] },
  { slug: "gemini", providerPatterns: ["google gemini", "googlegemini", "gemini"], modelPatterns: ["gemini"] },
  { slug: "gemma", providerPatterns: ["gemma"], modelPatterns: ["gemma"] },
  { slug: "doubao", providerPatterns: ["doubao"], modelPatterns: ["doubao"] },
  { slug: "grok", providerPatterns: ["grok"], modelPatterns: ["grok"] },
  { slug: "sora", providerPatterns: ["sora"], modelPatterns: ["sora"] },
  { slug: "chatglm", providerPatterns: ["chatglm", "chat glm", "glm", "glmv"], modelPatterns: ["chatglm", "chat glm", "glm", "glmv"] },
  { slug: "hunyuan", providerPatterns: ["hunyuan"], modelPatterns: ["hunyuan"] },
  { slug: "longcat", providerPatterns: ["longcat"], modelPatterns: ["longcat"] },
  { slug: "minimax", providerPatterns: ["minimax", "mini max"], modelPatterns: ["minimax", "mini max"] },
  { slug: "nanobanana", providerPatterns: ["nano banana", "nanobanana"], modelPatterns: ["nano banana", "nanobanana"] },
  { slug: "qwen", providerPatterns: ["qwen"], modelPatterns: ["qwen"] },
  { slug: "wenxin", providerPatterns: ["wenxin"], modelPatterns: ["wenxin", "ernie"] },
  { slug: "xiaomimimo", providerPatterns: ["xiaomi mimo", "mimo", "xiaomimimo"], modelPatterns: ["xiaomi mimo", "mimo", "xiaomimimo"] },
  { slug: "ai360", providerPatterns: ["ai360", "360 ai"], modelPatterns: ["ai360", "360 ai"] },
  { slug: "aihubmix", providerPatterns: ["aihubmix", "ai hub mix"], modelPatterns: ["aihubmix", "ai hub mix"] },
  { slug: "deepseek", providerPatterns: ["deepseek"], modelPatterns: ["deepseek"] },
  { slug: "moonshot", providerPatterns: ["moonshot", "kimi", "kimi coding", "kimicoding"], modelPatterns: ["moonshot", "kimi", "kimi coding", "kimicoding"] },
  { slug: "stepfun", providerPatterns: ["stepfun", "step fun"], modelPatterns: ["stepfun", "step fun"] },
  { slug: "mistral", providerPatterns: ["mistral"], modelPatterns: ["mistral"] },
  { slug: "openrouter", providerPatterns: ["openrouter", "open router"], modelPatterns: ["openrouter", "open router"] },
  { slug: "zenmux", providerPatterns: ["zenmux", "zen mux"], modelPatterns: ["zenmux", "zen mux"] },
  { slug: "llamaindex", providerPatterns: ["llamaindex", "llama index"], modelPatterns: ["llamaindex", "llama index"] },
  { slug: "meta", providerPatterns: ["meta"], modelPatterns: ["llama", "meta"] },
  { slug: "vertexai", providerPatterns: ["vertex ai", "vertexai"], modelPatterns: ["vertex ai", "vertexai"] },
  { slug: "googlecloud", providerPatterns: ["google cloud", "googlecloud"], modelPatterns: ["google cloud", "googlecloud"] },
  { slug: "azure", providerPatterns: ["azure openai", "azureopenai", "azure ai", "azureai", "azure"], modelPatterns: ["azure ai", "azureai", "azure"] },
  { slug: "alibabacloud", providerPatterns: ["alibaba cloud", "alibabacloud"], modelPatterns: ["alibaba cloud", "alibabacloud"] },
  { slug: "bailian", providerPatterns: ["bailian"], modelPatterns: ["bailian"] },
  { slug: "alibaba", providerPatterns: ["alibaba"], modelPatterns: ["alibaba"] },
  { slug: "antgroup", providerPatterns: ["ant group", "antgroup"], modelPatterns: ["ant group", "antgroup"] },
  { slug: "aws", providerPatterns: ["aws", "amazon web services"], modelPatterns: ["aws", "bedrock"] },
  { slug: "baiducloud", providerPatterns: ["baidu cloud", "baiducloud"], modelPatterns: ["baidu cloud", "baiducloud"] },
  { slug: "baidu", providerPatterns: ["baidu"], modelPatterns: ["baidu"] },
  { slug: "bilibili", providerPatterns: ["bilibili"], modelPatterns: ["bilibili"] },
  { slug: "bytedance", providerPatterns: ["bytedance", "byte dance"], modelPatterns: ["bytedance", "byte dance"] },
  { slug: "cloudflare", providerPatterns: ["cloudflare"], modelPatterns: ["cloudflare", "workers ai", "workersai"] },
  { slug: "cohere", providerPatterns: ["cohere"], modelPatterns: ["cohere", "command a", "command r", "commanda"] },
  { slug: "deepinfra", providerPatterns: ["deepinfra", "deep infra"], modelPatterns: ["deepinfra", "deep infra"] },
  { slug: "deepmind", providerPatterns: ["deepmind", "deep mind"], modelPatterns: ["deepmind", "deep mind"] },
  { slug: "fireworks", providerPatterns: ["fireworks"], modelPatterns: ["fireworks"] },
  { slug: "giteeai", providerPatterns: ["gitee ai", "giteeai"], modelPatterns: ["gitee ai", "giteeai"] },
  { slug: "github", providerPatterns: ["github", "git hub"], modelPatterns: ["github", "copilot"] },
  { slug: "groq", providerPatterns: ["groq"], modelPatterns: ["groq"] },
  { slug: "huawei", providerPatterns: ["huawei"], modelPatterns: ["huawei"] },
  { slug: "huggingface", providerPatterns: ["hugging face", "huggingface"], modelPatterns: ["hugging face", "huggingface"] },
  { slug: "ibm", providerPatterns: ["ibm"], modelPatterns: ["ibm", "watsonx"] },
  { slug: "jina", providerPatterns: ["jina ai", "jinaai", "jina"], modelPatterns: ["jina ai", "jinaai", "jina"] },
  { slug: "kluster", providerPatterns: ["kluster"], modelPatterns: ["kluster"] },
  { slug: "lmstudio", providerPatterns: ["lm studio", "lmstudio"], modelPatterns: ["lm studio", "lmstudio"] },
  { slug: "modelscope", providerPatterns: ["model scope", "modelscope"], modelPatterns: ["model scope", "modelscope"] },
  { slug: "newapi", providerPatterns: ["new api", "newapi"], modelPatterns: ["new api", "newapi"] },
  { slug: "novita", providerPatterns: ["novita"], modelPatterns: ["novita"] },
  { slug: "nvidia", providerPatterns: ["nvidia"], modelPatterns: ["nvidia", "nemotron"] },
  { slug: "ollama", providerPatterns: ["ollama"], modelPatterns: ["ollama"] },
  { slug: "perplexity", providerPatterns: ["perplexity"], modelPatterns: ["perplexity", "sonar"] },
  { slug: "ppio", providerPatterns: ["ppio"], modelPatterns: ["ppio"] },
  { slug: "qiniu", providerPatterns: ["qiniu", "qiniu cloud"], modelPatterns: ["qiniu"] },
  { slug: "siliconcloud", providerPatterns: ["silicon cloud", "siliconcloud", "siliconflow", "silicon flow", "silicon"], modelPatterns: ["silicon cloud", "siliconcloud", "siliconflow", "silicon flow", "silicon"] },
  { slug: "statecloud", providerPatterns: ["state cloud", "statecloud"], modelPatterns: ["state cloud", "statecloud"] },
  { slug: "tencentcloud", providerPatterns: ["tencent cloud", "tencentcloud", "tencent clude", "tencentclude"], modelPatterns: ["tencent cloud", "tencentcloud", "tencent clude", "tencentclude"] },
  { slug: "tencent", providerPatterns: ["tencent"], modelPatterns: ["tencent"] },
  { slug: "together", providerPatterns: ["together ai", "together"], modelPatterns: ["together ai", "together"] },
  { slug: "vercel", providerPatterns: ["vercel"], modelPatterns: ["vercel", "v0"] },
  { slug: "vllm", providerPatterns: ["vllm", "v llm"], modelPatterns: ["vllm", "v llm"] },
  { slug: "volcengine", providerPatterns: ["volcengine", "volc engine"], modelPatterns: ["volcengine", "volc engine"] },
  { slug: "xai", providerPatterns: ["xai", "x ai"], modelPatterns: ["xai", "x ai"] },
  { slug: "zhipu", providerPatterns: ["zhipu", "智谱", "zai", "z ai"], modelPatterns: ["zhipu", "智谱", "zai", "z ai"] },
  { slug: "opencode", providerPatterns: ["opencode", "open code", "opencode go", "opencode-go"], modelPatterns: ["opencode", "open code"] },
  { slug: "anthropic", providerPatterns: ["anthropic"], modelPatterns: ["anthropic"] },
  { slug: "openai", providerPatterns: ["openai"], modelPatterns: ["openai", "chatgpt", "gpt", /(^|[^a-z0-9])o(?:1|3|4)(?:[^a-z0-9]|$)/u] },
  { slug: "google", providerPatterns: ["google"], modelPatterns: ["google"] },
  { slug: "apple", providerPatterns: ["apple"], modelPatterns: ["apple"] },
  { slug: "microsoft", providerPatterns: ["microsoft"], modelPatterns: ["microsoft"] },
];

export function matchProviderIcon(name: string) {
  return matchBrand(name, "providerPatterns");
}

export function matchModelIcon(modelId: string, displayName?: string) {
  return matchBrand(modelId, "modelPatterns") ?? matchBrand(displayName ?? "", "modelPatterns");
}

function matchBrand(input: string, ruleKey: "providerPatterns" | "modelPatterns") {
  const prepared = prepareText(input);

  if (!prepared.raw) return null;

  for (const rule of BRAND_RULES) {
    const patterns = rule[ruleKey];
    if (!patterns?.length) continue;
    if (patterns.some((pattern) => matchesPattern(prepared, pattern))) {
      return rule.slug;
    }
  }

  return null;
}

function matchesPattern(prepared: PreparedText, pattern: MatcherPattern) {
  if (pattern instanceof RegExp) {
    return pattern.test(prepared.raw);
  }

  const normalizedPattern = normalizeForMatching(pattern);
  return prepared.raw.includes(pattern.toLowerCase()) || prepared.normalized.includes(` ${normalizedPattern} `);
}

function prepareText(value: string): PreparedText {
  const raw = value.trim().toLowerCase();
  return {
    raw,
    normalized: ` ${normalizeForMatching(value)} `,
  };
}

function normalizeForMatching(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, " ")
    .replace(/\s+/gu, " ")
    .trim();
}
