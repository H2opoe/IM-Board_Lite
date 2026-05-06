import type { Platform } from "../features/profiles/model/types";
import { PLATFORM_LABELS } from "./platforms";

export const EMPTY_STATE_MESSAGES = {
  noProfiles: "暂无账号",
  noStats: "暂无统计",
  noMessageTypes: "暂无统计",
  noKeywords: "今天暂未发现",
  noActionItems: "今天暂未发现",
  aiActionPending: "AI分析中，完成后写入列表",
  aiTopicPending: "AI分析中，完成后写入列表"
};

export const APP_MESSAGES = {
  cancel: "取消",
  saveConfig: "保存配置",
  saveSettings: "保存设置",
  settingsSaved: "设置已保存。",
  deleteInProgress: "删除中",
  confirmDelete: "确认删除"
};

export const DASHBOARD_MESSAGES = {
  statusLabels: {
    sync: {
      idle: "空闲",
      synced: "已同步",
      syncing: "同步中",
      analyzing: "分析中",
      failed: "失败",
      cancelled: "已取消"
    },
    ai: {
      not_configured: "未配置",
      ready: "已就绪",
      analyzing: "分析中",
      failed: "异常"
    }
  },
  retryAnalysisDescription: "重新生成会清空当前业务日AI结果，重新分析当前业务日全部消息。历史待回复和待办不会被清除。",
  fullResyncDescription: "重新同步会清空缓存，并重新读取当前业务日消息。历史待回复和待办不会被清除。"
};

export const APP_SETTINGS_MESSAGES = {
  readFailed: "设置读取失败。",
  saveFailed: "设置保存失败。",
  diagnosticsExporting: "正在导出诊断包，请稍候。",
  diagnosticsExported: (filePath: string) => `诊断包已导出到：${filePath}`,
  diagnosticsExportFailed: "诊断包导出失败。"
};

export const PROFILE_MESSAGES = {
  selectForManage: "请先选择需要管理的账号。",
  selectForDelete: "请先选择需要删除的账号。",
  selectForReadTest: "请先选择需要测试读取的账号。",
  profileOrderSaved: "账号排序已更新。",
  profileOrderFailed: "账号排序保存失败。",
  deleteImpact: "删除后会移除此账号配置和同步状态。",
  deleteFailed: "删除账号配置失败。",
  readTestFailed: "账号配置读取测试失败。",
  batchDeleteButton: "批量删除",
  bulkDeletePrompt: (count: number) => `确认删除${count}个账号配置？这会移除所选账号配置和同步状态。`,
  bulkDeleteConfirmButton: (count: number) => `确认删除${count}个`,
  bulkDeleteSuccess: (count: number) => `已删除${count}个账号配置。`,
  bulkDeleteFailed: "批量删除账号配置失败。",
  deletePrompt: (name: string) => `确认删除「${name}」？这会移除该账号配置和同步状态。`,
  batchStatusStart: (action: string, count: number) => `正在${action}${count}个账号…`,
  batchStatusSuccess: (action: string, count: number) => `已${action}${count}个账号。`,
  batchStatusFailed: (action: string) => `${action}账号失败。`,
  readTestStart: (name: string) => `正在测试读取「${name}」…`,
  readTestSuccess: (name: string, detail: string) => `「${name}」${detail}`,
  readTestFailedWithName: (name: string, detail: string) => `「${name}」${detail}`
};

export const OFFICIAL_CLI_MESSAGES = {
  clipboardUnsupported: "当前环境不支持自动复制。",
  copyFailed: "复制命令失败，请手动复制命令详情。",
  checkingVersion: "正在检查官方CLI版本",
  readyStatusTitle: "已加载CLI",
  preparingStatusTitle: "CLI准备中",
  pendingReady: "等待CLI就绪",
  prepareFailed: (platform: Platform) => `${platformDisplayName(platform)}官方CLI准备失败。`,
  versionCheckFailed: (platform: Platform) => `${platformDisplayName(platform)}官方CLI版本核查失败。`,
  waitReady: (platform: Platform) => `请先等待${platformDisplayName(platform)}官方CLI准备完成。`,
  updateFailed: (platform: Platform) => `${platformDisplayName(platform)}官方CLI更新失败。`,
  cleanupFailed: (platform: Platform) => `清理${platformDisplayName(platform)}官方CLI失败`,
  placeholder: (platform: Platform) => `正在准备${platformDisplayName(platform)}官方CLI…`
};

export const AI_SETTINGS_MESSAGES = {
  clearModelConfirm: "再次点击“确认清除”将删除本地模型文件和运行时部署。",
  downloadOrRuntimeFailed: "本地DeepSeek模型下载或本地运行时配置失败，请按提示检查后重试。",
  downloadCancelled: "本地DeepSeek模型下载已取消，临时下载文件已清除。",
  configSaved: "AI配置已保存。",
  testingConnection: "正在测试AI连接，请稍候。",
  downloadStarted: "本地DeepSeek模型已开始后台下载，离开AI配置页也会继续。",
  aiDisabled: "AI当前未启用。",
  connectionPassed: "API连接测试通过，模型已返回响应。",
  testFailed: "测试连接失败。",
  configuringRuntime: "正在配置并启动本地DeepSeek运行环境。",
  localModelStatusFailed: "本地DeepSeek模型状态读取失败。",
  localDeepseekEnabled: "本地DeepSeek已启用，后续AI分析不再要求云端API Key。",
  downloadFailed: "本地DeepSeek模型下载失败。",
  cancelDownloadFailed: "取消本地DeepSeek模型下载失败。",
  modelCleared: "本地DeepSeek模型文件和运行时部署已清除。",
  clearModelFailed: "清除本地DeepSeek模型失败。",
  modelPathCopied: "本地DeepSeek模型存储路径已复制。",
  modelDownloaded: "本地DeepSeek模型已下载完成。",
  downloadedAndEnabled: "本地DeepSeek已下载并启用，后续AI分析不再要求云端API Key。",
  saveFailed: "AI配置保存失败。"
};

function platformDisplayName(platform: Platform): string {
  return PLATFORM_LABELS[platform] ?? "平台";
}
