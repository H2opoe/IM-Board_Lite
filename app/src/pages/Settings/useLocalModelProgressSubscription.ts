import { useEffect } from "react";
import { watchLocalModelDownloadProgress } from "../../api/aiApi";
import type { LocalModelDownloadProgress } from "../../features/ai/model/types";
import { logRecoverableError } from "../../utils/logging";

export function useLocalModelProgressSubscription(
  onProgress: (progress: LocalModelDownloadProgress) => void
) {
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void watchLocalModelDownloadProgress((progress) => {
      if (active) onProgress(progress);
    })
      .then((cleanup) => {
        if (!active) {
          cleanup();
          return;
        }
        unlisten = cleanup;
      })
      .catch((error) => {
        logRecoverableError("监听本地模型下载进度失败", error);
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [onProgress]);
}
