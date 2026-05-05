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
