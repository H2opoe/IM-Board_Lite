export type Platform = "wechat" | "wecom" | "feishu" | "dingtalk";

export interface ImProfile {
  id: string;
  platform: Platform;
  label: string;
  enabled: boolean;
  configJson: Record<string, unknown>;
  status: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface DingtalkIdentity {
  corpId: string;
  orgName: string;
  userId: string;
  userName: string;
}

export interface AccountIdentity {
  platform: Extract<Platform, "feishu" | "dingtalk">;
  tenantId: string;
  tenantName: string;
  userId: string;
  userName: string;
}

export interface DashboardMetric {
  key: string;
  label: string;
  value: number;
  sources: SourceStat[];
}

export interface SourceStat {
  profileId: string;
  platform: Platform;
  platformLabel: string;
  remark: string;
  label: string;
  count: number;
  chats: string[];
}

export interface ActionItem {
  id: string;
  itemType: "task" | "reply" | "attention";
  status: "open" | "done" | "ignored";
  priority: "high" | "medium" | "low";
  title: string;
  description: string;
  suggestedReply?: string;
  profileId: string;
  platform: Platform;
  platformLabel: string;
  platformRemark: string;
  sourceLabel: string;
  chatId: string;
  chatName: string;
  evidenceSummary: string;
  contextIncomplete: boolean;
  carryOver: boolean;
  sourceMessageAt: string;
  lastUpdatedAt: string;
  completedAt?: string;
}

export interface DashboardData {
  day: string;
  metrics: DashboardMetric[];
  replies: ActionItem[];
  tasks: ActionItem[];
  topics: Array<{ title: string; summary: string; count?: number; sourceChats?: Array<{ chatName: string; isGroup: boolean }>; sources?: SourceStat[] }>;
  chatRank: Array<{ chat: string; count: number; sourceLabel?: string }>;
  speakerTop: Array<{ speaker: string; count: number; sourceLabel?: string }>;
  hourlyActivity: Array<{ hour: string; count: number; sources?: SourceStat[] }>;
  messageTypes: Array<{ type: string; count: number; sources?: SourceStat[] }>;
  keywords: Array<{ text: string; weight: number; count?: number; sources?: SourceStat[] }>;
  aiStatus: "not_configured" | "ready" | "analyzing" | "failed";
  syncStatus: string;
}

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

export interface AppSettings {
  cacheClearTime: string;
}

export interface DiagnosticExport {
  filePath: string;
}

export interface SyncResult {
  profileId: string;
  syncStatus: string;
  aiStatus: DashboardData["aiStatus"];
  insertedMessages: number;
  analyzedMessages: number;
  warnings: string[];
  startedAt: string;
  finishedAt: string;
}

export interface WechatCandidate {
  id: string;
  label: string;
  pid: number;
  relatedPids?: number[];
  bundleId: string;
  containerId?: string;
  appPath: string;
  cliPath?: string;
  cliVersion?: string;
  runtime?: string;
  setupMode?: string;
  requiresPassword?: boolean;
  signatureStatus?: string;
  hasGetTaskAllow?: boolean;
  needsResign?: boolean;
  requiresAppManagementPermission?: boolean;
  signingReadError?: string;
  dataDir: string;
  wechatFilesPath?: string;
  dbDir: string;
  profileDir?: string;
  configPath?: string;
  keysPath?: string;
  cacheDir?: string;
  candidateDbDirs?: WechatDbCandidate[];
  running?: boolean;
  lastModified?: string;
  confidence?: string;
}

export interface WechatDbCandidate {
  path: string;
  score: number;
  confidence: string;
  valid: boolean;
  lastModified: string;
  hasSessionDb: boolean;
  hasMessageDir: boolean;
  hasMessageDb: boolean;
  hasContactDb: boolean;
  dbCount: number;
  messageDbCount: number;
}
