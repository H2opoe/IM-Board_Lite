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
  platform: Extract<Platform, "wecom" | "feishu" | "dingtalk">;
  tenantId: string;
  tenantName: string;
  userId: string;
  userName: string;
}
