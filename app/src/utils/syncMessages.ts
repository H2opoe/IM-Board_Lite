import { userErrorMessage } from "./errors";

export type SyncMaintenanceAction = "retry-analysis" | "full-resync";
type SyncMessageState = "idle" | "syncing" | "analyzing" | "done" | "failed";

export const SYNC_CANCEL_CONFIRM_MESSAGES: Partial<Record<SyncMessageState, string>> = {
  syncing: "再次点击“终止同步”将停止当前同步进程。",
  analyzing: "再次点击“终止分析”将停止当前AI分析。"
};

export function syncErrorMessage(error: unknown): string {
  return userErrorMessage(error, "同步失败。");
}

export function shouldShowSyncWarning(warning: string): boolean {
  const text = warning.trim();
  if (/^\[page \d+\]\s+(fetching|fetching\.\.\.|fetched .*)$/i.test(text)) return false;
  if (/^飞书：已(?:获取到|发现)\s*\d+\s*个可见群聊，但今天(?:窗口内)?没有可读消息/.test(text)) return false;
  if (/^钉钉：今天没有读到可读消息。?$/.test(text)) return false;
  if (/^钉钉\s*\/.+(?:DINGTALK_MESSAGE_PERMISSION_MISSING|消息读取权限|CLI 数据访问权限|chat\.message:list)/.test(text)) return false;
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
