import type { PlatformDeployment } from "../../api/bridgeApi";
import { PLATFORM_LABELS } from "../../constants/platforms";
import type { ImProfile, WechatCandidate } from "../../types";
import { profileRemark } from "../../utils/profiles";

export function latestWechatDbDir(candidate?: WechatCandidate): string {
  const sorted = [...(candidate?.candidateDbDirs ?? [])].sort((left, right) =>
    String(right.lastModified || "").localeCompare(String(left.lastModified || ""))
  );
  return sorted[0]?.path ?? candidate?.dbDir ?? "";
}

export function findMatchingWechatInstance(
  candidates: WechatCandidate[],
  previous?: WechatCandidate | null
): WechatCandidate | null {
  if (!previous) return null;
  return (
    candidates.find((candidate) => candidate.appPath && candidate.appPath === previous.appPath) ??
    candidates.find((candidate) => candidate.bundleId && candidate.bundleId === previous.bundleId) ??
    null
  );
}

export function wechatDbFolderName(path: string): string {
  const normalized = path.replace(/[\\/]+$/, "");
  const segments = normalized.split(/[\\/]+/);
  return segments[segments.length - 2] || segments[segments.length - 1] || "微信账号数据";
}

export function formatWechatDbTime(value?: string): string {
  if (!value) return "未知";
  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  }).format(new Date(timestamp));
}

export function isOriginalWechatCli(candidate?: WechatCandidate): boolean {
  return candidate?.runtime === "windows_original_cli" || candidate?.setupMode === "windows_original_cli";
}

export function usesWindowsWechatRuntime(profile: ImProfile): boolean {
  return profile.platform === "wechat" && profile.configJson.runtime === "windows_original_cli";
}

export function isWindowsRuntime(): boolean {
  const platform = navigator.platform || "";
  const userAgent = navigator.userAgent || "";
  return platform.toLowerCase().startsWith("win") || /windows/i.test(userAgent);
}

export function joinNativePath(baseDir: string, leaf: string): string {
  const separator = /^[A-Za-z]:[\\/]/.test(baseDir) || baseDir.includes("\\") ? "\\" : "/";
  return `${baseDir.replace(/[\\/]+$/, "")}${separator}${leaf}`;
}

export function commandLineToolName(deployment?: PlatformDeployment | null): string {
  const command = deployment?.command.trimStart() ?? "";
  return command.startsWith("$env:") || command.startsWith("New-Item") ? "Windows PowerShell" : "macOS终端";
}

export function formatCliVersion(version: string): string {
  const normalized = version.trim();
  if (!normalized || normalized === "未知") return "未知";
  if (normalized === "内置版本" || normalized === "已配置版本") return normalized;
  return normalized.startsWith("v") ? normalized : `v${normalized}`;
}

export function getConfigString(profile: ImProfile, key: string): string {
  const value = profile.configJson[key];
  return typeof value === "string" ? value : "";
}

export function profilePlatformLabel(profile: ImProfile): string {
  return PLATFORM_LABELS[profile.platform] ?? profile.label;
}

export function profileAccountSubtitle(profile: ImProfile): string {
  return profileDisplayRemark(profile) || profilePlatformLabel(profile);
}

function profileDisplayRemark(profile: ImProfile): string {
  const remark = profileRemark(profile);
  if (remark) return remark;
  return profile.label !== profilePlatformLabel(profile) ? profile.label : "";
}
