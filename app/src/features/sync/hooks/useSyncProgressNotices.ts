import { useCallback, useState } from "react";
import { upsertNoticeToStackTop } from "../../../components/shared/noticeStack";
import type { SyncProgress } from "../model/types";

export type SyncProgressNoticeVariant = "info" | "success" | "error";

export interface SyncProgressNotice {
  id: string;
  message: string;
  variant: SyncProgressNoticeVariant;
}

function noticeVariant(phase: string): SyncProgressNoticeVariant {
  if (phase.includes("failed")) return "error";
  if (phase.endsWith("_done") || phase === "profile_sync_done") return "success";
  return "info";
}

export function useSyncProgressNotices() {
  const [syncProgressNotices, setSyncProgressNotices] = useState<SyncProgressNotice[]>([]);

  const updateSyncProgressNotice = useCallback((progress: SyncProgress) => {
    const notice: SyncProgressNotice = {
      id: progress.profileId || progress.phase,
      message: progress.message,
      variant: noticeVariant(progress.phase)
    };
    setSyncProgressNotices((current) => upsertNoticeToStackTop(current, notice));
  }, []);

  const closeSyncProgressNotice = useCallback((noticeId: string) => {
    setSyncProgressNotices((current) => current.filter((notice) => notice.id !== noticeId));
  }, []);

  return {
    closeSyncProgressNotice,
    setSyncProgressNotices,
    syncProgressNotices,
    updateSyncProgressNotice
  };
}
