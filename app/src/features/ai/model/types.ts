export interface AiConfig {
  provider: string;
  apiKey: string;
  apiKeyConfigured: boolean;
  clearApiKey: boolean;
  baseUrl: string;
  model: string;
  userPrompt: string;
  analysisPrompt: string;
  summaryPrompt: string;
  analysisPromptCustom: boolean;
  summaryPromptCustom: boolean;
  analysisBatchSize: number;
  enabled: boolean;
  testStatus: string;
}

export type AiConfigView = Omit<AiConfig, "apiKey" | "clearApiKey">;
export type AiConfigInput = AiConfig;

export interface LocalModelStatus {
  provider: string;
  model: string;
  fileName: string;
  filePath: string;
  sourceUrl: string;
  installed: boolean;
  sizeBytes: number;
  expectedSizeBytes: number;
  updatedAt: string;
}

export interface LocalModelDownloadProgress {
  provider: string;
  model: string;
  status: "starting" | "downloading" | "done" | "failed" | "cancelled";
  downloadedBytes: number;
  totalBytes: number;
  percent: number;
}

export interface SystemCapabilities {
  os: string;
  architecture: string;
  logicalCpuCores: number;
  physicalCpuCores: number;
  totalMemoryBytes: number;
  availableMemoryBytes: number;
  availableDiskBytes: number;
  recommendedAnalysisBatchSize: number;
  localModelSupported: boolean;
  warnings: string[];
}
