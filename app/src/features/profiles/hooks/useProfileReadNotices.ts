import { useState } from "react";
import type { FloatingNoticeVariant } from "../../../components/shared/FloatingNotice";
import { upsertNoticeToStackTop } from "../../../components/shared/noticeStack";

export interface ProfileReadNotice {
  id: string;
  message: string;
  variant: FloatingNoticeVariant;
}

export function useProfileReadNotices() {
  const [profileReadNotices, setProfileReadNotices] = useState<ProfileReadNotice[]>([]);

  function updateProfileReadNotice(notice: ProfileReadNotice) {
    setProfileReadNotices((currentNotices) => {
      return upsertNoticeToStackTop(currentNotices, notice);
    });
  }

  function closeProfileReadNotice(noticeId: string) {
    setProfileReadNotices((currentNotices) => currentNotices.filter((notice) => notice.id !== noticeId));
  }

  return {
    closeProfileReadNotice,
    profileReadNotices,
    updateProfileReadNotice
  };
}
