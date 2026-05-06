import { testProfileRead } from "../../../api/profileReadApi";
import { PROFILE_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import { profileDisplayName } from "../../../utils/profiles";
import { createProfileDraft, deleteProfile, upsertProfile } from "../api/profilesApi";
import { isOfficialCliBindPlatform, type OfficialCliBindPlatform } from "../bind-flows/officialCli";
import type { ImProfile, Platform } from "../model/types";

interface UseProfilesControllerParams {
  orderedProfiles: ImProfile[];
  selectedProfiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
  cleanupPlatformCliIfUnused: (platform: Platform) => Promise<void>;
  openOfficialCliSetup: (platform: OfficialCliBindPlatform, profile: ImProfile) => void;
  setDeletingProfileId: (profileId: string) => void;
  setFormMessage: (message: string) => void;
  setPendingBulkDeleteIds: (profileIds: string[]) => void;
  setPendingDeleteId: (profileId: string) => void;
  setProfileMessage: (message: string) => void;
  setProfileMessageIsError: (isError: boolean) => void;
  setProfileNotice: (message: string, isError?: boolean) => void;
  updateProfileReadNotice: (notice: { id: string; message: string; variant: "info" | "success" | "error" }) => void;
}

export function useProfilesController({
  orderedProfiles,
  selectedProfiles,
  onProfilesChange,
  cleanupPlatformCliIfUnused,
  openOfficialCliSetup,
  setDeletingProfileId,
  setFormMessage,
  setPendingBulkDeleteIds,
  setPendingDeleteId,
  setProfileMessage,
  setProfileMessageIsError,
  setProfileNotice,
  updateProfileReadNotice
}: UseProfilesControllerParams) {
  function addProfile(platform: Platform) {
    setFormMessage("");
    if (platform === "wechat") {
      setProfileNotice(PROFILE_MESSAGES.wechatPaidOnly);
      return;
    }
    const draft = createProfileDraft(platform, nextProfileSortOrder());
    if (isOfficialCliBindPlatform(platform)) {
      openOfficialCliSetup(platform, draft);
    }
  }

  function nextProfileSortOrder() {
    if (orderedProfiles.length === 0) return 0;
    return Math.max(...orderedProfiles.map((profile) => profile.sortOrder)) + 1;
  }

  async function toggleProfile(profile: ImProfile) {
    await upsertProfile({
      ...profile,
      enabled: !profile.enabled,
      status: profile.enabled ? "disabled" : "normal",
      updatedAt: new Date().toISOString()
    });
    await onProfilesChange();
  }

  async function removeProfile(profile: ImProfile) {
    setPendingDeleteId(profile.id);
    setProfileMessageIsError(false);
    setProfileMessage(PROFILE_MESSAGES.deletePrompt(profileDisplayName(profile)));
  }

  async function confirmRemoveProfile(profile: ImProfile) {
    setDeletingProfileId(profile.id);
    try {
      await deleteProfile(profile.id);
      await cleanupPlatformCliIfUnused(profile.platform);
      setPendingDeleteId("");
      setProfileMessageIsError(false);
      setProfileMessage("");
      await onProfilesChange();
    } catch (error) {
      setProfileMessageIsError(true);
      setProfileMessage(userErrorMessage(error, PROFILE_MESSAGES.deleteFailed));
    } finally {
      setDeletingProfileId("");
    }
  }

  async function runProfileReadTest(profile: ImProfile) {
    const noticeId = `profile-read-${profile.id}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    updateProfileReadNotice({
      id: noticeId,
      message: PROFILE_MESSAGES.readTestStart(profileDisplayName(profile)),
      variant: "info"
    });
    try {
      const message = await testProfileRead(profile);
      updateProfileReadNotice({
        id: noticeId,
        message: PROFILE_MESSAGES.readTestSuccess(profileDisplayName(profile), message),
        variant: "success"
      });
    } catch (error) {
      updateProfileReadNotice({
        id: noticeId,
        message: PROFILE_MESSAGES.readTestFailedWithName(profileDisplayName(profile), userErrorMessage(error, PROFILE_MESSAGES.readTestFailed)),
        variant: "error"
      });
    }
  }

  async function testRead(profile: ImProfile) {
    await runProfileReadTest(profile);
  }

  async function testSelectedProfilesRead() {
    if (selectedProfiles.length === 0) {
      setProfileNotice(PROFILE_MESSAGES.selectForReadTest);
      return;
    }
    setPendingBulkDeleteIds([]);
    await Promise.all(selectedProfiles.map((profile) => runProfileReadTest(profile)));
  }

  function editProfile(profile: ImProfile) {
    setFormMessage("");
    if (profile.platform === "wechat") {
      setProfileNotice(PROFILE_MESSAGES.wechatPaidOnly);
      return;
    }
    if (isOfficialCliBindPlatform(profile.platform)) {
      openOfficialCliSetup(profile.platform, profile);
    }
  }

  return {
    addProfile,
    confirmRemoveProfile,
    editProfile,
    removeProfile,
    testRead,
    testSelectedProfilesRead,
    toggleProfile
  };
}
