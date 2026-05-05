import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, DiagnosticExport } from "../features/app-settings/model/types";
import { isTauri, requireTauri } from "./tauri";

export async function setThemeDockIcon(theme: "light" | "dark"): Promise<void> {
  if (isTauri) return invoke("set_theme_dock_icon", { theme });
}

export async function getAppSettings(): Promise<AppSettings> {
  if (isTauri) return invoke("get_app_settings");
  return requireTauri("读取应用设置");
}

export async function saveAppSettings(settings: AppSettings): Promise<AppSettings> {
  if (isTauri) return invoke("save_app_settings", { settings });
  void settings;
  return requireTauri("保存应用设置");
}

export async function exportDiagnosticPackage(): Promise<DiagnosticExport> {
  if (isTauri) return invoke("export_diagnostic_package");
  return requireTauri("导出诊断包");
}

export async function openMacosPrivacySettings(
  pane: "app_management" | "full_disk_access",
  appPath = "",
  dataDir = ""
): Promise<void> {
  if (isTauri) return invoke("open_macos_privacy_settings", { pane, appPath, dataDir });
  return requireTauri("打开系统设置");
}
