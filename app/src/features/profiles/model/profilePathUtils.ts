import { PLATFORM_LABELS } from "../../../constants/platforms";
import { profileRemark } from "../../../utils/profiles";
import type { ImProfile } from "./types";

export function joinNativePath(baseDir: string, leaf: string): string {
  const separator = /^[A-Za-z]:[\\/]/.test(baseDir) || baseDir.includes("\\") ? "\\" : "/";
  return `${baseDir.replace(/[\\/]+$/, "")}${separator}${leaf}`;
}

export function commandLineToolName(deployment?: { command: string } | null): string {
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
