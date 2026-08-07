import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { AiConfigInput, AiConfigView, LocalModelDownloadProgress, LocalModelStatus, SystemCapabilities } from "../features/ai/model/types";
import { isTauri, requireTauri } from "./tauri";

const LOCAL_MODEL_PROGRESS_EVENT = "local-model-download-progress";

export async function getAiConfig(): Promise<AiConfigView> {
  if (isTauri) return invoke("get_ai_config");
  return requireTauri("读取AI配置");
}

export async function getDefaultAiConfig(): Promise<AiConfigView> {
  if (isTauri) return invoke("get_default_ai_config");
  return requireTauri("读取AI默认配置");
}

export async function getSystemCapabilities(): Promise<SystemCapabilities> {
  if (isTauri) return invoke("get_system_capabilities");
  return requireTauri("读取系统能力");
}

export async function saveAiConfig(config: AiConfigInput): Promise<AiConfigView> {
  if (isTauri) return invoke("save_ai_config", { config });
  void config;
  return requireTauri("保存AI配置");
}

export async function testAiConnection(config: AiConfigInput): Promise<string> {
  if (isTauri) return invoke("test_ai_connection", { config });
  void config;
  return requireTauri("测试AI连接");
}

export async function getLocalDeepseekStatus(): Promise<LocalModelStatus> {
  if (isTauri) return invoke("get_local_deepseek_status");
  return requireTauri("读取本地模型状态");
}

export async function getLocalDeepseekDownloadProgress(): Promise<LocalModelDownloadProgress | null> {
  if (isTauri) return invoke("get_local_deepseek_download_progress");
  return requireTauri("读取本地模型下载进度");
}

export async function cancelLocalDeepseekDownload(): Promise<LocalModelStatus> {
  if (isTauri) return invoke("cancel_local_deepseek_download");
  return requireTauri("取消本地模型下载");
}

export async function installLocalDeepseekModel(): Promise<LocalModelStatus> {
  if (isTauri) return invoke("install_local_deepseek_model");
  return requireTauri("安装本地模型");
}

export async function clearLocalDeepseekModel(): Promise<LocalModelStatus> {
  if (isTauri) return invoke("clear_local_deepseek_model");
  return requireTauri("清除本地模型");
}

export async function watchLocalModelDownloadProgress(
  handler: (progress: LocalModelDownloadProgress) => void
): Promise<() => void> {
  if (isTauri) return listen<LocalModelDownloadProgress>(LOCAL_MODEL_PROGRESS_EVENT, (event) => handler(event.payload));
  void handler;
  return requireTauri("监听本地模型下载进度");
}
