import { useCallback, useEffect, useState, type SetStateAction } from "react";
import { getDashboard } from "../api/dashboardApi";
import type { DashboardData } from "../model/types";

export function useDashboardStore(activeProfileId: string, isDashboardActive: boolean) {
  const [dashboard, setDashboard] = useState<DashboardData | null>(null);
  const [dashboardProfileId, setDashboardProfileId] = useState("");

  const setDashboardForProfile = useCallback(
    (nextDashboard: SetStateAction<DashboardData | null>, profileId = activeProfileId) => {
      setDashboard(nextDashboard);
      setDashboardProfileId(profileId);
    },
    [activeProfileId]
  );

  const refreshDashboard = useCallback(
    async (profileId = activeProfileId) => {
      const nextDashboard = await getDashboard(profileId);
      setDashboardForProfile(nextDashboard, profileId);
      return nextDashboard;
    },
    [activeProfileId, setDashboardForProfile]
  );

  useEffect(() => {
    if (!isDashboardActive) return undefined;

    let isCancelled = false;
    setDashboard(null);
    getDashboard(activeProfileId).then((nextDashboard) => {
      if (isCancelled) return;
      setDashboardForProfile(nextDashboard, activeProfileId);
    });
    return () => {
      isCancelled = true;
    };
  }, [activeProfileId, isDashboardActive, setDashboardForProfile]);

  return {
    dashboard,
    dashboardProfileId,
    setDashboard: setDashboardForProfile,
    refreshDashboard
  };
}
