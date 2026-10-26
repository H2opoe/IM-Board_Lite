import { invoke } from "@tauri-apps/api/core";
import { setTheme } from "@tauri-apps/api/app";
import type { AppSettings, DiagnosticExport } from "../features/app-settings/model/types";
import { isTauri, requireTauri } from "./tauri";

type ThemeMode = "light" | "dark";

export async function setNativeTheme(theme: ThemeMode | null): Promise<void> {
  if (isTauri) return setTheme(theme);
}

export async function setThemeDockIcon(theme: ThemeMode): Promise<void> {
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

export async function exportDiagnosticPackage(filePath: string): Promise<DiagnosticExport> {
  if (isTauri) return invoke("export_diagnostic_package", { filePath });
  void filePath;
  return requireTauri("导出诊断包");
}
