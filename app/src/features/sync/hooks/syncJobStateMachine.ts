import type { SyncJobMode, SyncProgress } from "../model/types";

export type SyncUiState = "idle" | "syncing" | "analyzing" | "done" | "failed";

export function isCancellableSyncState(state: SyncUiState) {
  return state === "syncing" || state === "analyzing";
}

export function nextStateForProgress(progress: SyncProgress): SyncUiState | null {
  return progress.phase === "analysis" ? "analyzing" : null;
}

export function shouldRefreshDashboardForProgress(mode: SyncJobMode, progress: SyncProgress) {
  if (!progress.shouldRefreshDashboard) return false;
  return mode === "retry_analysis" ? progress.phase === "summary_done" : true;
}
