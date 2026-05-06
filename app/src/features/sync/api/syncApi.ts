import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { isDemoMode } from "../../../api/demoMode";
import { isTauri, requireTauri } from "../../../api/tauri";
import type { SyncJobMode, SyncProgress, SyncResult } from "../model/types";

const SYNC_PROGRESS_EVENT = "sync-progress";

export async function runSyncJob(profileId: string, mode: SyncJobMode): Promise<SyncResult> {
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
  if (isTauri) return invoke("run_sync_job", { profileId, mode });
  return requireTauri("执行同步任务");
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
