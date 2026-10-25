import { useCallback, useEffect, useRef } from "react";
import { getDashboard } from "../../dashboard/api/dashboardApi";
import type { DashboardSetter } from "../../dashboard/hooks/useDashboardStore";
import { logRecoverableError } from "../../../utils/logging";

export function useDashboardRefreshCoordinator(activeProfileId: string, setDashboard: DashboardSetter) {
  const activeProfileIdRef = useRef(activeProfileId);
  const refreshSequence = useRef(0);
  const mounted = useRef(false);
  const pending = useRef(false);
  const running = useRef(false);
  useEffect(() => { activeProfileIdRef.current = activeProfileId; refreshSequence.current += 1; }, [activeProfileId]);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; pending.current = false; refreshSequence.current += 1; }; }, []);
  const refreshDashboardForArrivedBatch = useCallback(() => {
    if (!mounted.current) return;
    pending.current = true;
    refreshSequence.current += 1;
    if (running.current) return;
    running.current = true;
    void (async () => {
      try {
        while (pending.current && mounted.current) {
          pending.current = false;
          const sequence = refreshSequence.current;
          const profileId = activeProfileIdRef.current;
          try {
            const data = await getDashboard(profileId);
            if (mounted.current && sequence === refreshSequence.current && profileId === activeProfileIdRef.current) setDashboard(data, profileId);
          } catch (error) { logRecoverableError("刷新看板失败，已等待下一次同步进度刷新", error); }
        }
      } finally { running.current = false; }
    })();
  }, [setDashboard]);
  const markDashboardRefreshSettled = useCallback(() => { refreshSequence.current += 1; pending.current = false; }, []);
  return { activeProfileIdRef, markDashboardRefreshSettled, refreshDashboardForArrivedBatch };
}
