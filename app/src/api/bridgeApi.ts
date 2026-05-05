import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ImProfile, Platform, WechatCandidate } from "../types";
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

export async function discoverWechatInstances(): Promise<WechatCandidate[]> {
  if (isTauri) {
    const envelope = await invoke<BridgeEnvelope<WechatCandidate[]>>("run_bridge_command", {
      request: {
        platform: "wechat",
        command: "discover",
        profile: null,
        args: {}
      }
    });
    if (!envelope.ok) {
      const detail = formatBridgeError(envelope);
      throw new Error(detail || "微信实例发现失败");
    }
    return envelope.data;
  }
  return requireTauri("发现微信实例");
}

export async function initializeWechatFiles(profile: ImProfile, candidate: WechatCandidate, sudoPassword = ""): Promise<ImProfile> {
  const requiresPassword = candidate.requiresPassword !== false && candidate.runtime !== "windows_original_cli" && candidate.runtime !== "original_cli";
  if (requiresPassword && !sudoPassword.trim()) throw new Error("请输入本机sudo密码用于一次性初始化。");
  const dbDir = typeof profile.configJson.dbDir === "string" ? profile.configJson.dbDir : candidate.dbDir;
  if (isTauri) {
    const args: Record<string, string> = {};
    if (candidate.pid) args.pid = String(candidate.pid);
    if (candidate.bundleId) args.bundle_id = candidate.bundleId;
    if (candidate.appPath) args.app_path = candidate.appPath;
    if (candidate.cliPath) args.cli_path = candidate.cliPath;
    if (dbDir) args.db_dir = dbDir;
    const envelope = await invoke<BridgeEnvelope>("run_bridge_command", {
      request: {
        platform: "wechat",
        command: "init-profile",
        profile,
        ...(requiresPassword ? { stdinSecret: sudoPassword } : {}),
        args
      }
    });
    if (!envelope.ok) {
      const detail = formatBridgeError(envelope);
      const error = new Error(detail || "微信初始化失败") as Error & { code?: string };
      error.code = envelope.error?.code;
      throw error;
    }
    const initConfig = wechatInitConfig(envelope.data);
    return {
      ...profile,
      configJson: {
        ...profile.configJson,
        ...initConfig
      }
    };
  }
  return requireTauri("初始化微信账号");
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

function wechatInitConfig(data: unknown) {
  if (!data || typeof data !== "object") return {};
  const record = data as Record<string, unknown>;
  const cacheDir = recordString(record, ["cacheDir"]);
  const tmpDir = cacheDir ? `${cacheDir.replace(/[\\/]+$/, "")}${cacheDir.includes("\\") ? "\\" : "/"}tmp` : "";
  return {
    ...(recordString(record, ["cliPath"]) ? { cliPath: recordString(record, ["cliPath"]) } : {}),
    ...(recordString(record, ["configPath"]) ? { configPath: recordString(record, ["configPath"]) } : {}),
    ...(recordString(record, ["keysPath"]) ? { keysPath: recordString(record, ["keysPath"]) } : {}),
    ...(cacheDir ? { cacheDir, tmpDir } : {})
  };
}

function recordString(value: unknown, keys: string[]) {
  if (!value || typeof value !== "object") return "";
  const record = value as Record<string, unknown>;
  const matched = keys.map((key) => record[key]).find((item): item is string => typeof item === "string" && item.trim().length > 0);
  return matched?.trim() ?? "";
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
  const label = bridgeErrorCodeLabel(envelope.error?.code);
  const message = envelope.error?.message;
  const wechatMessage = formatWechatBridgeError(envelope.error?.code, message, envelope.warnings);
  if (wechatMessage) return wechatMessage;
  return [isBridgeErrorLabelRedundant(label, message) ? "" : label, message, ...(envelope.warnings ?? [])]
    .filter(Boolean)
    .join("\n");
}

function formatWechatBridgeError(code?: string, message?: string, warnings?: string[]) {
  const rawDetail = [message, ...(warnings ?? [])].filter(Boolean).join("\n");
  if (
    code === "WECHAT_SIGN_PERMISSION_DENIED" ||
    (code === "WECHAT_SIGN_FAILED" && /Operation not permitted|已阻止修改|App 管理/i.test(rawDetail))
  ) {
    return [
      "微信签名被 macOS 权限拦截：请到 系统设置 > 隐私与安全性 > App 管理，允许 IM-Board 修改 App。",
      "授权后继续开始绑定，已输入的电脑用户密码和备注会保留。",
      rawDetail ? `底层返回：${rawDetail}` : ""
    ]
      .filter(Boolean)
      .join("\n");
  }
  if (code === "WECHAT_RESTART_REQUIRED") {
    return message || "微信已完成重新签名，并已自动重启微信和重新检测。请在微信窗口重新登录账号。";
  }
  if (code === "WECHAT_KEYS_EMPTY") {
    return [
      "微信当前未登录：请先在微信窗口完成登录，然后回到 IM-Board 重试绑定。",
      "如果已经登录，请刷新微信实例并确认选中的是当前登录账号的数据文件夹。",
      warnings?.length ? `诊断信息：${warnings.join("；")}` : ""
    ]
      .filter(Boolean)
      .join("\n");
  }
  return "";
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
    WECHAT_UNSUPPORTED_COMMAND: "微信命令不支持",
    WECHAT_CLI_MISSING: "微信CLI不可用",
    WECHAT_ORIGINAL_CLI_MISSING: "微信CLI不可用",
    WECHAT_ORIGINAL_CLI_INIT_FAILED: "微信CLI初始化失败",
    WECHAT_ORIGINAL_CLI_FAILED: "微信CLI执行失败",
    WECHAT_PROFILE_NOT_FOUND: "缺少微信账号配置",
    WECHAT_RESTART_REQUIRED: "需要重启微信",
    WECHAT_SIGN_PERMISSION_DENIED: "微信签名权限不足",
    WECHAT_SIGN_FAILED: "微信签名失败",
    WECHAT_KEYS_EMPTY: "微信密钥文件为空",
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
