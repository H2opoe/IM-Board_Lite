import type { LocalModelDownloadProgress } from "../../features/ai/model/types";

export type LocalModelOperation =
  | "idle"
  | "installingLocalModel"
  | "cancellingLocalModelDownload"
  | "configuringLocalModel"
  | "clearingLocalModel"
  | "saving"
  | "saved"
  | "testing"
  | "tested"
  | "error";

export function isLocalModelDownloadActive(progress: LocalModelDownloadProgress) {
  return progress.status === "starting" || progress.status === "downloading";
}

export function localModelUiFlags(
  operation: LocalModelOperation,
  progress: LocalModelDownloadProgress | null,
  isSavingConfig: boolean
) {
  const isDownloading = operation === "installingLocalModel" || Boolean(progress && isLocalModelDownloadActive(progress));
  const isCancelling = operation === "cancellingLocalModelDownload";
  const isConfiguring = operation === "configuringLocalModel";
  const isClearing = operation === "clearingLocalModel";
  return {
    isDownloading,
    isCancelling,
    isConfiguring,
    isClearing,
    isPrimaryRunning: isDownloading || isCancelling || isConfiguring || isSavingConfig,
    isActionRunning: isDownloading || isCancelling || isConfiguring || isSavingConfig || isClearing
  };
}
