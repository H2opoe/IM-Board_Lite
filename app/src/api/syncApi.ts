import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type { SyncResult } from "../types";
import { isDemoMode } from "./demoMode";
import { isTauri, requireTauri } from "./tauri";

const SYNC_PROGRESS_EVENT = "sync-progress";

export interface SyncProgress {
  profileId: string;
  profileLabel: string;
  phase: string;
  message: string;
  current: number;
  total: number;
}

export async function runManualSync(profileId: string): Promise<SyncResult> {
  if (isDemoMode()) {
    await new Promise((resolve) => window.setTimeout(resolve, 420));
    const now = new Date().toISOString();
    return {
      profileId,
      syncStatus: "synced",
      aiStatus: "ready",
      insertedMessages: 24,
      analyzedMessages: 6,
      warnings: [],
      startedAt: now,
      finishedAt: now
    };
  }
  if (isTauri) return invoke("run_manual_sync", { profileId });
  return requireTauri("同步消息");
}

export async function runFullResync(profileId: string): Promise<SyncResult> {
  if (isDemoMode()) return runManualSync(profileId);
  if (isTauri) return invoke("run_full_resync", { profileId });
  return requireTauri("重新同步消息");
}

export async function retryAiAnalysis(profileId: string): Promise<SyncResult> {
  if (isDemoMode()) return runManualSync(profileId);
  if (isTauri) return invoke("retry_ai_analysis", { profileId });
  return requireTauri("重新生成AI分析");
}

export async function cancelSync(): Promise<boolean> {
  if (isTauri) return invoke("cancel_sync");
  return requireTauri("终止同步");
}

export async function watchSyncProgress(handler: (progress: SyncProgress) => void): Promise<() => void> {
  if (isTauri) return listen<SyncProgress>(SYNC_PROGRESS_EVENT, (event) => handler(event.payload));
  void handler;
  return requireTauri("监听同步进度");
}
