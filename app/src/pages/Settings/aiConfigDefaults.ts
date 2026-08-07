import type { AiConfig } from "../../features/ai/model/types";

export const providerDefaults: Record<string, { baseUrl: string; model: string }> = {
  本地DeepSeek: { baseUrl: "http://127.0.0.1:11434/v1", model: "deepseek-r1-distill-qwen-7b-q4_k_m" },
  "DeepSeek API": { baseUrl: "https://api.deepseek.com", model: "deepseek-v4-flash" },
  OpenRouter: { baseUrl: "https://openrouter.ai/api/v1", model: "deepseek/deepseek-v3.2" },
  火山方舟: { baseUrl: "https://ark.cn-beijing.volces.com/api/v3", model: "doubao-seed-1-6-251015" },
  其他本地模型: { baseUrl: "http://127.0.0.1:11434/v1", model: "" }
};

export const providerOptions = ["本地DeepSeek", "火山方舟", "DeepSeek API", "OpenRouter", "其他本地模型"];
export const localDeepseekDisplayName = "DeepSeek-R1-Distill-Qwen-7B Q4_K_M";
export const localDeepseekEnablePendingKey = "imboard:local-deepseek-enable-pending";
export const localDeepseekDefaultBatchSize = 20;

const legacyLocalDeepseekModels = new Set(["deepseek-r1-distill-qwen-1.5b-q4_k_m"]);
const minAnalysisBatchSize = 10;
const localDeepseekMaxBatchSize = 30;
const legacyOtherModelDefaultBatchSize = 50;
const otherModelDefaultBatchSize = 100;
const otherModelMaxBatchSize = 300;

export const emptyConfig: AiConfig = {
  provider: "本地DeepSeek",
  apiKey: "",
  apiKeyConfigured: false,
  clearApiKey: false,
  baseUrl: providerDefaults["本地DeepSeek"].baseUrl,
  model: providerDefaults["本地DeepSeek"].model,
  userPrompt: "",
  analysisPrompt: "",
  summaryPrompt: "",
  analysisPromptCustom: false,
  summaryPromptCustom: false,
  analysisBatchSize: localDeepseekDefaultBatchSize,
  enabled: true,
  testStatus: "untested"
};

export function normalizeConfig(config: Partial<AiConfig> & Pick<AiConfig, "provider">): AiConfig {
  const normalizedProvider = config.provider === "火山引擎" ? "火山方舟" : config.provider;
  const provider = normalizedProvider === "本地模型"
    ? "其他本地模型"
    : providerOptions.includes(normalizedProvider)
      ? normalizedProvider
      : "本地DeepSeek";
  const defaults = providerDefaults[provider];
  const providerChanged = provider !== config.provider;
  const currentModel = config.model ?? "";
  const currentBaseUrl = config.baseUrl ?? "";
  const model = providerChanged || !currentModel.trim() || (provider === "本地DeepSeek" && legacyLocalDeepseekModels.has(currentModel))
    ? defaults.model
    : currentModel;
  return {
    ...emptyConfig,
    ...config,
    provider,
    apiKey: provider.includes("本地") ? "" : (config.apiKey ?? ""),
    apiKeyConfigured: Boolean(config.apiKeyConfigured),
    clearApiKey: Boolean(config.clearApiKey),
    baseUrl: providerChanged || !currentBaseUrl.trim() ? defaults.baseUrl : currentBaseUrl,
    model,
    analysisBatchSize: normalizeAnalysisBatchSize(provider, config.analysisBatchSize ?? 0),
    enabled: config.enabled ?? true
  };
}

export function defaultAnalysisBatchSize(provider: string): number {
  return provider === "本地DeepSeek" ? localDeepseekDefaultBatchSize : otherModelDefaultBatchSize;
}

function normalizeAnalysisBatchSize(provider: string, value: number): number {
  const fallback = defaultAnalysisBatchSize(provider);
  const max = provider === "本地DeepSeek" ? localDeepseekMaxBatchSize : otherModelMaxBatchSize;
  if (provider !== "本地DeepSeek" && value === legacyOtherModelDefaultBatchSize) return fallback;
  const requested = Number.isFinite(value) && value > 0 ? value : fallback;
  return Math.max(minAnalysisBatchSize, Math.min(max, requested));
}
