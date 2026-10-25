import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri } from "../../../api/tauri";
import type { DashboardSetter } from "../../dashboard/hooks/useDashboardStore";
import type { ImProfile } from "../../profiles/model/types";
import {
  cancelSync,
  runSyncJob,
  waitForSyncIdle
} from "../api/syncApi";
import type { SyncJobMode, SyncJobSnapshot } from "../model/types";
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
import { useSyncJobObserver } from "./useSyncJobObserver";
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
  const requestSequence = useRef(0);
  const observedJob = useRef("");
  const {
    activeProfileIdRef,
    markDashboardRefreshSettled,
    refreshDashboardForArrivedBatch
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

  const onSnapshot = useCallback((snapshot: SyncJobSnapshot) => {
    if (!snapshot.jobId) return;
    const mode = snapshot.mode as SyncJobMode;
    const config = SYNC_JOB_UI_CONFIG[mode];
    if (!config) return;
    if (snapshot.running) {
      if (observedJob.current !== snapshot.jobId) {
        observedJob.current = snapshot.jobId;
        setSyncProgressNotices([]);
        setSyncMessagePages([]);
        setIsSyncCancelArmed(false);
        setSyncState(config.initialState);
        setSyncMessage(config.initialMessage);
      }
      if (snapshot.progress) {
        const progress = snapshot.progress;
        if (!pendingMaintenanceAction.current && !isSyncCancelArmed) setSyncMessage(progress.message);
        updateSyncProgressNotice(progress);
        const next = nextStateForProgress(progress);
        if (next) setSyncState(next);
        if (shouldRefreshDashboardForProgress(mode, progress)) refreshDashboardForArrivedBatch();
      }
      return;
    }
    observedJob.current = snapshot.jobId;
    setIsSyncCancelArmed(false);
    markDashboardRefreshSettled();
    refreshDashboardForArrivedBatch();
    if (snapshot.result) {
      const result = snapshot.result;
      setSyncState(result.syncStatus === "idle" ? "idle" : result.aiStatus === "failed" || result.syncStatus === "failed" ? "failed" : "done");
      const warnings = result.warnings.filter(shouldShowSyncWarning);
      setSyncProgressNotices(syncWarningNotices(warnings));
      setSyncMessagePages([]);
      setSyncMessage(warnings.length ? "" : syncCompletionMessage(mode, result));
    } else if (snapshot.error) {
      const message = syncErrorMessage(snapshot.error);
      setSyncProgressNotices([]);
      if (pendingMaintenanceAction.current && isSyncCancellationMessage(message)) {
        setSyncMessagePages([]);
        setSyncMessage(syncMaintenancePendingMessage(pendingMaintenanceAction.current));
      } else {
        setSyncState("failed"); setSyncMessagePages([message]); setSyncMessage(message);
      }
    }
  }, [isSyncCancelArmed, refreshDashboardForArrivedBatch, updateSyncProgressNotice]);
  const { backendBusy, reconcile } = useSyncJobObserver(isTauri && !demoMode, onSnapshot);

  const executeSyncJob = useCallback(
    (mode: SyncJobMode, targetProfileId = activeProfileId) => {
      if (syncInFlight.current) return syncRunPromise.current ?? Promise.resolve();
      const targets = targetProfileId === "aggregate"
        ? profiles.filter(profile => profile.enabled)
        : profiles.filter(profile => profile.id === targetProfileId && profile.enabled);
      if (!targets.length) {
        setSyncState("idle");
        setIsSyncCancelArmed(false);
        setSyncProgressNotices([]);
        setSyncMessagePages([]);
        setSyncMessage(profiles.length ? SYNC_RUNTIME_MESSAGES.noEnabledAccounts : SYNC_RUNTIME_MESSAGES.noAccounts);
        return Promise.resolve();
      }

      const id = ++requestSequence.current;
      const config = SYNC_JOB_UI_CONFIG[mode];
      const run = (async () => {
        syncInFlight.current = true;
        try {
          const state = await reconcile();
          if (state?.busy || backendBusy.current) { setSyncMessage("已有后台任务正在运行，请等待完成或先终止任务。"); return; }
          setSyncState(config.initialState);
          setIsSyncCancelArmed(false);
          setSyncMessagePages([]);
          setSyncProgressNotices([]);
          setSyncMessage(config.initialMessage);
          if (mode === "retry_analysis") {
            setDashboard(current => current ? { ...current, aiStatus: "analyzing" } : current, activeProfileIdRef.current);
          }
          await runSyncJob(targetProfileId, config.mode);
          await reconcile();
        } catch (error) {
          if (requestSequence.current !== id) return;
          // Query the final backend state even when the invocation rejected.
          const latest = await reconcile().catch(() => null);
          if (requestSequence.current !== id || latest?.running) return;
          const message = syncErrorMessage(error);
          if (pendingMaintenanceAction.current && isSyncCancellationMessage(message)) {
            setSyncMessagePages([]);
            setSyncMessage(syncMaintenancePendingMessage(pendingMaintenanceAction.current));
          } else {
            setSyncState("failed");setSyncProgressNotices([]);setSyncMessagePages([message]);setSyncMessage(message);
          }
        } finally {
          if (requestSequence.current === id) {
            syncInFlight.current = false;
            if (!pendingMaintenanceAction.current) setIsSyncCancelArmed(false);
          }
        }
      })();
      const trackedRun = run.finally(() => { if (syncRunPromise.current === trackedRun) syncRunPromise.current = null; });
      syncRunPromise.current = trackedRun;
      return trackedRun;
    }, [activeProfileId, profiles, reconcile, setDashboard]
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
      const requestSeq = ++maintenanceRequestSeq.current;
      let snapshot: SyncJobSnapshot | null;
      try { snapshot = await reconcile(); }
      catch (error) {
        if (maintenanceRequestSeq.current !== requestSeq) return;
        const message = syncErrorMessage(error);
        setSyncState("failed"); setSyncMessagePages([message]); setSyncMessage(message);
        return;
      }
      if (maintenanceRequestSeq.current !== requestSeq) return;
      if (!syncInFlight.current && !backendBusy.current && !snapshot?.busy) {
        await executeSyncJob(mode, targetProfileId);
        return;
      }

      pendingMaintenanceAction.current = action;
      setIsSyncCancelArmed(false);
      setSyncMessagePages([]);
      setSyncProgressNotices([]);
      setSyncMessage(syncMaintenanceStoppingMessage(action));

      try {
        // 维护动作允许在同步/分析中直接触发：先取消当前后端任务，再串行启动用户刚点击的新任务，避免两个桥接进程同时读写缓存。
        await cancelSync();
        await waitForSyncIdle();
        if (maintenanceRequestSeq.current !== requestSeq || pendingMaintenanceAction.current !== action) return;
        pendingMaintenanceAction.current = null;
        requestSequence.current += 1;
        syncInFlight.current = false;
        syncRunPromise.current = null;
        await executeSyncJob(mode, targetProfileId);
      } catch (error) {
        if (maintenanceRequestSeq.current !== requestSeq) return;
        pendingMaintenanceAction.current = null;
        setSyncState("failed");
        const message = syncErrorMessage(error);
        setSyncProgressNotices([]);
        setSyncMessagePages([message]);
        setSyncMessage(message);
      }
    },
    [activeProfileId, executeSyncJob, profiles, reconcile]
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
