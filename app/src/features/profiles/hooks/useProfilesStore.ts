import { useCallback, useEffect, useRef, useState } from "react";
import { listProfiles } from "../api/profilesApi";
import type { ImProfile } from "../model/types";
import { userErrorMessage } from "../../../utils/errors";

export function useProfilesStore() {
  const [profiles, setProfiles] = useState<ImProfile[]>([]);
  const [profilesError, setProfilesError] = useState("");
  const [isRefreshing, setIsRefreshing] = useState(false);
  const mounted = useRef(false);
  const refreshing = useRef(false);
  const requestSequence = useRef(0);

  const refreshProfiles = useCallback(async () => {
    if (!mounted.current) return;
    const request = ++requestSequence.current;
    refreshing.current = true;
    setIsRefreshing(true);
    const isCurrent = () => mounted.current && request === requestSequence.current;
    try {
      const next = await listProfiles();
      if (isCurrent()) {
        setProfiles(next);
        setProfilesError("");
      }
    } catch (error) {
      if (isCurrent()) setProfilesError(userErrorMessage(error, "账号列表读取失败，请重试。"));
      throw error;
    } finally {
      if (isCurrent()) {
        refreshing.current = false;
        setIsRefreshing(false);
      }
    }
  }, []);

  const refreshProfilesForUi = useCallback(async () => {
    try {
      await refreshProfiles();
    } catch {
      // refreshProfiles publishes a visible error and preserves the displayed list.
      // Explicit refresh callers still receive the rejection for their own workflow.
    }
  }, [refreshProfiles]);

  const retryProfiles = useCallback(async () => {
    if (refreshing.current || !mounted.current) return;
    await refreshProfilesForUi();
  }, [refreshProfilesForUi]);

  useEffect(() => {
    mounted.current = true;
    void refreshProfilesForUi();
    const onProfilesChanged = () => { void refreshProfilesForUi(); };
    window.addEventListener("im-board-profiles-changed", onProfilesChanged);
    return () => {
      mounted.current = false;
      refreshing.current = false;
      requestSequence.current += 1;
      window.removeEventListener("im-board-profiles-changed", onProfilesChanged);
    };
  }, [refreshProfilesForUi]);

  return { profiles, refreshProfiles, profilesError, isRefreshing, retryProfiles };
}
