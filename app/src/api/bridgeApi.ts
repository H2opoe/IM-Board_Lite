import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ImProfile, Platform } from "../features/profiles/model/types";
import { isTauri, requireTauri } from "./tauri";

const PLATFORM_CLI_DEPLOYMENT_PROGRESS_EVENT = "platform-cli-deployment-progress";

interface BridgeEnvelope<T = unknown> {
  ok: boolean;
  data: T;
  warnings?: string[];
  error?: {
    code?: string;
    message: string;
  };
}

export interface PlatformDeployment {
  platform: Platform;
  cliPath: string;
  configDir: string;
  command: string;
  commandShell?: string;
  source: string;
  currentVersion: string;
}

export interface PlatformCliDeploymentProgress {
  platform: Platform;
  profileId: string;
  phase: string;
  message: string;
  current: number;
  total: number;
  registry?: string;
  packageName?: string;
  version?: string;
}

export interface PlatformCliVersionStatus {
  platform: Platform;
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  source: string;
  checkedAt: string;
}

export async function runBridgeCommand(profile: ImProfile, command: string, args: Record<string, string>) {
  const envelope = await invoke<BridgeEnvelope>("run_bridge_command", {
    request: {
      platform: profile.platform,
      command,
      profile,
      args
    }
  });
  if (!envelope.ok) {
    const detail = formatBridgeError(envelope);
    throw new Error(detail || "账号配置读取测试失败");
  }
  return envelope;
}

export async function deployPlatformBridge(platform: Platform, profileId: string): Promise<PlatformDeployment> {
  if (isTauri) return invoke("deploy_platform_bridge", { platform, profileId });
  return requireTauri("准备平台CLI");
}

export async function watchPlatformCliDeploymentProgress(
  handler: (progress: PlatformCliDeploymentProgress) => void
): Promise<() => void> {
  if (isTauri) {
    return listen<PlatformCliDeploymentProgress>(PLATFORM_CLI_DEPLOYMENT_PROGRESS_EVENT, (event) => handler(event.payload));
  }
  void handler;
  return requireTauri("监听平台CLI准备进度");
}

export async function checkPlatformCliUpdate(platform: Platform): Promise<PlatformCliVersionStatus> {
  if (isTauri) return invoke("check_platform_cli_update", { platform });
  return requireTauri("检查平台CLI更新");
}

export async function updatePlatformCli(platform: Platform): Promise<PlatformCliVersionStatus> {
  if (isTauri) return invoke("update_platform_cli", { platform });
  return requireTauri("更新平台CLI");
}

export async function cleanupUnusedPlatformCli(platform: Platform): Promise<boolean> {
  if (isTauri) return invoke("cleanup_unused_platform_cli", { platform });
  return requireTauri("清理未使用的平台CLI");
}

function formatBridgeError(envelope: { warnings?: string[]; error?: { code?: string; message: string } }) {
  if (
    envelope.error?.code === "WECOM_MESSAGE_PERMISSION_UNSUPPORTED" ||
    envelope.error?.code === "WECOM_NOT_AUTHENTICATED" ||
    envelope.error?.code === "FEISHU_NOT_AUTHENTICATED" ||
    envelope.error?.code === "DINGTALK_NOT_AUTHENTICATED" ||
    envelope.error?.code === "DINGTALK_MESSAGE_PERMISSION_MISSING"
  ) {
    return envelope.error.message;
  }
  if (envelope.error?.code === "WECHAT_NOT_LOGGED_IN") {
    return envelope.error.message || "微信读取不到消息，请确认电脑微信是否已登录后重试。";
  }
  const label = bridgeErrorCodeLabel(envelope.error?.code);
  const message = envelope.error?.message;
  return [isBridgeErrorLabelRedundant(label, message) ? "" : label, message, ...(envelope.warnings ?? [])]
    .filter(Boolean)
    .join("\n");
}
function isBridgeErrorLabelRedundant(label?: string, message?: string) {
  if (!label || !message) return false;
  const normalizedLabel = normalizeBridgeErrorPrefix(label);
  const normalizedMessage = normalizeBridgeErrorPrefix(message);
  return normalizedMessage.startsWith(normalizedLabel);
}

function normalizeBridgeErrorPrefix(value: string) {
  return value.replace(/\s+/g, "").replace(/官方/g, "");
}

function bridgeErrorCodeLabel(code?: string) {
  if (!code) return "";
  const labels: Record<string, string> = {
    BRIDGE_CRASHED: "桥接进程执行失败",
    BRIDGE_SPAWN_FAILED: "桥接进程启动失败",
    MISSING_CHAT: "缺少会话参数",
    WECHAT_NOT_LOGGED_IN: "微信未登录",
    WECOM_PROFILE_NOT_FOUND: "缺少企业微信账号配置",
    WECOM_UNSUPPORTED_COMMAND: "企业微信命令不支持",
    WECOM_CLI_MISSING: "企业微信CLI不可用",
    WECOM_CLI_FAILED: "企业微信CLI执行失败",
    WECOM_API_ERROR: "企业微信接口返回错误",
    WECOM_NOT_AUTHENTICATED: "企业微信尚未登录",
    FEISHU_PROFILE_NOT_FOUND: "缺少飞书账号配置",
    FEISHU_UNSUPPORTED_COMMAND: "飞书命令不支持",
    FEISHU_CLI_MISSING: "飞书CLI不可用",
    FEISHU_CLI_FAILED: "飞书CLI执行失败",
    FEISHU_NOT_AUTHENTICATED: "飞书尚未登录",
    FEISHU_B2C_APP_UNSUPPORTED: "飞书应用会话不支持读取",
    DINGTALK_PROFILE_NOT_FOUND: "缺少钉钉账号配置",
    DINGTALK_UNSUPPORTED_COMMAND: "钉钉命令不支持",
    DINGTALK_CLI_MISSING: "钉钉CLI不可用",
    DINGTALK_MESSAGE_PERMISSION_MISSING: "钉钉消息读取权限不足",
    DINGTALK_CLI_FAILED: "钉钉CLI执行失败"
  };
  return labels[code] || "桥接错误";
}
