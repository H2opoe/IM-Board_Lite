import { useCallback, useEffect, useRef, useState } from "react";
import { getDashboard, markActionItem } from "../api/dashboardApi";
import type { ActionItem, DashboardData } from "../model/types";
import { userErrorMessage } from "../../../utils/errors";

export function useDashboardActions(activeProfileId: string, onDashboardChange: (data: DashboardData) => void) {
  const mounted = useRef(false);
  const contextGeneration = useRef(0);
  const readSequence = useRef(0);
  const currentProfileId = useRef(activeProfileId);
  const failedOperation = useRef<{ itemId: string; status: string; retry: () => Promise<void> } | null>(null);
  const retrying = useRef(false);
  const [actionError, setActionError] = useState("");
  const [isActionRetrying, setIsActionRetrying] = useState(false);
  currentProfileId.current = activeProfileId;

  useEffect(() => {
    mounted.current = true;
    contextGeneration.current += 1;
    failedOperation.current = null;
    retrying.current = false;
    setActionError("");
    setIsActionRetrying(false);
    return () => {
      mounted.current = false;
      contextGeneration.current += 1;
    };
  }, [activeProfileId]);

  const updateActionStatus = useCallback(async function updateActionStatus(item: ActionItem, status: "open" | "done") {
    if (!mounted.current || currentProfileId.current !== activeProfileId) return;
    const context = contextGeneration.current;
    const isCurrent = () => mounted.current && currentProfileId.current === activeProfileId && context === contextGeneration.current;
    const publishFailure = (error: unknown, retry: () => Promise<void>) => {
      if (!isCurrent()) return;
      failedOperation.current = { itemId: item.id, status, retry };
      setActionError(userErrorMessage(error, "事项状态更新失败，请重试。"));
    };
    const refreshAfterMark = async () => {
      if (!isCurrent()) return;
      // Marks may finish in either order; each successful mark gets a new read.
      const read = ++readSequence.current;
      try {
        const next = await getDashboard(activeProfileId);
        if (!isCurrent() || read !== readSequence.current) return;
        onDashboardChange(next);
        if (failedOperation.current?.itemId === item.id && failedOperation.current.status === status) {
          failedOperation.current = null;
          setActionError("");
        }
      } catch (error) {
        if (read === readSequence.current) publishFailure(error, refreshAfterMark);
      }
    };
    try {
      // A status change already sent to the backend remains valid after navigation.
      await markActionItem(item.id, status);
    } catch (error) {
      publishFailure(error, () => updateActionStatus(item, status));
      return;
    }
    await refreshAfterMark();
  }, [activeProfileId, onDashboardChange]);

  const retryAction = useCallback(async () => {
    if (!mounted.current || retrying.current || !failedOperation.current) return;
    const context = contextGeneration.current;
    retrying.current = true;
    setIsActionRetrying(true);
    try {
      await failedOperation.current.retry();
    } finally {
      if (mounted.current && context === contextGeneration.current) {
        retrying.current = false;
        setIsActionRetrying(false);
      }
    }
  }, []);
  const complete = useCallback((item: ActionItem) => updateActionStatus(item, "done"), [updateActionStatus]);
  const reopen = useCallback((item: ActionItem) => updateActionStatus(item, "open"), [updateActionStatus]);
  return { complete, reopen, actionError, retryAction, isActionRetrying };
}
