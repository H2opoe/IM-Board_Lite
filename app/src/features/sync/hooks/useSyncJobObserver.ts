import { useCallback, useEffect, useRef } from "react";
import { getSyncJobState, watchSyncJobState } from "../api/syncApi";
import type { SyncJobSnapshot } from "../model/types";
import { logRecoverableError } from "../../../utils/logging";

/** Subscribe before querying; revisions make event/query delivery order irrelevant. */
export function useSyncJobObserver(enabled: boolean, onSnapshot: (snapshot: SyncJobSnapshot) => void) {
  const callback = useRef(onSnapshot);
  callback.current = onSnapshot;
  const latestRevision = useRef(-1);
  const backendBusy = useRef(false);
  const mounted = useRef(false);
  const receive = useCallback((snapshot: SyncJobSnapshot) => {
    if (!mounted.current || snapshot.revision < latestRevision.current) return;
    backendBusy.current = snapshot.busy;
    if (snapshot.revision === latestRevision.current) return;
    latestRevision.current = snapshot.revision;
    callback.current(snapshot);
  }, []);
  const reconcile = useCallback(async () => {
    if (!enabled) return null;
    const snapshot = await getSyncJobState();
    receive(snapshot);
    return snapshot;
  }, [enabled, receive]);
  useEffect(() => {
    if (!enabled) return;
    mounted.current = true;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const recover = () => { void reconcile().catch(error => logRecoverableError("读取后台同步状态失败，稍后重试", error)); };
    void (async () => {
      try {
        const stop = await watchSyncJobState(snapshot => { if (!disposed) receive(snapshot); });
        if (disposed) { stop(); return; }
        unlisten = stop;
      } catch (error) {
        if (!disposed) logRecoverableError("后台同步事件订阅失败，使用状态查询恢复", error);
      }
      if (!disposed) recover();
    })();
    const onVisible = () => { if (document.visibilityState === "visible") recover(); };
    window.addEventListener("focus", recover);
    document.addEventListener("visibilitychange", onVisible);
    // Also repairs a lost event without reading the dashboard repeatedly.
    const timer = window.setInterval(recover, 15_000);
    return () => {
      disposed = true; mounted.current = false; unlisten?.();
      window.clearInterval(timer);
      window.removeEventListener("focus", recover);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [enabled, receive, reconcile]);
  return { backendBusy, reconcile };
}
