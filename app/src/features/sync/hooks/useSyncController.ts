import { useCallback, useEffect, useRef, useState } from "react";
import { getDashboard } from "../../dashboard/api/dashboardApi";
import { isTauri } from "../../../api/tauri";
import type { DashboardSetter } from "../../dashboard/hooks/useDashboardStore";
import type { DashboardData } from "../../dashboard/model/types";
import type { ImProfile } from "../../profiles/model/types";
import {
  cancelSync,
  runSyncJob,
  watchSyncProgress
} from "../api/syncApi";
import type { SyncJobMode } from "../model/types";
import {
  SYNC_CANCEL_CONFIRM_MESSAGES,
  SYNC_JOB_UI_CONFIG,
  SYNC_RUNTIME_MESSAGES,
  type SyncMaintenanceAction,
  shouldShowSyncWarning,
  isSyncMessageError,
  syncCompletionMessage,
  syncErrorMessage,
  syncMaintenancePendingMessage,
  syncMaintenanceStoppingMessage
} from "../messages/syncMessages";
import { useSyncFrequency } from "./useSyncFrequency";
import { useDashboardRefreshCoordinator } from "./useDashboardRefreshCoordinator";
import { useSyncProgressNotices, type SyncProgressNotice } from "./useSyncProgressNotices";
import {
  isCancellableSyncState,
  nextStateForProgress,
  shouldRefreshDashboardForProgress,
  type SyncUiState
} from "./syncJobStateMachine";

export type { SyncUiState, SyncProgressNotice };

interface UseSyncControllerOptions {
  activeProfileId: string;
  demoMode: boolean;
  profiles: ImProfile[];
  setDashboard: DashboardSetter;
}

const SYNC_CANCEL_CONFIRM_TIMEOUT_MS = 3_000;

function isSyncCancellationMessage(message: string) {
  return message.includes("同步已终止");
}


function syncWarningNotices(warnings: string[]): SyncProgressNotice[] {
  return warnings.map((warning, index) => ({
    id: `sync-warning-${index}`,
    message: warning,
    variant: isSyncMessageError(warning) ? "error" : "info"
  }));
}

export function useSyncController({ activeProfileId, demoMode, profiles, setDashboard }: UseSyncControllerOptions) {
  const [syncState, setSyncState] = useState<SyncUiState>("idle");
  const [syncMessage, setSyncMessage] = useState("");
  const [syncMessagePages, setSyncMessagePages] = useState<string[]>([]);
  const {
    closeSyncProgressNotice,
    setSyncProgressNotices,
    syncProgressNotices,
    updateSyncProgressNotice
  } = useSyncProgressNotices();
  const [isSyncCancelArmed, setIsSyncCancelArmed] = useState(false);
  const { syncFrequencyMinutes, setSyncFrequencyMinutes } = useSyncFrequency(demoMode);
  const syncInFlight = useRef(false);
  const syncRunPromise = useRef<Promise<void> | null>(null);
  const pendingMaintenanceAction = useRef<SyncMaintenanceAction | null>(null);
  const maintenanceRequestSeq = useRef(0);
  const initialSyncStarted = useRef(false);
  const {
    activeProfileIdRef,
    markDashboardRefreshSettled,
    refreshDashboardForArrivedBatch,
    setDashboardForVisibleProfile
  } = useDashboardRefreshCoordinator(activeProfileId, setDashboard);

  const dismissSyncMessage = useCallback(() => {
    setSyncMessage("");
    setSyncMessagePages([]);
  }, []);


  function isDisabledSingleProfile(profileId: string) {
    if (profileId === "aggregate") return false;
    return profiles.some((profile) => profile.id === profileId && !profile.enabled);
  }

  function showDisabledProfileSyncMessage() {
    setSyncState("idle");
    setIsSyncCancelArmed(false);
    setSyncProgressNotices([]);
    setSyncMessagePages([]);
    setSyncMessage(SYNC_RUNTIME_MESSAGES.accountSyncDisabled);
  }

  const executeSyncJob = useCallback(
    (mode: SyncJobMode, targetProfileId = activeProfileId) => {
      if (syncInFlight.current) return syncRunPromise.current ?? Promise.resolve();
      const config = SYNC_JOB_UI_CONFIG[mode];

      const run = (async () => {
        syncInFlight.current = true;
        setSyncState(config.initialState);
        if (mode === "retry_analysis") {
          const visibleProfileId = activeProfileIdRef.current;
          setDashboard(
            (currentDashboard) =>
              currentDashboard
                ? {
                    ...currentDashboard,
                    aiStatus: "analyzing"
                  }
                : currentDashboard,
            visibleProfileId
          );
        }
        setIsSyncCancelArmed(false);
        setSyncMessagePages([]);
        setSyncProgressNotices([]);
        setSyncMessage(config.initialMessage);
        let unlisten = () => {};

        try {
          try {
            unlisten = await watchSyncProgress((progress) => {
              if (targetProfileId !== "aggregate" && progress.profileId !== targetProfileId) return;
              setSyncMessagePages([]);
              setSyncMessage(progress.message);
              updateSyncProgressNotice(progress);
              const nextState = nextStateForProgress(progress);
              if (nextState) setSyncState(nextState);
              if (shouldRefreshDashboardForProgress(mode, progress)) {
                refreshDashboardForArrivedBatch();
              }
            });
          } catch {
            setSyncMessage(config.fallbackMessage);
          }

          const result = await runSyncJob(targetProfileId, config.mode);
          markDashboardRefreshSettled();
          const analyzingProfileId = targetProfileId === "aggregate" ? activeProfileIdRef.current : targetProfileId;
          const nextDashboard = await getDashboard(analyzingProfileId);
          if (mode !== "retry_analysis" && result.analyzedMessages > 0) {
            setDashboardForVisibleProfile(
              {
                ...nextDashboard,
                syncStatus: "analyzing",
                aiStatus: "analyzing"
              },
              analyzingProfileId
            );
            setSyncState("analyzing");
            setSyncMessagePages([]);
            setSyncMessage(SYNC_RUNTIME_MESSAGES.refreshingDashboard(result.insertedMessages, result.analyzedMessages));
            await new Promise((resolve) => window.setTimeout(resolve, 850));
          }

          markDashboardRefreshSettled();
          const finalProfileId = targetProfileId === "aggregate" ? activeProfileIdRef.current : targetProfileId;
          const finalDashboard = await getDashboard(finalProfileId);
          setDashboardForVisibleProfile(
            {
              ...finalDashboard,
              syncStatus: result.syncStatus,
              aiStatus: result.aiStatus
            },
            finalProfileId
          );
          setSyncState(result.aiStatus === "failed" ? "failed" : "done");
          const visibleWarnings = result.warnings.filter(shouldShowSyncWarning);
          if (visibleWarnings.length > 0) {
            setSyncProgressNotices(syncWarningNotices(visibleWarnings));
            setSyncMessagePages([]);
            setSyncMessage("");
          } else {
            setSyncMessagePages([]);
            setSyncProgressNotices([]);
            setSyncMessage(syncCompletionMessage(mode, result));
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
    [activeProfileId, refreshDashboardForArrivedBatch, setDashboard, setDashboardForVisibleProfile]
  );

  const handleSyncButtonClick = useCallback(() => {
    if (!isCancellableSyncState(syncState)) {
      if (isDisabledSingleProfile(activeProfileId)) {
        showDisabledProfileSyncMessage();
        return;
      }
      void executeSyncJob("incremental", activeProfileId);
      return;
    }
    if (!isSyncCancelArmed) {
      setIsSyncCancelArmed(true);
      setSyncMessagePages([]);
      setSyncMessage(SYNC_CANCEL_CONFIRM_MESSAGES[syncState] ?? SYNC_RUNTIME_MESSAGES.cancelCurrentTask);
      return;
    }
    setSyncMessage(syncState === "analyzing" ? SYNC_RUNTIME_MESSAGES.cancellingAnalysis : SYNC_RUNTIME_MESSAGES.cancellingSync);
    setSyncMessagePages([]);
    void cancelSync().catch((error) => {
      setSyncState("failed");
      const message = syncErrorMessage(error);
      setSyncMessagePages([message]);
      setSyncMessage(message);
    });
  }, [activeProfileId, executeSyncJob, isSyncCancelArmed, profiles, syncState]);

  const runMaintenanceAction = useCallback(
    async (action: SyncMaintenanceAction, targetProfileId = activeProfileId) => {
      const mode: SyncJobMode = action === "retry-analysis" ? "retry_analysis" : "full_resync";
      if (isDisabledSingleProfile(targetProfileId)) {
        showDisabledProfileSyncMessage();
        return;
      }
      if (!syncInFlight.current) {
        await executeSyncJob(mode, targetProfileId);
        return;
      }

      const requestSeq = maintenanceRequestSeq.current + 1;
      maintenanceRequestSeq.current = requestSeq;
      pendingMaintenanceAction.current = action;
      setIsSyncCancelArmed(false);
      setSyncMessagePages([]);
      setSyncProgressNotices([]);
      setSyncMessage(syncMaintenanceStoppingMessage(action));

      try {
        // 维护动作允许在同步/分析中直接触发：先取消当前后端任务，再串行启动用户刚点击的新任务，避免两个桥接进程同时读写缓存。
        const currentRun = syncRunPromise.current;
        await cancelSync();
        await currentRun;
        if (maintenanceRequestSeq.current !== requestSeq || pendingMaintenanceAction.current !== action) return;
        pendingMaintenanceAction.current = null;
        await executeSyncJob(mode, targetProfileId);
      } catch (error) {
        pendingMaintenanceAction.current = null;
        setSyncState("failed");
        const message = syncErrorMessage(error);
        setSyncProgressNotices([]);
        setSyncMessagePages([message]);
        setSyncMessage(message);
      }
    },
    [activeProfileId, executeSyncJob, profiles]
  );

  useEffect(() => {
    if (!isSyncCancelArmed || !isCancellableSyncState(syncState)) return;
    const armedMessage = SYNC_CANCEL_CONFIRM_MESSAGES[syncState];
    const timer = window.setTimeout(() => {
      setIsSyncCancelArmed(false);
      setSyncMessage((message) =>
        message === armedMessage
          ? syncState === "analyzing"
            ? SYNC_RUNTIME_MESSAGES.analysisStillRunning
            : SYNC_RUNTIME_MESSAGES.syncStillRunning
          : message
      );
    }, SYNC_CANCEL_CONFIRM_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [isSyncCancelArmed, syncState]);

  useEffect(() => {
    if (demoMode) return;
    if (initialSyncStarted.current) return;
    initialSyncStarted.current = true;
    void executeSyncJob("incremental", "aggregate");
  }, [demoMode, executeSyncJob]);

  useEffect(() => {
    if (demoMode) return undefined;
    if (isTauri) return undefined;
    const interval = window.setInterval(() => {
      void executeSyncJob("incremental", "aggregate");
    }, syncFrequencyMinutes * 60 * 1000);
    return () => window.clearInterval(interval);
  }, [demoMode, executeSyncJob, syncFrequencyMinutes]);

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
