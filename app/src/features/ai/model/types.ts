export interface AiConfig {
  provider: string;
  apiKey: string;
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
