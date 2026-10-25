import { useEffect, useRef } from "react";
import { releaseUnusedDraft } from "./useBindingDraft";
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
  const addRequest = useRef(0);
  useEffect(() => () => { addRequest.current += 1; }, []);
  async function addProfile(platform: Platform) {
    const request = ++addRequest.current;
    let unclaimedDraft: ImProfile | null = null;
    setFormMessage("");
    try {
      const draft = await createProfileDraft(platform, nextProfileSortOrder());
      unclaimedDraft = draft;
      if (request !== addRequest.current) { releaseUnusedDraft(draft.id); return; }
      if (isOfficialCliBindPlatform(platform)) {
        openOfficialCliSetup(platform, draft);
        unclaimedDraft = null;
      }
      if (unclaimedDraft) releaseUnusedDraft(unclaimedDraft.id);
    } catch (error) {
      if (unclaimedDraft) releaseUnusedDraft(unclaimedDraft.id);
      if (request !== addRequest.current) return;
      setFormMessage(userErrorMessage(error, "创建账号配置失败，请重试。"));
    }
  }

  function nextProfileSortOrder() {
    if (orderedProfiles.length === 0) return 0;
    return Math.max(...orderedProfiles.map((profile) => profile.sortOrder)) + 1;
  }

  async function toggleProfile(profile: ImProfile) {
    try {
      await upsertProfile({
        ...profile,
        enabled: !profile.enabled,
        status: profile.enabled ? "disabled" : "normal",
        updatedAt: profile.updatedAt
      });
      await onProfilesChange();
    } catch (error) {
      setProfileMessageIsError(true);
      setProfileMessage(userErrorMessage(error, "账号状态保存失败，请刷新后重试。"));
    }
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
