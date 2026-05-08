import { useEffect, useState } from "react";
import { Settings2 } from "lucide-react";
import {
  deployPlatformBridge,
  updatePlatformCli
} from "../../api/bridgeApi";
import type { PlatformDeployment } from "../../api/bridgeApi";
import { reorderProfiles } from "../../features/profiles/api/profilesApi";
import type { ImProfile, Platform } from "../../features/profiles/model/types";
import { FloatingNotice, FloatingNoticeStack, type FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { PagedTextBlock } from "../../components/shared/PagedTextBlock";
import { APP_MESSAGES, OFFICIAL_CLI_MESSAGES, PROFILE_MESSAGES } from "../../constants/messages";
import { userErrorMessage } from "../../utils/errors";
import { profileDisplayName } from "../../utils/profiles";
import { isOfficialCliBindPlatform } from "../../features/profiles/bind-flows/officialCli";
import { ProfileAccountList, ProfileBatchToolbar, ProfilePlatformEntryPanel } from "../../features/profiles/bind-flows/profileList";
import { useProfileDragOrdering } from "../../features/profiles/hooks/useProfileDragOrdering";
import { useModalEnterGuard } from "../../features/profiles/hooks/useModalEnterGuard";
import { useProfileBatchManagement } from "../../features/profiles/hooks/useProfileBatchManagement";
import { useCopiedCommandStatus } from "../../features/profiles/hooks/useCopiedCommandStatus";
import { useProfileReadNotices } from "../../features/profiles/hooks/useProfileReadNotices";
import { useProfilesController } from "../../features/profiles/hooks/useProfilesController";
import { usePlatformCliDeployment } from "../../features/profiles/hooks/usePlatformCliDeployment";
import { useOfficialCliBindController } from "../../features/profiles/hooks/useOfficialCliBindController";
import { ProfilesPageModals } from "./ProfilesPageModals";
import { DeveloperFeedbackModal } from "../../components/DeveloperFeedbackModal";

interface Props {
  profiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
}

export function ProfilesPage({ profiles, onProfilesChange }: Props) {
  const orderedProfiles = [...profiles].sort((left, right) => left.sortOrder - right.sortOrder);
  const [updatingCliPlatform, setUpdatingCliPlatform] = useState<Platform | null>(null);
  const [formMessage, setFormMessage] = useState("");
  const [profileMessage, setProfileMessage] = useState("");
  const [profileMessageIsError, setProfileMessageIsError] = useState(false);
  const [isAboutOpen, setIsAboutOpen] = useState(false);
  const [isDeveloperContactOpen, setIsDeveloperContactOpen] = useState(false);
  const [pendingDeleteId, setPendingDeleteId] = useState("");
  const [isBatchManaging, setIsBatchManaging] = useState(false);
  const {
    cancelDeploymentRequest,
    cleanupPlatformCliIfUnused,
    cliDeploymentProgress,
    isDeploymentRequestActive,
    nextDeploymentRequest,
    resetCliDeploymentProgress,
    watchDeploymentProgressFor
  } = usePlatformCliDeployment();
  const {
    copiedCommandPlatform,
    resetCopiedCommandStatus,
    scheduleCopiedCommandReset,
    setCopiedCommandPlatform
  } = useCopiedCommandStatus();
  const {
    closeProfileReadNotice,
    profileReadNotices,
    updateProfileReadNotice
  } = useProfileReadNotices();
  const {
    resetModalCompositionState,
    handleModalCompositionStart,
    handleModalCompositionEnd,
    shouldHandleModalEnter
  } = useModalEnterGuard();
  const {
    closeOfficialCliSetup,
    isSavingOfficialCliProfile,
    officialCliFlow,
    openOfficialCliSetup,
    saveOfficialCliProfile,
    saveOfficialCliSetupFromEnter,
    setOfficialCliFlow,
    updateOfficialCliFlow
  } = useOfficialCliBindController({
    cancelDeploymentRequest,
    cleanupPlatformCliIfUnused,
    isDeploymentRequestActive,
    nextDeploymentRequest,
    onProfilesChange,
    orderedProfiles,
    preservePersistedProfileState,
    resetCliDeploymentProgress,
    resetCopiedCommandStatus,
    runAfterModalPaint,
    setFormMessage,
    setUpdatingCliPlatform,
    shouldHandleModalEnter,
    watchDeploymentProgressFor
  });
  const {
    visibleProfiles,
    draggedProfile,
    draggedProfileId,
    dragTargetProfileId,
    dragPlacement,
    dragVisualState,
    profileTableRef,
    profileDragRowStyle,
    resetProfileDragState,
    startProfilePointerDrag,
    startProfileRowPointerDrag
  } = useProfileDragOrdering({ orderedProfiles, isBatchManaging, saveProfileOrder });

  const {
    batchStatusAction,
    bulkDeleteButtonLabel,
    bulkDeleteId,
    deletingProfileId,
    exitBatchManagement,
    handleBulkDeleteProfiles,
    isAllProfilesSelected,
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
  } = useProfileBatchManagement({
    clearPendingDelete: () => setPendingDeleteId(""),
    cleanupPlatformCliIfUnused,
    draggedProfileId,
    isBatchManaging,
    onProfilesChange,
    orderedProfiles,
    resetProfileDragState,
    setIsBatchManaging,
    setProfileNotice,
    appendProfileNotice
  });
  const {
    addProfile,
    confirmRemoveProfile,
    editProfile,
    removeProfile,
    testRead,
    testSelectedProfilesRead,
    toggleProfile
  } = useProfilesController({
    cleanupPlatformCliIfUnused,
    onProfilesChange,
    openOfficialCliSetup,
    orderedProfiles,
    selectedProfiles,
    setDeletingProfileId,
    setFormMessage,
    setPendingBulkDeleteIds,
    setPendingDeleteId,
    setProfileMessage,
    setProfileMessageIsError,
    setProfileNotice,
    updateProfileReadNotice
  });
  const profileNoticeVariant: FloatingNoticeVariant = profileMessageIsError
    ? "error"
    : pendingDeleteId || isBulkDeleteConfirming || profileMessage.startsWith("正在") || profileMessage.startsWith(APP_MESSAGES.confirmDelete)
      ? "info"
      : "success";
  const modalFormMessageVariant: FloatingNoticeVariant = "error";

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (!isAboutOpen && !isDeveloperContactOpen && !officialCliFlow.profile && !isBatchManaging) return;
      event.preventDefault();

      if (isAboutOpen) {
        setIsAboutOpen(false);
        return;
      }
      if (isDeveloperContactOpen) {
        setIsDeveloperContactOpen(false);
        return;
      }
      if (officialCliFlow.profile) {
        closeOfficialCliSetup();
        return;
      }
      exitBatchManagement();
    }

    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [isAboutOpen, isBatchManaging, isDeveloperContactOpen, officialCliFlow.profile]);

  useEffect(() => {
    if (officialCliFlow.profile) return;
    resetModalCompositionState();
  }, [officialCliFlow.profile]);

  function setProfileNotice(message: string, isError = false) {
    setProfileMessageIsError(isError);
    setProfileMessage(message);
  }

  function appendProfileNotice(message: string, variant: FloatingNoticeVariant) {
    updateProfileReadNotice({
      id: `profile-notice-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      message,
      variant
    });
  }

  async function saveProfileOrder(nextProfiles: ImProfile[]) {
    try {
      await reorderProfiles(nextProfiles);
      setProfileNotice(PROFILE_MESSAGES.profileOrderSaved);
      await onProfilesChange();
    } catch (error) {
      setProfileNotice(userErrorMessage(error, PROFILE_MESSAGES.profileOrderFailed), true);
    }
  }

  async function copyOfficialCliCommand(platform: Platform, deployment: PlatformDeployment | null) {
    if (!deployment) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error(OFFICIAL_CLI_MESSAGES.clipboardUnsupported);
      }
      await navigator.clipboard.writeText(deployment.command);
      setCopiedCommandPlatform(platform);
      scheduleCopiedCommandReset(platform);
      setFormMessage("");
    } catch (error) {
      resetCopiedCommandStatus();
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.copyFailed));
    }
  }

  async function copyActiveOfficialCliCommand() {
    if (!officialCliFlow.platform) return;
    await copyOfficialCliCommand(officialCliFlow.platform, officialCliFlow.deployment);
  }

  async function updateOfficialCli(platform: Platform) {
    setUpdatingCliPlatform(platform);
    setFormMessage("");
    try {
      const status = await updatePlatformCli(platform);
      if (isOfficialCliBindPlatform(platform)) {
        updateOfficialCliFlow(platform, { versionStatus: status });
        const setupProfile = officialCliFlow.platform === platform ? officialCliFlow.profile : null;
        const deployment = setupProfile ? await deployPlatformBridge(platform, setupProfile.id) : null;
        setOfficialCliFlow((current) => {
          if (current.platform !== platform) return current;
          return {
            ...current,
            versionStatus: status,
            deployment: deployment ?? (current.deployment ? { ...current.deployment, currentVersion: status.currentVersion } : current.deployment),
            cliPath: deployment?.cliPath ?? current.cliPath
          };
        });
      }
    } catch (error) {
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.updateFailed(platform)));
    } finally {
      setUpdatingCliPlatform(null);
    }
  }

  function isPersistedProfile(profile: ImProfile) {
    return profiles.some((item) => item.id === profile.id);
  }

  function preservePersistedProfileState(original: ImProfile, nextProfile: ImProfile) {
    if (!isPersistedProfile(original)) return nextProfile;
    return {
      ...nextProfile,
      enabled: original.enabled,
      status: original.status,
      sortOrder: original.sortOrder
    };
  }

  function runAfterModalPaint(task: () => void) {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => window.setTimeout(task, 80));
    });
  }

  function renderModalFormMessage() {
    if (!formMessage) return null;
    const messageElement = formMessage.includes("\n") ? "pre" : "span";
    return (
      <FloatingNoticeStack scope="modal">
        <FloatingNotice
          variant={modalFormMessageVariant}
          withinLayer
          autoCloseMs={false}
          onClose={() => setFormMessage("")}
        >
          <PagedTextBlock
            text={formMessage}
            className="modal-form-message-content"
            controlsClassName="modal-form-message-pager"
            element={messageElement}
            maxLines={4}
            compactCopy
          />
        </FloatingNotice>
      </FloatingNoticeStack>
    );
  }

  return (
    <div className="page-surface profile-page">
      <header className="topbar">
        <div>
          <h1>平台管理</h1>
          <span>新增、排序、展示或删除平台</span>
        </div>
      </header>
      {(profileMessage || profileReadNotices.length > 0) && (
        <FloatingNoticeStack>
          {[...profileReadNotices].reverse().map((notice) => (
            <FloatingNotice
              key={notice.id}
              message={notice.message}
              variant={notice.variant}
              withinLayer
              autoCloseMs={notice.variant === "success" ? undefined : false}
              onClose={() => closeProfileReadNotice(notice.id)}
            />
          ))}
          {profileMessage && (
            <FloatingNotice
              message={profileMessage}
              variant={profileNoticeVariant}
              withinLayer
              autoCloseMs={profileNoticeVariant === "success" ? undefined : false}
              onClose={() => {
                if (pendingDeleteId && profileMessage.startsWith(APP_MESSAGES.confirmDelete)) {
                  setPendingDeleteId("");
                }
                if (pendingBulkDeleteIds.length > 0 && profileMessage.startsWith(APP_MESSAGES.confirmDelete)) {
                  setPendingBulkDeleteIds([]);
                }
                setProfileMessage("");
                setProfileMessageIsError(false);
              }}
            />
          )}
        </FloatingNoticeStack>
      )}

      <section className="profile-layout">
        <ProfilePlatformEntryPanel
          orderedProfiles={orderedProfiles}
          onAddProfile={addProfile}
          onOpenAbout={() => setIsAboutOpen(true)}
          onOpenDeveloperContact={() => setIsDeveloperContactOpen(true)}
        />

        <article className="panel profile-list-panel">
          <header className="panel-header">
            <div>
              <strong>已绑定账号</strong>
              <span>{orderedProfiles.length}个账号</span>
            </div>
            <button className={`secondary-button ${isBatchManaging ? "active" : ""}`.trim()} onClick={toggleBatchManagement}>
              <Settings2 size={16} />
              {isBatchManaging ? "完成管理" : "批量管理/排序"}
            </button>
          </header>
          {isBatchManaging && orderedProfiles.length > 0 && (
            <ProfileBatchToolbar
              isAllProfilesSelected={isAllProfilesSelected}
              selectedProfileCount={selectedProfileCount}
              isBulkDeleteConfirming={isBulkDeleteConfirming}
              deletingProfileId={deletingProfileId}
              bulkDeleteId={bulkDeleteId}
              bulkDeleteButtonLabel={bulkDeleteButtonLabel()}
              batchStatusAction={batchStatusAction}
              onToggleAll={toggleAllProfilesSelected}
              onBatchStatusChange={() => setSelectedProfilesEnabled(batchStatusAction === "enable")}
              onTestSelectedRead={testSelectedProfilesRead}
              onBulkDelete={handleBulkDeleteProfiles}
            />
          )}
          <ProfileAccountList
            orderedProfiles={orderedProfiles}
            visibleProfiles={visibleProfiles}
            selectedProfileIds={selectedProfileIds}
            isBatchManaging={isBatchManaging}
            draggedProfileId={draggedProfileId}
            dragTargetProfileId={dragTargetProfileId}
            dragPlacement={dragPlacement}
            draggedProfile={draggedProfile}
            dragVisualState={dragVisualState}
            pendingDeleteId={pendingDeleteId}
            deletingProfileId={deletingProfileId}
            tableRef={profileTableRef}
            profileDragRowStyle={profileDragRowStyle}
            onStartProfilePointerDrag={startProfilePointerDrag}
            onStartProfileRowPointerDrag={startProfileRowPointerDrag}
            onToggleProfileSelection={toggleProfileSelection}
            onToggleProfile={toggleProfile}
            onEditProfile={editProfile}
            onTestRead={testRead}
            onRemoveProfile={removeProfile}
            onCancelDelete={() => {
              setPendingDeleteId("");
              if (profileMessage.startsWith(APP_MESSAGES.confirmDelete)) {
                setProfileMessage("");
              }
            }}
            onConfirmRemoveProfile={confirmRemoveProfile}
          />
        </article>
      </section>

      <ProfilesPageModals
        isAboutOpen={isAboutOpen}
        onCloseAbout={() => setIsAboutOpen(false)}
        bindFlow={{
          shared: {
            updatingCliPlatform,
            cliDeploymentProgress,
            formMessage: renderModalFormMessage(),
            onUpdateCli: updateOfficialCli,
            onOpenAbout: () => setIsAboutOpen(true),
            onCompositionStart: handleModalCompositionStart,
            onCompositionEnd: handleModalCompositionEnd
          },
          officialCli: {
            state: officialCliFlow,
            isCopied: Boolean(officialCliFlow.platform && copiedCommandPlatform === officialCliFlow.platform),
            onCopy: copyActiveOfficialCliCommand,
            onRemarkChange: (remark) => setOfficialCliFlow((current) => ({ ...current, remark })),
            onClose: closeOfficialCliSetup,
            onSave: saveOfficialCliProfile,
            isSaving: isSavingOfficialCliProfile,
            onKeyDown: saveOfficialCliSetupFromEnter
          }
        }}
      />
      {isDeveloperContactOpen && <DeveloperFeedbackModal onClose={() => setIsDeveloperContactOpen(false)} />}
    </div>
  );
}
