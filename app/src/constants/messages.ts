import type { Platform } from "../features/profiles/model/types";
import { PLATFORM_LABELS } from "./platforms";

export const EMPTY_STATE_MESSAGES = {
  noProfiles: "暂无账号",
  noStats: "暂无统计",
  noMessageTypes: "暂无统计",
  noKeywords: "今天暂未发现",
  noActionItems: "今天暂未发现",
  aiActionPending: "AI分析中，完成后写入列表",
  aiTopicPending: "AI正在汇总今天话题"
};

export const APP_MESSAGES = {
  cancel: "取消",
  saveConfig: "保存配置",
  saveSettings: "保存设置",
  settingsSaved: "设置已保存。",
  deleteInProgress: "删除中",
  confirmDelete: "确认删除"
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
  checkingVersion: "正在检查官方 CLI 版本",
  readyStatusTitle: "已加载 CLI",
  preparingStatusTitle: "CLI 准备中",
  pendingReady: "等待 CLI 就绪",
  prepareFailed: (platform: Platform) => `${platformDisplayName(platform)}官方 CLI 准备失败。`,
  versionCheckFailed: (platform: Platform) => `${platformDisplayName(platform)}官方 CLI 版本核查失败。`,
  waitReady: (platform: Platform) => `请先等待${platformDisplayName(platform)}官方 CLI 准备完成。`,
  updateFailed: (platform: Platform) => `${platformDisplayName(platform)}官方 CLI 更新失败。`,
  cleanupFailed: (platform: Platform) => `清理${platformDisplayName(platform)}官方 CLI 失败`,
  placeholder: (platform: Platform) => `正在准备${platformDisplayName(platform)}官方 CLI…`
};

function platformDisplayName(platform: Platform): string {
  return PLATFORM_LABELS[platform] ?? "平台";
}
