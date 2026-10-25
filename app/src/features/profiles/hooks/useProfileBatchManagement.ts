import { useEffect, useRef, useState } from "react";
import { deleteProfile, upsertProfile } from "../api/profilesApi";
import { APP_MESSAGES, PROFILE_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import type { ImProfile } from "../model/types";

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
}: UseProfileBatchManagementParams) {
  const batchRunning = useRef(false);
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

  async function showBatchResult(message: string, failed: boolean) {
    try {
      await onProfilesChange();
      setProfileNotice(message, failed);
    } catch (error) {
      setProfileNotice(`${message} 刷新账号列表失败：${userErrorMessage(error, "请重新刷新。")}`, true);
    }
  }

  async function setSelectedProfilesEnabled(enabled: boolean) {
    if (batchRunning.current) return;
    if (selectedProfiles.length === 0) {
      setProfileNotice(PROFILE_MESSAGES.selectForManage);
      return;
    }
    batchRunning.current = true;
    const profilesToUpdate = selectedProfiles;
    const label = enabled ? "启用" : "暂停";
    setPendingBulkDeleteIds([]);
    setProfileNotice(PROFILE_MESSAGES.batchStatusStart(label, profilesToUpdate.length));
    try {
      const results = await Promise.allSettled(
        profilesToUpdate.map((profile) =>
          upsertProfile({
            ...profile,
            enabled,
            status: enabled ? "normal" : "disabled",
            updatedAt: profile.updatedAt
          })
        )
      );
      const failures = results.filter((result): result is PromiseRejectedResult => result.status === "rejected");
      const message = failures.length === 0
        ? PROFILE_MESSAGES.batchStatusSuccess(label, results.length)
        : `批量${label}：完成 ${results.length - failures.length} 个，未完成 ${failures.length} 个。${userErrorMessage(failures[0].reason, PROFILE_MESSAGES.batchStatusFailed(label))}`;
      await showBatchResult(message, failures.length > 0);
    } finally {
      batchRunning.current = false;
    }
  }

  function requestBulkDeleteProfiles() {
    if (batchRunning.current) return;
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
    if (batchRunning.current) return;
    const profilesToDelete = orderedProfiles.filter((profile) => pendingBulkDeleteIds.includes(profile.id));
    if (profilesToDelete.length === 0) {
      setPendingBulkDeleteIds([]);
      return;
    }
    batchRunning.current = true;
    setDeletingProfileId(BULK_DELETE_ID);
    const failedIds: string[] = [];
    let firstError: unknown;
    try {
      for (const profile of profilesToDelete) {
        try {
          await deleteProfile(profile.id);
        } catch (error) {
          failedIds.push(profile.id);
          firstError ??= error;
        }
      }
      setPendingBulkDeleteIds(failedIds);
      setSelectedProfileIds(new Set(failedIds));
      const message = failedIds.length === 0
        ? PROFILE_MESSAGES.bulkDeleteSuccess(profilesToDelete.length)
        : `批量删除：完成 ${profilesToDelete.length - failedIds.length} 个，未完成 ${failedIds.length} 个。${userErrorMessage(firstError, PROFILE_MESSAGES.bulkDeleteFailed)}`;
      await showBatchResult(message, failedIds.length > 0);
    } finally {
      setDeletingProfileId("");
      batchRunning.current = false;
    }
  }

  function handleBulkDeleteProfiles() {
    if (isBulkDeleteConfirming) {
      return confirmBulkDeleteProfiles();
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
