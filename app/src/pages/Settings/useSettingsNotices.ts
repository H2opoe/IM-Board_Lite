import { useCallback, useRef, useState } from "react";
import type { FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { upsertNoticeToStackTop } from "../../components/shared/noticeStack";

export interface SettingsNotice {
  id: string;
  message: string;
  variant: FloatingNoticeVariant;
}

export function useSettingsNotices() {
  const noticeIdRef = useRef(0);
  const [notices, setNotices] = useState<SettingsNotice[]>([]);

  const pushNotice = useCallback((message: string, variant: FloatingNoticeVariant = "info") => {
    const id = `ai-settings-notice-${Date.now()}-${noticeIdRef.current++}`;
    setNotices((current) => [...current, { id, message, variant }]);
    return id;
  }, []);

  const setNotice = useCallback((id: string, message: string, variant: FloatingNoticeVariant = "info") => {
    setNotices((current) => upsertNoticeToStackTop(current, { id, message, variant }));
  }, []);

  const dismissNotice = useCallback((id: string) => {
    setNotices((current) => current.filter((notice) => notice.id !== id));
  }, []);

  return { notices, pushNotice, setNotice, dismissNotice };
}
