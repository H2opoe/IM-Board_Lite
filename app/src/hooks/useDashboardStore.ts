import { useCallback, useEffect, useState } from "react";
import { getDashboard } from "../api/dashboardApi";
import type { DashboardData } from "../types";

export function useDashboardStore(activeProfileId: string, isDashboardActive: boolean) {
  const [dashboard, setDashboard] = useState<DashboardData | null>(null);
  const [dashboardProfileId, setDashboardProfileId] = useState("");

  const refreshDashboard = useCallback(
    async (profileId = activeProfileId) => {
      const nextDashboard = await getDashboard(profileId);
      setDashboard(nextDashboard);
      setDashboardProfileId(profileId);
      return nextDashboard;
    },
    [activeProfileId]
  );

  useEffect(() => {
    if (!isDashboardActive) return undefined;

    let isCancelled = false;
    setDashboard(null);
    getDashboard(activeProfileId).then((nextDashboard) => {
      if (isCancelled) return;
      setDashboard(nextDashboard);
      setDashboardProfileId(activeProfileId);
    });
    return () => {
      isCancelled = true;
    };
  }, [activeProfileId, isDashboardActive]);

  return {
    dashboard,
    dashboardProfileId,
    setDashboard,
    refreshDashboard
  };
}
