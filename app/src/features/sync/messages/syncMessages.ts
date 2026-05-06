import { userErrorMessage } from "../../../utils/errors";
import type { SyncJobMode, SyncResult } from "../model/types";

export type SyncMaintenanceAction = "retry-analysis" | "full-resync";
type SyncMessageState = "idle" | "syncing" | "analyzing" | "done" | "failed";

interface SyncJobUiConfig {
  mode: SyncJobMode;
  initialState: SyncMessageState;
  initialMessage: string;
  fallbackMessage: string;
  completedPrefix: string;
}

export const SYNC_JOB_UI_CONFIG: Record<SyncJobMode, SyncJobUiConfig> = {
  incremental: {
    mode: "incremental",
    initialState: "syncing",
    initialMessage: "正在准备读取当前业务日消息...",
    fallbackMessage: "正在读取当前业务日消息...",
    completedPrefix: ""
  },
  full_resync: {
    mode: "full_resync",
    initialState: "syncing",
    initialMessage: "正在清空缓存并准备重新同步...",
    fallbackMessage: "正在重新同步消息...",
    completedPrefix: "重新同步完成，"
  },
  retry_analysis: {
    mode: "retry_analysis",
    initialState: "analyzing",
    initialMessage: "正在清空AI结果并重新生成...",
    fallbackMessage: "正在清空AI结果并重新生成...",
    completedPrefix: "重新生成完成，"
  }
};

export const SYNC_CANCEL_CONFIRM_MESSAGES: Partial<Record<SyncMessageState, string>> = {
  syncing: "再次点击“终止同步”将停止当前同步进程。",
  analyzing: "再次点击“终止分析”将停止当前AI分析。"
};

export const SYNC_RUNTIME_MESSAGES = {
  cancelCurrentTask: "再次点击将停止当前任务。",
  cancellingAnalysis: "正在终止当前AI分析...",
  cancellingSync: "正在终止当前同步进程...",
  analysisStillRunning: "AI分析仍在进行...",
  syncStillRunning: "同步仍在进行...",
  refreshingDashboard: (insertedMessages: number, analyzedMessages: number) =>
    `已读取${insertedMessages}条新消息，AI已识别${analyzedMessages}个事项，正在刷新看板...`
};

export function syncErrorMessage(error: unknown): string {
  return userErrorMessage(error, "同步失败。");
}

export function shouldShowSyncWarning(warning: string): boolean {
  const text = warning.trim();
  if (/^\[page \d+\]\s+(fetching|fetching\.\.\.|fetched .*)$/i.test(text)) return false;
  if (/^飞书：已(?:获取到|发现)\s*\d+\s*个可见群聊，但今天(?:窗口内)?没有可读消息/.test(text)) return false;
  if (/^钉钉：今天没有读到可读消息。?$/.test(text)) return false;
  if (/^钉钉\s*\/.+(?:DINGTALK_MESSAGE_PERMISSION_MISSING|消息读取权限|CLI数据访问权限|chat\.message:list)/.test(text)) return false;
  if (/飞书.*(?:应用|机器人)会话/.test(text) && /231204|b2c app not support|不影响其他会话同步/.test(text)) return false;

  // 这些是平台侧的“部分会话不可读”噪声，不代表整次同步失败；保留真正会影响用户行动的报错。
  return ![/找不到聊天对象/, /fetch-messages\s*执行失败/].some((pattern) => pattern.test(text));
}

export function isSyncMessageError(message: string): boolean {
  const trimmed = message.trim();
  return (
    /执行失败|同步失败|分析失败|权限不足|缺少.*权限|未授权|未登录/.test(trimmed) ||
    /"error"\s*:/.test(trimmed) ||
    /\b(?:DINGTALK|FEISHU|WECOM|WECHAT)_[A-Z0-9_]+/.test(trimmed)
  );
}

export function syncMaintenancePendingMessage(action: SyncMaintenanceAction): string {
  return action === "retry-analysis" ? "当前任务已停止，正在准备重新生成AI分析..." : "当前任务已停止，正在准备重新同步...";
}

export function syncMaintenanceStoppingMessage(action: SyncMaintenanceAction): string {
  return action === "retry-analysis" ? "正在终止当前任务，随后重新生成AI分析..." : "正在终止当前任务，随后重新同步...";
}

export function syncCompletionMessage(mode: SyncJobMode, result: SyncResult): string {
  const prefix = SYNC_JOB_UI_CONFIG[mode].completedPrefix;
  if (result.aiStatus === "not_configured") {
    if (mode === "retry_analysis") return "AI未启用。";
    return `${prefix}已读取${result.insertedMessages}条新消息，AI未启用。`;
  }
  if (mode === "retry_analysis") {
    return `${prefix}AI已识别${result.analyzedMessages}个事项。`;
  }
  return `${prefix}已读取${result.insertedMessages}条新消息，AI分析完成，识别${result.analyzedMessages}个事项。`;
}
