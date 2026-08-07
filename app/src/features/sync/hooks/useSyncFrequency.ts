import { useEffect, useRef, useState } from "react";
import { isTauri } from "../../../api/tauri";
import {
  initializeAutoSyncFrequencyMinutes,
  setAutoSyncFrequencyMinutes
} from "../api/syncApi";
import { logRecoverableError } from "../../../utils/logging";

const DEFAULT_SYNC_FREQUENCY_MINUTES = 15;
const SYNC_FREQUENCY_STORAGE_KEY = "im-board-sync-frequency-minutes";

function loadSyncFrequency() {
  const stored = window.localStorage.getItem(SYNC_FREQUENCY_STORAGE_KEY);
  const parsed = stored ? Number.parseInt(stored, 10) : DEFAULT_SYNC_FREQUENCY_MINUTES;
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_SYNC_FREQUENCY_MINUTES;
}

export function useSyncFrequency(demoMode: boolean) {
  const [syncFrequencyMinutes, setSyncFrequencyMinutes] = useState(loadSyncFrequency);
  const hydrated = useRef(false);

  useEffect(() => {
    if (demoMode || !isTauri) {
      hydrated.current = true;
      return;
    }
    let active = true;
    void initializeAutoSyncFrequencyMinutes(loadSyncFrequency())
      .then((minutes) => {
        if (!active) return;
        hydrated.current = true;
        setSyncFrequencyMinutes(minutes);
      })
      .catch((error) => {
        hydrated.current = true;
        logRecoverableError("读取后台同步频率失败，已使用默认设置", error);
      });
    return () => {
      active = false;
    };
  }, [demoMode]);

  useEffect(() => {
    window.localStorage.setItem(SYNC_FREQUENCY_STORAGE_KEY, String(syncFrequencyMinutes));
    if (demoMode || !isTauri || !hydrated.current) return;
    void setAutoSyncFrequencyMinutes(syncFrequencyMinutes).catch((error) => {
      logRecoverableError("更新后台同步频率失败，已保留当前前端设置", error);
    });
  }, [demoMode, syncFrequencyMinutes]);

  return { syncFrequencyMinutes, setSyncFrequencyMinutes };
}
