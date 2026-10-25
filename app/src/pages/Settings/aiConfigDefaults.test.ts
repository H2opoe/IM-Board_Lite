import { describe, expect, it } from "vitest";
import { configForProvider, emptyConfig, normalizeConfig, providerDefaults } from "./aiConfigDefaults";

describe("AI settings compatibility", () => {
  it.each([
    ["火山引擎", "火山方舟", "https://fixture.invalid/custom/v1", "custom-cloud-model"],
    ["本地模型", "其他本地模型", "http://127.0.0.1:45678/v1", "custom-local-model"],
  ])("renames %s without replacing the user's endpoint or model", (oldProvider, provider, baseUrl, model) => {
    const result = normalizeConfig({ provider: oldProvider, baseUrl, model });
    expect(result.provider).toBe(provider);
    expect(result.baseUrl).toBe(baseUrl);
    expect(result.model).toBe(model);
    expect(normalizeConfig(result)).toEqual(result);
  });

  it("upgrades only the retired built-in model and preserves custom models", () => {
    const legacy = "deepseek-r1-distill-qwen-1.5b-q4_k_m";
    expect(normalizeConfig({ provider: "本地DeepSeek", model: legacy }).model).toBe(providerDefaults["本地DeepSeek"].model);
    expect(normalizeConfig({ provider: "本地DeepSeek", model: "custom-model" }).model).toBe("custom-model");
    expect(normalizeConfig({ provider: "本地模型", model: legacy }).model).toBe(legacy);
  });

  it("fills blank alias settings with defaults and resets unsupported providers", () => {
    expect(normalizeConfig({ provider: "火山引擎", baseUrl: " ", model: "" })).toMatchObject(providerDefaults["火山方舟"]);
    expect(normalizeConfig({ provider: "unsupported-fixture", baseUrl: "https://fixture.invalid", model: "old-model" })).toMatchObject({ provider: "本地DeepSeek", ...providerDefaults["本地DeepSeek"] });
  });
});

describe("provider switching", () => {
  it.each(["http://127.0.0.1:56423/v1", "http://localhost:49201/v1/", "http://[::1]:51328/v1"])("replaces managed runtime endpoint %s with the chosen provider default", baseUrl => {
    for (const provider of ["火山方舟", "DeepSeek API", "OpenRouter"]) {
      const next = configForProvider({...emptyConfig, baseUrl}, provider);
      expect(next.baseUrl).toBe(providerDefaults[provider].baseUrl);
      expect(next.model).toBe(providerDefaults[provider].model);
      expect(next.analysisBatchSize).toBe(100);
    }
  });
  it("preserves custom settings within a provider and resets the endpoint when switching", () => {
    const config = {...emptyConfig, provider:"火山方舟",baseUrl:"https://fixture.invalid/custom/v1",model:"custom-model"};
    expect(configForProvider(config,"火山方舟")).toEqual(config);
    expect(configForProvider(config,"OpenRouter")).toMatchObject({baseUrl:providerDefaults.OpenRouter.baseUrl,model:config.model});
    const localProxy = {...config,baseUrl:"http://127.0.0.1:56423/v1"};
    expect(configForProvider(localProxy,"OpenRouter").baseUrl).toBe(providerDefaults.OpenRouter.baseUrl);
  });
  it("recognizes static default endpoints with trailing whitespace or slash", () => {
    expect(configForProvider({...emptyConfig,baseUrl:" http://127.0.0.1:11434/v1/ "},"火山方舟").baseUrl).toBe(providerDefaults["火山方舟"].baseUrl);
  });
});

it("uses the reviewed official API base URLs for every provider transition", () => {
  const endpoints: Record<string,string> = {
    "DeepSeek API":"https://api.deepseek.com",
    OpenRouter:"https://openrouter.ai/api/v1",
    火山方舟:"https://ark.cn-beijing.volces.com/api/v3",
    本地DeepSeek:"http://127.0.0.1:11434/v1",
    其他本地模型:"http://127.0.0.1:11434/v1"
  };
  for (const from of Object.keys(endpoints)) {
    for (const [to, baseUrl] of Object.entries(endpoints)) {
      if (from === to) continue;
      expect(configForProvider({...emptyConfig,provider:from,baseUrl:"https://fixture.invalid/previous"},to).baseUrl).toBe(baseUrl);
    }
  }
});
