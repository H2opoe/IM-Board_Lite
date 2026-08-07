import { useCallback, useEffect, useRef } from "react";
import { getDashboard } from "../../dashboard/api/dashboardApi";
import type { DashboardSetter } from "../../dashboard/hooks/useDashboardStore";
import type { DashboardData } from "../../dashboard/model/types";
import { logRecoverableError } from "../../../utils/logging";

export function useDashboardRefreshCoordinator(
  activeProfileId: string,
  setDashboard: DashboardSetter
) {
  const activeProfileIdRef = useRef(activeProfileId);
  const refreshSequence = useRef(0);

  useEffect(() => {
    activeProfileIdRef.current = activeProfileId;
  }, [activeProfileId]);

  const refreshDashboardForArrivedBatch = useCallback(() => {
    const requestSequence = refreshSequence.current + 1;
    const visibleProfileId = activeProfileIdRef.current;
    refreshSequence.current = requestSequence;
    void getDashboard(visibleProfileId)
      .then((nextDashboard) => {
        if (refreshSequence.current !== requestSequence) return;
        if (activeProfileIdRef.current !== visibleProfileId) return;
        setDashboard(nextDashboard, visibleProfileId);
      })
      .catch((error) => {
        logRecoverableError("刷新看板失败，已等待下一次同步进度刷新", error);
      });
  }, [setDashboard]);

  const setDashboardForVisibleProfile = useCallback(
    (nextDashboard: DashboardData, profileId: string) => {
      if (activeProfileIdRef.current !== profileId) return;
      setDashboard(nextDashboard, profileId);
    },
    [setDashboard]
  );

  function markDashboardRefreshSettled() {
    refreshSequence.current += 1;
  }

  return {
    activeProfileIdRef,
    markDashboardRefreshSettled,
    refreshDashboardForArrivedBatch,
    setDashboardForVisibleProfile
  };
}
