export type Platform = "wecom" | "feishu" | "dingtalk";

export interface ConnectorCapability {
  platform: Platform;
  label: string;
  authDescription: string;
  runtimeDependency: string;
  permissions: string[];
  commands: string[];
  healthCheck: string;
}

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
  platform: Platform;
  tenantId: string;
  tenantName: string;
  userId: string;
  userName: string;
}
