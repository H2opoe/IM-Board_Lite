import { useCallback, useEffect, useRef, useState } from "react";
import { getDashboard } from "../api/dashboardApi";
import { cancelSync, retryAiAnalysis, runFullResync, runManualSync, watchSyncProgress, type SyncProgress } from "../api/syncApi";
import type { DashboardData } from "../types";
import {
  SYNC_CANCEL_CONFIRM_MESSAGES,
  type SyncMaintenanceAction,
  shouldShowSyncWarning,
  syncErrorMessage,
  syncMaintenancePendingMessage
} from "../utils/syncMessages";

export type SyncUiState = "idle" | "syncing" | "analyzing" | "done" | "failed";
export type SyncProgressNoticeVariant = "info" | "success" | "error";

export interface SyncProgressNotice {
  id: string;
  message: string;
  variant: SyncProgressNoticeVariant;
}

interface UseSyncControllerOptions {
  activeProfileId: string;
  demoMode: boolean;
  setDashboard: (dashboard: DashboardData) => void;
}

const DEFAULT_SYNC_FREQUENCY_MINUTES = 15;
const SYNC_FREQUENCY_STORAGE_KEY = "im-board-sync-frequency-minutes";
const SYNC_CANCEL_CONFIRM_TIMEOUT_MS = 3_000;
const DASHBOARD_REFRESH_PROGRESS_PHASES = new Set(["clear_cache_done", "history_done", "fetch_messages_done", "analysis_done", "summary_done"]);

function loadSyncFrequency() {
  const stored = window.localStorage.getItem(SYNC_FREQUENCY_STORAGE_KEY);
  const parsed = stored ? Number.parseInt(stored, 10) : DEFAULT_SYNC_FREQUENCY_MINUTES;
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_SYNC_FREQUENCY_MINUTES;
}

function isCancellableSyncState(state: SyncUiState) {
  return state === "syncing" || state === "analyzing";
}

function isSyncCancellationMessage(message: string) {
  return message.includes("同步已终止");
}

function syncProgressNoticeVariant(phase: string): SyncProgressNoticeVariant {
  if (phase.includes("failed")) return "error";
  if (phase.endsWith("_done") || phase === "profile_sync_done") return "success";
  return "info";
}

export function useSyncController({ activeProfileId, demoMode, setDashboard }: UseSyncControllerOptions) {
  const [syncState, setSyncState] = useState<SyncUiState>("idle");
  const [syncMessage, setSyncMessage] = useState("");
  const [syncMessagePages, setSyncMessagePages] = useState<string[]>([]);
  const [syncProgressNotices, setSyncProgressNotices] = useState<SyncProgressNotice[]>([]);
  const [isSyncCancelArmed, setIsSyncCancelArmed] = useState(false);
  const [syncFrequencyMinutes, setSyncFrequencyMinutes] = useState(loadSyncFrequency);
  const syncInFlight = useRef(false);
  const syncRunPromise = useRef<Promise<void> | null>(null);
  const pendingMaintenanceAction = useRef<SyncMaintenanceAction | null>(null);
  const maintenanceRequestSeq = useRef(0);
  const initialSyncStarted = useRef(false);

  useEffect(() => {
    window.localStorage.setItem(SYNC_FREQUENCY_STORAGE_KEY, String(syncFrequencyMinutes));
  }, [syncFrequencyMinutes]);

  function updateSyncProgressNotice(progress: SyncProgress) {
    const noticeId = progress.profileId || progress.phase;
    const notice: SyncProgressNotice = {
      id: noticeId,
      message: progress.message,
      variant: syncProgressNoticeVariant(progress.phase)
    };
    setSyncProgressNotices((currentNotices) => {
      const existingIndex = currentNotices.findIndex((currentNotice) => currentNotice.id === noticeId);
      if (existingIndex < 0) return [...currentNotices, notice];
      return currentNotices.map((currentNotice, index) => (index === existingIndex ? notice : currentNotice));
    });
  }

  const closeSyncProgressNotice = useCallback((noticeId: string) => {
    setSyncProgressNotices((currentNotices) => currentNotices.filter((notice) => notice.id !== noticeId));
  }, []);

  const dismissSyncMessage = useCallback(() => {
    setSyncMessage("");
    setSyncMessagePages([]);
  }, []);

  const startSync = useCallback(
    (targetProfileId = activeProfileId, resetCache = false) => {
      if (syncInFlight.current) return syncRunPromise.current ?? Promise.resolve();

      const run = (async () => {
        syncInFlight.current = true;
        setSyncState("syncing");
        setIsSyncCancelArmed(false);
        setSyncMessagePages([]);
        setSyncProgressNotices([]);
        setSyncMessage(resetCache ? "正在清空缓存并准备重新同步..." : "正在准备读取今天消息...");
        let unlisten = () => {};

        try {
          try {
            unlisten = await watchSyncProgress((progress) => {
              if (targetProfileId !== "aggregate" && progress.profileId !== targetProfileId) return;
              setSyncMessagePages([]);
              setSyncMessage(progress.message);
              updateSyncProgressNotice(progress);
              if (progress.phase === "analysis") {
                setSyncState("analyzing");
              }
              if (DASHBOARD_REFRESH_PROGRESS_PHASES.has(progress.phase)) {
                void getDashboard(activeProfileId).then(setDashboard);
              }
            });
          } catch {
            setSyncMessage("正在读取今天消息...");
          }

          const result = resetCache ? await runFullResync(targetProfileId) : await runManualSync(targetProfileId);
          const next = await getDashboard(activeProfileId);
          if (result.analyzedMessages > 0) {
            setDashboard({
              ...next,
              syncStatus: "analyzing",
              aiStatus: "analyzing"
            });
            setSyncState("analyzing");
            setSyncMessagePages([]);
            setSyncMessage(`已读取${result.insertedMessages}条新消息，AI已识别${result.analyzedMessages}个事项，正在刷新看板...`);
            await new Promise((resolve) => window.setTimeout(resolve, 850));
          }

          const finalDashboard = await getDashboard(activeProfileId);
          setDashboard({
            ...finalDashboard,
            syncStatus: result.syncStatus,
            aiStatus: result.aiStatus
          });
          setSyncState(result.aiStatus === "failed" ? "failed" : "done");
          const visibleWarnings = result.warnings.filter(shouldShowSyncWarning);
          if (visibleWarnings.length > 0) {
            setSyncProgressNotices([]);
            setSyncMessagePages(visibleWarnings);
            setSyncMessage(visibleWarnings[0]);
          } else if (result.aiStatus === "not_configured") {
            setSyncMessagePages([]);
            setSyncProgressNotices([]);
            setSyncMessage(`${resetCache ? "重新同步完成，" : ""}已读取${result.insertedMessages}条新消息，AI未启用。`);
          } else {
            setSyncMessagePages([]);
            setSyncProgressNotices([]);
            setSyncMessage(`${resetCache ? "重新同步完成，" : ""}已读取${result.insertedMessages}条新消息，AI分析完成，识别${result.analyzedMessages}个事项。`);
          }
        } catch (error) {
          const message = syncErrorMessage(error);
          if (pendingMaintenanceAction.current && isSyncCancellationMessage(message)) {
            setSyncMessagePages([]);
            setSyncMessage(syncMaintenancePendingMessage(pendingMaintenanceAction.current));
          } else {
            setSyncState("failed");
            setSyncProgressNotices([]);
            setSyncMessagePages([message]);
            setSyncMessage(message);
          }
        } finally {
          unlisten();
          syncInFlight.current = false;
          if (!pendingMaintenanceAction.current) {
            setIsSyncCancelArmed(false);
          }
        }
      })();

      const trackedRun = run.finally(() => {
        if (syncRunPromise.current === trackedRun) {
          syncRunPromise.current = null;
        }
      });
      syncRunPromise.current = trackedRun;
      return syncRunPromise.current;
    },
    [activeProfileId, setDashboard]
  );

  const handleSyncButtonClick = useCallback(() => {
    if (!isCancellableSyncState(syncState)) {
      void startSync(activeProfileId);
      return;
    }
    if (!isSyncCancelArmed) {
      setIsSyncCancelArmed(true);
      setSyncMessagePages([]);
      setSyncMessage(SYNC_CANCEL_CONFIRM_MESSAGES[syncState] ?? "再次点击将停止当前任务。");
      return;
    }
    setSyncMessage(syncState === "analyzing" ? "正在终止当前AI分析..." : "正在终止当前同步进程...");
    setSyncMessagePages([]);
    void cancelSync().catch((error) => {
      setSyncState("failed");
      const message = syncErrorMessage(error);
      setSyncMessagePages([message]);
      setSyncMessage(message);
    });
  }, [activeProfileId, isSyncCancelArmed, startSync, syncState]);

  const retryAnalysis = useCallback(
    (targetProfileId = activeProfileId) => {
      if (syncInFlight.current) return syncRunPromise.current ?? Promise.resolve();

      const run = (async () => {
        syncInFlight.current = true;
        setSyncState("analyzing");
        setIsSyncCancelArmed(false);
        setSyncMessagePages([]);
        setSyncProgressNotices([]);
        setSyncMessage("正在清空AI结果并重新生成...");
        let unlisten = () => {};

        try {
          try {
            unlisten = await watchSyncProgress((progress) => {
              if (targetProfileId !== "aggregate" && progress.profileId !== targetProfileId) return;
              setSyncMessagePages([]);
              setSyncMessage(progress.message);
              updateSyncProgressNotice(progress);
              if (DASHBOARD_REFRESH_PROGRESS_PHASES.has(progress.phase)) {
                void getDashboard(activeProfileId).then(setDashboard);
              }
            });
          } catch {
            setSyncMessage("正在清空AI结果并重新生成...");
          }

          const result = await retryAiAnalysis(targetProfileId);
          const finalDashboard = await getDashboard(activeProfileId);
          setDashboard({
            ...finalDashboard,
            syncStatus: result.syncStatus,
            aiStatus: result.aiStatus
          });
          setSyncState(result.aiStatus === "failed" ? "failed" : "done");
          const visibleWarnings = result.warnings.filter(shouldShowSyncWarning);
          if (visibleWarnings.length > 0) {
            setSyncProgressNotices([]);
            setSyncMessagePages(visibleWarnings);
            setSyncMessage(visibleWarnings[0]);
          } else if (result.aiStatus === "not_configured") {
            setSyncMessagePages([]);
            setSyncProgressNotices([]);
            setSyncMessage("AI未启用。");
          } else {
            setSyncMessagePages([]);
            setSyncProgressNotices([]);
            setSyncMessage(`重新生成完成，AI已识别${result.analyzedMessages}个事项。`);
          }
        } catch (error) {
          const message = syncErrorMessage(error);
          if (pendingMaintenanceAction.current && isSyncCancellationMessage(message)) {
            setSyncMessagePages([]);
            setSyncMessage(syncMaintenancePendingMessage(pendingMaintenanceAction.current));
          } else {
            setSyncState("failed");
            setSyncProgressNotices([]);
            setSyncMessagePages([message]);
            setSyncMessage(message);
          }
        } finally {
          unlisten();
          syncInFlight.current = false;
          if (!pendingMaintenanceAction.current) {
            setIsSyncCancelArmed(false);
          }
        }
      })();

      const trackedRun = run.finally(() => {
        if (syncRunPromise.current === trackedRun) {
          syncRunPromise.current = null;
        }
      });
      syncRunPromise.current = trackedRun;
      return syncRunPromise.current;
    },
    [activeProfileId, setDashboard]
  );

  const runMaintenanceAction = useCallback(
    async (action: SyncMaintenanceAction, targetProfileId = activeProfileId) => {
      if (!syncInFlight.current) {
        if (action === "retry-analysis") {
          await retryAnalysis(targetProfileId);
        } else {
          await startSync(targetProfileId, true);
        }
        return;
      }

      const requestSeq = maintenanceRequestSeq.current + 1;
      maintenanceRequestSeq.current = requestSeq;
      pendingMaintenanceAction.current = action;
      setIsSyncCancelArmed(false);
      setSyncMessagePages([]);
      setSyncProgressNotices([]);
      setSyncMessage(action === "retry-analysis" ? "正在终止当前任务，随后重新生成AI分析..." : "正在终止当前任务，随后重新同步...");

      try {
        // 维护动作允许在同步/分析中直接触发：先取消当前后端任务，再串行启动用户刚点击的新任务，避免两个桥接进程同时读写缓存。
        const currentRun = syncRunPromise.current;
        await cancelSync();
        await currentRun;
        if (maintenanceRequestSeq.current !== requestSeq || pendingMaintenanceAction.current !== action) return;
        pendingMaintenanceAction.current = null;
        if (action === "retry-analysis") {
          await retryAnalysis(targetProfileId);
        } else {
          await startSync(targetProfileId, true);
        }
      } catch (error) {
        pendingMaintenanceAction.current = null;
        setSyncState("failed");
        const message = syncErrorMessage(error);
        setSyncProgressNotices([]);
        setSyncMessagePages([message]);
        setSyncMessage(message);
      }
    },
    [activeProfileId, retryAnalysis, startSync]
  );

  useEffect(() => {
    if (!isSyncCancelArmed || !isCancellableSyncState(syncState)) return;
    const armedMessage = SYNC_CANCEL_CONFIRM_MESSAGES[syncState];
    const timer = window.setTimeout(() => {
      setIsSyncCancelArmed(false);
      setSyncMessage((message) => (message === armedMessage ? (syncState === "analyzing" ? "AI分析仍在进行..." : "同步仍在进行...") : message));
    }, SYNC_CANCEL_CONFIRM_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [isSyncCancelArmed, syncState]);

  useEffect(() => {
    if (demoMode) return;
    if (initialSyncStarted.current) return;
    initialSyncStarted.current = true;
    void startSync("aggregate");
  }, [demoMode, startSync]);

  useEffect(() => {
    if (demoMode) return undefined;
    const interval = window.setInterval(() => {
      void startSync("aggregate");
    }, syncFrequencyMinutes * 60 * 1000);
    return () => window.clearInterval(interval);
  }, [demoMode, startSync, syncFrequencyMinutes]);

  return {
    syncState,
    syncMessage,
    syncMessagePages,
    syncProgressNotices,
    syncFrequencyMinutes,
    isSyncCancelArmed,
    handleSyncButtonClick,
    runMaintenanceAction,
    setSyncFrequencyMinutes,
    dismissSyncMessage,
    closeSyncProgressNotice
  };
}
