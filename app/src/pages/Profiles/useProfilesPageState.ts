import { useState } from "react";
import type { Platform } from "../../features/profiles/model/types";
import { PROFILE_MESSAGES } from "../../constants/messages";

export function useProfilesPageState() {
  const [updatingCliPlatform, setUpdatingCliPlatform] = useState<Platform | null>(null);
  const [formMessage, setFormMessage] = useState("");
  const [profileMessage, setProfileMessage] = useState("");
  const [profileMessageIsError, setProfileMessageIsError] = useState(false);
  const [isAboutOpen, setIsAboutOpen] = useState(false);
  const [pendingDeleteId, setPendingDeleteId] = useState("");
  const [isBatchManaging, setIsBatchManaging] = useState(false);
  const [highlightedProfileId, setHighlightedProfileId] = useState("");

  function setProfileNotice(message: string, isError = false) {
    setProfileMessageIsError(isError);
    setProfileMessage(message);
  }

  function handleDuplicateProfileReplaced(profileId: string) {
    setHighlightedProfileId(profileId);
    setProfileNotice(PROFILE_MESSAGES.accountUpdatedByDuplicate);
  }

  return {
    formMessage,
    handleDuplicateProfileReplaced,
    highlightedProfileId,
    isAboutOpen,
    isBatchManaging,
    pendingDeleteId,
    profileMessage,
    profileMessageIsError,
    setFormMessage,
    setHighlightedProfileId,
    setIsAboutOpen,
    setIsBatchManaging,
    setPendingDeleteId,
    setProfileMessage,
    setProfileMessageIsError,
    setProfileNotice,
    setUpdatingCliPlatform,
    updatingCliPlatform
  };
}
