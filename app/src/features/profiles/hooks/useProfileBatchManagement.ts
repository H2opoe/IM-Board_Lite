import { useEffect, useState } from "react";
import { deleteProfile, upsertProfile } from "../api/profilesApi";
import { APP_MESSAGES, PROFILE_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import type { ImProfile, Platform } from "../model/types";

const BULK_DELETE_ID = "__bulk_delete__";

interface UseProfileBatchManagementParams {
  isBatchManaging: boolean;
  orderedProfiles: ImProfile[];
  draggedProfileId: string | null;
  onProfilesChange: () => Promise<void>;
  resetProfileDragState: () => void;
  setIsBatchManaging: (value: boolean) => void;
  setProfileNotice: (message: string, isError?: boolean) => void;
  clearPendingDelete: () => void;
  cleanupPlatformCliIfUnused: (platform: Platform) => Promise<void>;
}

export function useProfileBatchManagement({
  isBatchManaging,
  orderedProfiles,
  draggedProfileId,
  onProfilesChange,
  resetProfileDragState,
  setIsBatchManaging,
  setProfileNotice,
  clearPendingDelete,
  cleanupPlatformCliIfUnused
}: UseProfileBatchManagementParams) {
  const [pendingBulkDeleteIds, setPendingBulkDeleteIds] = useState<string[]>([]);
  const [deletingProfileId, setDeletingProfileId] = useState("");
  const [selectedProfileIds, setSelectedProfileIds] = useState<Set<string>>(() => new Set());

  const selectedProfiles = orderedProfiles.filter((profile) => selectedProfileIds.has(profile.id));
  const selectedProfileCount = selectedProfiles.length;
  const isAllProfilesSelected = orderedProfiles.length > 0 && selectedProfileCount === orderedProfiles.length;
  const enabledProfileCount = orderedProfiles.filter((profile) => profile.enabled).length;
  const disabledProfileCount = orderedProfiles.length - enabledProfileCount;
  const batchStatusAction: "enable" | "pause" = disabledProfileCount > enabledProfileCount ? "enable" : "pause";
  const isBulkDeleteConfirming = pendingBulkDeleteIds.length > 0;

  useEffect(() => {
    const currentProfileIds = new Set(orderedProfiles.map((profile) => profile.id));
    setSelectedProfileIds((currentIds) => {
      const nextIds = new Set([...currentIds].filter((profileId) => currentProfileIds.has(profileId)));
      return nextIds.size === currentIds.size ? currentIds : nextIds;
    });
    setPendingBulkDeleteIds((currentIds) => {
      const nextIds = currentIds.filter((profileId) => currentProfileIds.has(profileId));
      return nextIds.length === currentIds.length ? currentIds : nextIds;
    });
    if (draggedProfileId && !currentProfileIds.has(draggedProfileId)) {
      resetProfileDragState();
    }
  }, [draggedProfileId, orderedProfiles, resetProfileDragState]);

  function exitBatchManagement() {
    setIsBatchManaging(false);
    setSelectedProfileIds(new Set());
    setPendingBulkDeleteIds([]);
    resetProfileDragState();
  }

  function toggleBatchManagement() {
    if (isBatchManaging) {
      exitBatchManagement();
      return;
    }
    setIsBatchManaging(true);
  }

  function toggleProfileSelection(profileId: string) {
    setPendingBulkDeleteIds([]);
    setSelectedProfileIds((currentIds) => {
      const nextIds = new Set(currentIds);
      if (nextIds.has(profileId)) {
        nextIds.delete(profileId);
      } else {
        nextIds.add(profileId);
      }
      return nextIds;
    });
  }

  function toggleAllProfilesSelected() {
    setPendingBulkDeleteIds([]);
    setSelectedProfileIds((currentIds) => {
      if (orderedProfiles.length > 0 && currentIds.size === orderedProfiles.length) {
        return new Set();
      }
      return new Set(orderedProfiles.map((profile) => profile.id));
    });
  }

  async function setSelectedProfilesEnabled(enabled: boolean) {
    if (selectedProfiles.length === 0) {
      setProfileNotice(PROFILE_MESSAGES.selectForManage);
      return;
    }
    const label = enabled ? "启用" : "暂停";
    setPendingBulkDeleteIds([]);
    setProfileNotice(PROFILE_MESSAGES.batchStatusStart(label, selectedProfiles.length));
    try {
      await Promise.all(
        selectedProfiles.map((profile) =>
          upsertProfile({
            ...profile,
            enabled,
            status: enabled ? "normal" : "disabled",
            updatedAt: new Date().toISOString()
          })
        )
      );
      setProfileNotice(PROFILE_MESSAGES.batchStatusSuccess(label, selectedProfiles.length));
      await onProfilesChange();
    } catch (error) {
      setProfileNotice(userErrorMessage(error, PROFILE_MESSAGES.batchStatusFailed(label)), true);
    }
  }

  function requestBulkDeleteProfiles() {
    if (selectedProfiles.length === 0) {
      setProfileNotice(PROFILE_MESSAGES.selectForDelete);
      return;
    }
    clearPendingDelete();
    setPendingBulkDeleteIds(selectedProfiles.map((profile) => profile.id));
    setProfileNotice(PROFILE_MESSAGES.bulkDeletePrompt(selectedProfiles.length));
  }

  function bulkDeleteButtonLabel() {
    if (deletingProfileId === BULK_DELETE_ID) return APP_MESSAGES.deleteInProgress;
    if (isBulkDeleteConfirming) return PROFILE_MESSAGES.bulkDeleteConfirmButton(pendingBulkDeleteIds.length);
    return PROFILE_MESSAGES.batchDeleteButton;
  }

  async function confirmBulkDeleteProfiles() {
    const profilesToDelete = orderedProfiles.filter((profile) => pendingBulkDeleteIds.includes(profile.id));
    if (profilesToDelete.length === 0) {
      setPendingBulkDeleteIds([]);
      return;
    }
    setDeletingProfileId(BULK_DELETE_ID);
    try {
      for (const profile of profilesToDelete) {
        await deleteProfile(profile.id);
      }
      for (const platform of new Set(profilesToDelete.map((profile) => profile.platform))) {
        await cleanupPlatformCliIfUnused(platform);
      }
      setPendingBulkDeleteIds([]);
      setSelectedProfileIds(new Set());
      setProfileNotice(PROFILE_MESSAGES.bulkDeleteSuccess(profilesToDelete.length));
      await onProfilesChange();
    } catch (error) {
      setProfileNotice(userErrorMessage(error, PROFILE_MESSAGES.bulkDeleteFailed), true);
    } finally {
      setDeletingProfileId("");
    }
  }

  function handleBulkDeleteProfiles() {
    if (isBulkDeleteConfirming) {
      void confirmBulkDeleteProfiles();
      return;
    }
    requestBulkDeleteProfiles();
  }

  return {
    batchStatusAction,
    bulkDeleteButtonLabel,
    bulkDeleteId: BULK_DELETE_ID,
    deletingProfileId,
    exitBatchManagement,
    handleBulkDeleteProfiles,
    isAllProfilesSelected,
    isBatchManaging,
    isBulkDeleteConfirming,
    pendingBulkDeleteIds,
    selectedProfileCount,
    selectedProfileIds,
    selectedProfiles,
    setDeletingProfileId,
    setPendingBulkDeleteIds,
    setSelectedProfilesEnabled,
    toggleAllProfilesSelected,
    toggleBatchManagement,
    toggleProfileSelection
  };
}
