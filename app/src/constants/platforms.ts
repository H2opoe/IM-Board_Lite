import type { Platform } from "../types";

export interface PlatformBindingOption {
  id: Platform;
  label: string;
  auth: string;
}

export const PLATFORM_LABELS: Record<Platform, string> = {
  wechat: "微信",
  wecom: "企业微信",
  feishu: "飞书",
  dingtalk: "钉钉"
};

export const PLATFORM_BINDING_OPTIONS: PlatformBindingOption[] = [
  { id: "wechat", label: PLATFORM_LABELS.wechat, auth: "读取本机微信数据" },
  { id: "wecom", label: PLATFORM_LABELS.wecom, auth: "使用官方 CLI 授权" },
  { id: "feishu", label: PLATFORM_LABELS.feishu, auth: "使用官方 CLI 授权" },
  { id: "dingtalk", label: PLATFORM_LABELS.dingtalk, auth: "使用官方 CLI 授权" }
];

export const PROFILE_STATUS_LABELS: Record<string, string> = {
  normal: "正常",
  needs_auth: "需要授权",
  needs_key_refresh: "需要刷新密钥",
  missing_data_dir: "数据目录不可访问",
  disabled: "已禁用",
  error: "错误"
};

export function platformLabel(platform: Platform): string {
  return PLATFORM_LABELS[platform];
}
