import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState
} from "react";
import { Settings2 } from "lucide-react";
import {
  cleanupUnusedPlatformCli,
  checkPlatformCliUpdate,
  deployPlatformBridge,
  discoverWechatInstances,
  initializeWechatFiles,
  updatePlatformCli,
  watchPlatformCliDeploymentProgress
} from "../../api/bridgeApi";
import type { PlatformCliDeploymentProgress, PlatformCliVersionStatus, PlatformDeployment } from "../../api/bridgeApi";
import { openMacosPrivacySettings } from "../../api/appSettingsApi";
import {
  createProfileDraft,
  deleteProfile,
  reorderProfiles,
  upsertProfile
} from "../../features/profiles/api/profilesApi";
import {
  testProfileRead,
  verifyOfficialCliAuthorization
} from "../../api/profileReadApi";
import type { ImProfile, Platform, WechatCandidate } from "../../features/profiles/model/types";
import { AboutModal } from "../../components/AboutModal";
import { FloatingNotice, type FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { PagedTextBlock } from "../../components/shared/PagedTextBlock";
import { APP_MESSAGES, OFFICIAL_CLI_MESSAGES, PROFILE_MESSAGES } from "../../constants/messages";
import { userErrorMessage } from "../../utils/errors";
import { logRecoverableError } from "../../utils/logging";
import { profileDisplayName, profileRemark } from "../../utils/profiles";
import { ProfileBindFlowController } from "../../features/profiles/bind-flows/BindFlowController";
import { GenericProfileConfigModal } from "../../features/profiles/bind-flows/GenericProfileConfigModal";
import {
  buildOfficialCliProfileForFlow,
  initialOfficialCliBindState,
  isOfficialCliBindPlatform,
  officialCliBindFlowConfig,
  type OfficialCliBindPlatform,
  type OfficialCliBindState
} from "../../features/profiles/bind-flows/officialCli";
import { ProfileAccountList, ProfileBatchToolbar, ProfilePlatformEntryPanel } from "../../features/profiles/bind-flows/profileList";
import {
  findDuplicateAccountIdentity,
  readBoundAccountIdentity,
  withAccountIdentity
} from "../../features/profiles/model/accountIdentity";
import {
  buildOriginalWechatCandidate,
  buildOriginalWechatDeployment,
  buildWechatProfile
} from "../../features/profiles/model/profileBuilders";
import {
  findMatchingWechatInstance,
  getConfigString,
  isOriginalWechatCli,
  isWindowsRuntime,
  latestWechatDbDir,
  usesWindowsWechatRuntime
} from "../../features/profiles/model/profilePathUtils";
import { useProfileDragOrdering } from "../../features/profiles/hooks/useProfileDragOrdering";

const COMMAND_COPIED_RESET_MS = 1800;
const MODAL_COMPOSITION_ENTER_GUARD_MS = 120;
const BULK_DELETE_ID = "__bulk_delete__";

interface Props {
  profiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
}

interface ProfileReadNotice {
  id: string;
  message: string;
  variant: FloatingNoticeVariant;
}

type WechatSetupMode = "local" | "windows_original_cli";

export function ProfilesPage({ profiles, onProfilesChange }: Props) {
  const orderedProfiles = [...profiles].sort((left, right) => left.sortOrder - right.sortOrder);
  const [editingProfile, setEditingProfile] = useState<ImProfile | null>(null);
  const [wechatSetupProfile, setWechatSetupProfile] = useState<ImProfile | null>(null);
  const [wechatSetupMode, setWechatSetupMode] = useState<WechatSetupMode>("local");
  const [wechatDeployment, setWechatDeployment] = useState<PlatformDeployment | null>(null);
  const [wechatRemark, setWechatRemark] = useState("");
  const [isCheckingWechatVersion, setIsCheckingWechatVersion] = useState(false);
  const [wechatVersionStatus, setWechatVersionStatus] = useState<PlatformCliVersionStatus | null>(null);
  const [wechatCandidates, setWechatCandidates] = useState<WechatCandidate[]>([]);
  const [selectedWechatId, setSelectedWechatId] = useState("");
  const [wechatDbDir, setWechatDbDir] = useState("");
  const [sudoPassword, setSudoPassword] = useState("");
  const [isInitializingWechat, setIsInitializingWechat] = useState(false);
  const [isDeployingWechatBridge, setIsDeployingWechatBridge] = useState(false);
  const [officialCliFlow, setOfficialCliFlow] = useState<OfficialCliBindState>(initialOfficialCliBindState);
  const [updatingCliPlatform, setUpdatingCliPlatform] = useState<Platform | null>(null);
  const [copiedCommandPlatform, setCopiedCommandPlatform] = useState<Platform | null>(null);
  const [formMessage, setFormMessage] = useState("");
  const [profileMessage, setProfileMessage] = useState("");
  const [profileMessageIsError, setProfileMessageIsError] = useState(false);
  const [isAboutOpen, setIsAboutOpen] = useState(false);
  const [pendingDeleteId, setPendingDeleteId] = useState("");
  const [pendingBulkDeleteIds, setPendingBulkDeleteIds] = useState<string[]>([]);
  const [deletingProfileId, setDeletingProfileId] = useState("");
  const [isBatchManaging, setIsBatchManaging] = useState(false);
  const [selectedProfileIds, setSelectedProfileIds] = useState<Set<string>>(() => new Set());
  const [profileReadNotices, setProfileReadNotices] = useState<ProfileReadNotice[]>([]);
  const [cliDeploymentProgress, setCliDeploymentProgress] = useState<PlatformCliDeploymentProgress[]>([]);
  const deploymentRequestRef = useRef(0);
  const copiedCommandResetTimerRef = useRef<number | null>(null);
  const modalInputComposingRef = useRef(false);
  const modalCompositionEndedAtRef = useRef(0);
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
  const selectedProfiles = orderedProfiles.filter((profile) => selectedProfileIds.has(profile.id));
  const selectedProfileCount = selectedProfiles.length;
  const isAllProfilesSelected = orderedProfiles.length > 0 && selectedProfileCount === orderedProfiles.length;
  const enabledProfileCount = orderedProfiles.filter((profile) => profile.enabled).length;
  const disabledProfileCount = orderedProfiles.length - enabledProfileCount;
  const batchStatusAction: "enable" | "pause" = disabledProfileCount > enabledProfileCount ? "enable" : "pause";
  const isBulkDeleteConfirming = pendingBulkDeleteIds.length > 0;
  const profileNoticeVariant: FloatingNoticeVariant = profileMessageIsError
    ? "error"
    : pendingDeleteId || isBulkDeleteConfirming || profileMessage.startsWith("正在") || profileMessage.startsWith(APP_MESSAGES.confirmDelete)
      ? "info"
      : "success";
  const modalFormMessageVariant: FloatingNoticeVariant = formMessage.startsWith("微信已完成重新签名，并已自动重启微信和重新检测。")
    ? "info"
    : "error";

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (!isAboutOpen && !wechatSetupProfile && !officialCliFlow.profile && !editingProfile) return;
      event.preventDefault();

      if (isAboutOpen) {
        setIsAboutOpen(false);
        return;
      }
      if (wechatSetupProfile) {
        closeWechatSetup();
        return;
      }
      if (officialCliFlow.profile) {
        closeOfficialCliSetup();
        return;
      }
      setEditingProfile(null);
    }

    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [editingProfile, isAboutOpen, officialCliFlow.profile, wechatSetupProfile]);

  useEffect(() => {
    return () => clearCopiedCommandResetTimer();
  }, []);

  useEffect(() => {
    if (wechatSetupProfile || officialCliFlow.profile || editingProfile) return;
    resetModalCompositionState();
  }, [editingProfile, officialCliFlow.profile, wechatSetupProfile]);

  useEffect(() => {
    const currentProfileIds = new Set(profiles.map((profile) => profile.id));
    setSelectedProfileIds((currentIds) => {
      const nextIds = new Set([...currentIds].filter((profileId) => currentProfileIds.has(profileId)));
      return nextIds.size === currentIds.size ? currentIds : nextIds;
    });
    setPendingBulkDeleteIds((currentIds) => currentIds.filter((profileId) => currentProfileIds.has(profileId)));
    if (draggedProfileId && !currentProfileIds.has(draggedProfileId)) {
      resetProfileDragState();
    }
  }, [profiles]);

  function clearCopiedCommandResetTimer() {
    if (copiedCommandResetTimerRef.current === null) return;
    window.clearTimeout(copiedCommandResetTimerRef.current);
    copiedCommandResetTimerRef.current = null;
  }

  function resetCopiedCommandStatus() {
    clearCopiedCommandResetTimer();
    setCopiedCommandPlatform(null);
  }

  function resetModalCompositionState() {
    modalInputComposingRef.current = false;
    modalCompositionEndedAtRef.current = 0;
  }

  function scheduleCopiedCommandReset(platform: Platform) {
    clearCopiedCommandResetTimer();
    copiedCommandResetTimerRef.current = window.setTimeout(() => {
      setCopiedCommandPlatform((currentPlatform) => (currentPlatform === platform ? null : currentPlatform));
      copiedCommandResetTimerRef.current = null;
    }, COMMAND_COPIED_RESET_MS);
  }

  function addProfile(platform: Platform) {
    setFormMessage("");
    const draft = createProfileDraft(platform, nextProfileSortOrder());
    if (platform === "wechat") {
      if (isWindowsRuntime()) {
        openWindowsWechatSetup(draft);
      } else {
        openWechatSetup(draft);
      }
      return;
    }
    if (isOfficialCliBindPlatform(platform)) {
      openOfficialCliSetup(platform, draft);
      return;
    }
    setEditingProfile(draft);
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

  function toggleBatchManagement() {
    setIsBatchManaging((isManaging) => {
      const nextIsManaging = !isManaging;
      if (!nextIsManaging) {
        setSelectedProfileIds(new Set());
        setPendingBulkDeleteIds([]);
        resetProfileDragState();
      }
      return nextIsManaging;
    });
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

  function setProfileNotice(message: string, isError = false) {
    setProfileMessageIsError(isError);
    setProfileMessage(message);
  }

  function updateProfileReadNotice(notice: ProfileReadNotice) {
    setProfileReadNotices((currentNotices) => {
      const existingIndex = currentNotices.findIndex((currentNotice) => currentNotice.id === notice.id);
      if (existingIndex < 0) return [...currentNotices, notice];
      return currentNotices.map((currentNotice, index) => (index === existingIndex ? notice : currentNotice));
    });
  }

  function closeProfileReadNotice(noticeId: string) {
    setProfileReadNotices((currentNotices) => currentNotices.filter((notice) => notice.id !== noticeId));
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
    setPendingDeleteId("");
    setPendingBulkDeleteIds(selectedProfiles.map((profile) => profile.id));
    setProfileNotice(PROFILE_MESSAGES.bulkDeletePrompt(selectedProfiles.length));
  }

  function handleBulkDeleteProfiles() {
    if (isBulkDeleteConfirming) {
      void confirmBulkDeleteProfiles();
      return;
    }
    requestBulkDeleteProfiles();
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

  async function saveProfileOrder(nextProfiles: ImProfile[]) {
    try {
      await reorderProfiles(nextProfiles);
      setProfileNotice(PROFILE_MESSAGES.profileOrderSaved);
      await onProfilesChange();
    } catch (error) {
      setProfileNotice(userErrorMessage(error, PROFILE_MESSAGES.profileOrderFailed), true);
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
      if (usesWindowsWechatRuntime(profile) || isWindowsRuntime()) {
        openWindowsWechatSetup(profile);
      } else {
        openWechatSetup(profile);
      }
      return;
    }
    if (isOfficialCliBindPlatform(profile.platform)) {
      openOfficialCliSetup(profile.platform, profile);
      return;
    }
    setEditingProfile(profile);
  }

  function updateOfficialCliFlow(platform: OfficialCliBindPlatform, patch: Partial<OfficialCliBindState>) {
    setOfficialCliFlow((current) => (current.platform === platform ? { ...current, ...patch } : current));
  }

  function openOfficialCliSetup(platform: OfficialCliBindPlatform, profile: ImProfile) {
    const requestId = nextDeploymentRequest();
    const config = officialCliBindFlowConfig(platform);
    setOfficialCliFlow({
      platform,
      profile,
      deployment: null,
      remark: profileRemark(profile),
      cliPath: getConfigString(profile, "cliPath") || config.defaultCliPath,
      isDeployingBridge: true,
      isCheckingVersion: false,
      versionStatus: null
    });
    setCliDeploymentProgress([]);
    resetCopiedCommandStatus();
    setFormMessage("");
    runAfterModalPaint(async () => {
      if (!isDeploymentRequestActive(requestId)) return;
      const unwatch = await watchDeploymentProgressFor(platform, profile.id, requestId);
      try {
        const deployment = await deployPlatformBridge(platform, profile.id);
        if (!isDeploymentRequestActive(requestId)) return;
        updateOfficialCliFlow(platform, {
          deployment,
          cliPath: deployment.cliPath || getConfigString(profile, "cliPath") || config.defaultCliPath
        });
        void refreshOfficialCliVersion(platform, requestId);
      } catch (error) {
        if (!isDeploymentRequestActive(requestId)) return;
        setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.prepareFailed(platform)));
      } finally {
        unwatch();
        if (isDeploymentRequestActive(requestId)) updateOfficialCliFlow(platform, { isDeployingBridge: false });
      }
    });
  }

  async function refreshOfficialCliVersion(platform: OfficialCliBindPlatform, requestId?: number) {
    updateOfficialCliFlow(platform, { isCheckingVersion: true });
    try {
      const status = await checkPlatformCliUpdate(platform);
      if (requestId && !isDeploymentRequestActive(requestId)) return;
      updateOfficialCliFlow(platform, { versionStatus: status });
    } catch (error) {
      if (requestId && !isDeploymentRequestActive(requestId)) return;
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.versionCheckFailed(platform)));
    } finally {
      if (!requestId || isDeploymentRequestActive(requestId)) updateOfficialCliFlow(platform, { isCheckingVersion: false });
    }
  }

  function openWindowsWechatSetup(profile: ImProfile) {
    const requestId = nextDeploymentRequest();
    setWechatSetupProfile(profile);
    setWechatSetupMode("windows_original_cli");
    setWechatDeployment(null);
    setWechatCandidates([]);
    setSelectedWechatId("");
    setWechatDbDir("");
    setWechatRemark(profileRemark(profile));
    setSudoPassword("");
    setWechatVersionStatus(null);
    setCliDeploymentProgress([]);
    setIsDeployingWechatBridge(true);
    resetCopiedCommandStatus();
    setFormMessage("");
    runAfterModalPaint(async () => {
      if (!isDeploymentRequestActive(requestId)) return;
      const unwatch = await watchDeploymentProgressFor("wechat", profile.id, requestId);
      try {
        const deployment = await deployPlatformBridge("wechat", profile.id);
        if (!isDeploymentRequestActive(requestId)) return;
        const candidate = buildOriginalWechatCandidate(profile, deployment);
        setWechatDeployment(deployment);
        setWechatCandidates([candidate]);
        setSelectedWechatId(candidate.id);
        void refreshWechatCliVersion(requestId);
      } catch (error) {
        if (!isDeploymentRequestActive(requestId)) return;
        setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.prepareFailed("wechat")));
      } finally {
        unwatch();
        if (isDeploymentRequestActive(requestId)) setIsDeployingWechatBridge(false);
      }
    });
  }

  async function refreshWechatCliVersion(requestId?: number) {
    setIsCheckingWechatVersion(true);
    try {
      const status = await checkPlatformCliUpdate("wechat");
      if (requestId && !isDeploymentRequestActive(requestId)) return;
      setWechatVersionStatus(status);
    } catch (error) {
      if (requestId && !isDeploymentRequestActive(requestId)) return;
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.versionCheckFailed("wechat")));
    } finally {
      if (!requestId || isDeploymentRequestActive(requestId)) setIsCheckingWechatVersion(false);
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

  function closeOfficialCliSetup() {
    const platform = officialCliFlow.platform;
    const shouldCleanupCli = Boolean(officialCliFlow.deployment);
    cancelDeploymentRequest();
    setOfficialCliFlow(initialOfficialCliBindState);
    setUpdatingCliPlatform(null);
    setCliDeploymentProgress([]);
    resetCopiedCommandStatus();
    setFormMessage("");
    if (shouldCleanupCli && platform) void cleanupPlatformCliIfUnused(platform);
  }

  async function saveOfficialCliProfile() {
    const { platform, profile: setupProfile, deployment } = officialCliFlow;
    if (!platform || !setupProfile) return;
    const config = officialCliBindFlowConfig(platform);
    if (!deployment) {
      setFormMessage(OFFICIAL_CLI_MESSAGES.waitReady(platform));
      return;
    }
    const profile = buildOfficialCliProfileForFlow(
      platform,
      setupProfile,
      orderedProfiles.length,
      officialCliFlow.remark.trim(),
      officialCliFlow.cliPath.trim() || config.defaultCliPath,
      deployment
    );
    try {
      if (config.verifyAuthorizationBeforeSave) {
        await verifyOfficialCliAuthorization(profile);
      }
      let identity = null;
      if (config.identityMode !== "none") {
        identity = await readBoundAccountIdentity(profile);
        if (!identity && config.identityMode === "required") {
          throw new Error(config.missingIdentityMessage ?? "未能读取当前授权身份。");
        }
      }
      if (identity && config.duplicateMessage && (platform === "feishu" || platform === "dingtalk")) {
        const duplicate = await findDuplicateAccountIdentity(platform, identity, profile.id, orderedProfiles);
        if (duplicate) {
          setFormMessage(config.duplicateMessage(duplicate, identity));
          return;
        }
      }
      if (platform === "wecom") {
        // 企业微信官方 CLI 当前只能稳定验证 bot.enc 授权文件，不能返回当前授权企业/用户；这里不做猜测式去重。
      }
      await upsertProfile(preservePersistedProfileState(setupProfile, identity ? withAccountIdentity(profile, identity) : profile));
      closeOfficialCliSetup();
      await onProfilesChange();
    } catch (error) {
      setFormMessage(userErrorMessage(error, config.saveFailedMessage));
    }
  }

  async function updateOfficialCli(platform: Platform) {
    setUpdatingCliPlatform(platform);
    setFormMessage("");
    try {
      const status = await updatePlatformCli(platform);
      if (platform === "wechat") {
        setWechatVersionStatus(status);
        const deployment = wechatSetupProfile ? await deployPlatformBridge("wechat", wechatSetupProfile.id) : null;
        setWechatDeployment((current) => deployment ?? (current ? { ...current, currentVersion: status.currentVersion } : current));
        if (deployment && wechatSetupProfile) {
          const candidate = buildOriginalWechatCandidate(wechatSetupProfile, deployment);
          setWechatCandidates([candidate]);
          setSelectedWechatId(candidate.id);
        }
      } else if (isOfficialCliBindPlatform(platform)) {
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

  function openWechatSetup(
    profile: ImProfile,
    options: {
      preserveInputs?: boolean;
      preferredDbDir?: string;
      preferredInstance?: WechatCandidate | null;
      notice?: string;
      selectLatestDbDir?: boolean;
    } = {}
  ) {
    const requestId = nextDeploymentRequest();
    const preserveInputs = Boolean(options.preserveInputs);
    const preferredDbDir = options.preferredDbDir ?? (options.selectLatestDbDir ? "" : preserveInputs ? wechatDbDir : getConfigString(profile, "dbDir"));
    setWechatSetupProfile(profile);
    setWechatSetupMode("local");
    setWechatCandidates([]);
    setSelectedWechatId("");
    if (!preserveInputs) {
      setWechatRemark(profileRemark(profile));
      setWechatDbDir("");
      setSudoPassword("");
    }
    resetCopiedCommandStatus();
    setFormMessage(options.notice ?? "");
    runAfterModalPaint(async () => {
      if (!isDeploymentRequestActive(requestId)) return;
      try {
        const candidates = await discoverWechatInstances();
        if (!isDeploymentRequestActive(requestId)) return;
        setWechatCandidates(candidates);
        const matchingCandidate = candidates.find(
          (candidate) =>
            candidate.appPath === profile.configJson.appPath ||
            (candidate.bundleId === profile.configJson.bundleId && (!profile.configJson.dbDir || candidate.dbDir === profile.configJson.dbDir))
        );
        const selectedCandidate =
          findMatchingWechatInstance(candidates, options.preferredInstance) ??
          matchingCandidate ??
          candidates.find((candidate) => candidate.dbDir === preferredDbDir || latestWechatDbDir(candidate) === preferredDbDir) ??
          candidates[0];
        const selectedDbDir = options.selectLatestDbDir
          ? latestWechatDbDir(selectedCandidate) || selectedCandidate?.dbDir || ""
          : preferredDbDir || latestWechatDbDir(selectedCandidate) || selectedCandidate?.dbDir || "";
        setSelectedWechatId(selectedCandidate?.id ?? "");
        setWechatDbDir(selectedDbDir);
        setFormMessage(options.notice ?? (candidates.length > 0 ? "" : "未发现运行中的微信实例，请先启动微信。"));
      } catch (error) {
        if (!isDeploymentRequestActive(requestId)) return;
        setFormMessage(userErrorMessage(error, "微信实例发现失败。"));
      }
    });
  }

  async function refreshWechatCandidates() {
    if (!wechatSetupProfile) return;
    await openWechatSetup(wechatSetupProfile, {
      preserveInputs: true,
      preferredDbDir: wechatDbDir,
      preferredInstance: selectedWechatCandidate
    });
  }

  function closeWechatSetup() {
    const shouldCleanupCli = Boolean(wechatDeployment);
    cancelDeploymentRequest();
    setWechatSetupProfile(null);
    setWechatSetupMode("local");
    setWechatDeployment(null);
    setWechatCandidates([]);
    setSelectedWechatId("");
    setWechatDbDir("");
    setSudoPassword("");
    setWechatRemark("");
    setWechatVersionStatus(null);
    setIsDeployingWechatBridge(false);
    setIsCheckingWechatVersion(false);
    setUpdatingCliPlatform(null);
    setCliDeploymentProgress([]);
    resetCopiedCommandStatus();
    setFormMessage("");
    setIsInitializingWechat(false);
    if (shouldCleanupCli) void cleanupPlatformCliIfUnused("wechat");
  }

  async function submitWechatInitialization() {
    if (isInitializingWechat) return;
    if (!wechatSetupProfile) return;
    const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
    if (!candidate) {
      setFormMessage("未选择微信运行实例。");
      return;
    }
    const usesOriginalCli = isOriginalWechatCli(candidate);
    if (!usesOriginalCli && !wechatDbDir.trim()) {
      setFormMessage("请确认聊天数据存储路径。");
      return;
    }
    if (!usesOriginalCli && !sudoPassword.trim()) {
      setFormMessage("请输入电脑用户密码。");
      return;
    }

    setIsInitializingWechat(true);

    const selectedCandidate = { ...candidate, dbDir: wechatDbDir.trim() || candidate.dbDir };
    const profile = buildWechatProfile(wechatSetupProfile, selectedCandidate, orderedProfiles.length, wechatRemark.trim());
    try {
      const initializedProfile = await initializeWechatFiles(profile, selectedCandidate, usesOriginalCli ? "" : sudoPassword);
      await upsertProfile(initializedProfile);
      closeWechatSetup();
      await onProfilesChange();
    } catch (error) {
      setIsInitializingWechat(false);
      const message = userErrorMessage(error, "微信初始化失败。");
      setFormMessage(message);
      const errorCode = (error as { code?: string })?.code;
      if (errorCode === "WECHAT_RESTART_REQUIRED" || errorCode === "WECHAT_KEYS_EMPTY") {
        await openWechatSetup(wechatSetupProfile, {
          preserveInputs: true,
          selectLatestDbDir: true,
          notice: message
        });
      }
    }
  }

  async function saveExistingWechatProfile() {
    if (!wechatSetupProfile) return;
    const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
    const remark = wechatRemark.trim();
    const dbDir = wechatDbDir.trim() || getConfigString(wechatSetupProfile, "dbDir");
    const selectedCandidate = candidate ? { ...candidate, dbDir: dbDir || candidate.dbDir } : null;
    const profile = selectedCandidate
      ? buildWechatProfile(wechatSetupProfile, selectedCandidate, wechatSetupProfile.sortOrder, remark)
      : {
          ...wechatSetupProfile,
          label: "微信",
          configJson: {
            ...wechatSetupProfile.configJson,
            remark,
            ...(dbDir ? { dbDir } : {})
          },
          updatedAt: new Date().toISOString()
        };
    try {
      await upsertProfile(preservePersistedProfileState(wechatSetupProfile, profile));
      closeWechatSetup();
      await onProfilesChange();
    } catch (error) {
      setFormMessage(userErrorMessage(error, "微信账号配置保存失败。"));
    }
  }

  async function saveProfile() {
    if (!editingProfile) return;
    let profileToSave = {
      ...editingProfile,
      updatedAt: new Date().toISOString()
    };
    if (profileToSave.platform !== "wechat" && profileToSave.platform !== "wecom") {
      const deployment = await deployPlatformBridge(profileToSave.platform, profileToSave.id);
      profileToSave = {
        ...profileToSave,
        configJson: {
          ...profileToSave.configJson,
          authType: "cli_session",
          cliPath: deployment.cliPath,
          configDir: deployment.configDir,
          officialSource: deployment.source,
          cliVersion: deployment.currentVersion
        }
      };
    }
    await upsertProfile(profileToSave);
    setEditingProfile(null);
    setFormMessage("");
    await onProfilesChange();
  }

  function updateConfig(key: string, value: string | number) {
    if (!editingProfile) return;
    setEditingProfile({
      ...editingProfile,
      configJson: {
        ...editingProfile.configJson,
        [key]: value
      }
    });
  }

  function configValue(key: string) {
    const value = editingProfile?.configJson[key];
    return typeof value === "string" || typeof value === "number" ? String(value) : "";
  }

  function handleModalCompositionStart() {
    modalInputComposingRef.current = true;
  }

  function handleModalCompositionEnd() {
    modalInputComposingRef.current = false;
    modalCompositionEndedAtRef.current = performance.now();
  }

  function isModalInputMethodEnter(event: ReactKeyboardEvent<HTMLElement>) {
    const nativeEvent = event.nativeEvent as KeyboardEvent & { isComposing?: boolean };
    const legacyCompositionKeyCode = 229;
    const nativeKeyCode = nativeEvent.keyCode || nativeEvent.which;
    const isJustAfterComposition = performance.now() - modalCompositionEndedAtRef.current < MODAL_COMPOSITION_ENTER_GUARD_MS;

    // macOS 中文输入法用回车放弃候选词时，不同输入法对 isComposing 的上报不稳定，这里同时记录弹窗内的 composition 生命周期。
    return nativeEvent.isComposing || modalInputComposingRef.current || nativeKeyCode === legacyCompositionKeyCode || isJustAfterComposition;
  }

  function shouldHandleModalEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key !== "Enter" || event.repeat || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) return false;
    if (isModalInputMethodEnter(event)) return false;
    const target = event.target as HTMLElement | null;
    if (!target) return true;
    const tagName = target.tagName.toLowerCase();
    if (tagName === "textarea" || tagName === "button" || target.closest("button")) return false;
    return true;
  }

  function saveWechatSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (isInitializingWechat || (!isEditingPersistedWechatProfile && wechatCandidates.length === 0)) return;
    void (isEditingPersistedWechatProfile ? saveExistingWechatProfile() : submitWechatInitialization());
  }

  function saveOfficialCliSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (!officialCliFlow.deployment) return;
    void saveOfficialCliProfile();
  }

  function saveGenericProfileFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    void saveProfile();
  }

  const selectedWechatCandidate = wechatCandidates.find((item) => item.id === selectedWechatId);
  const selectedWechatUsesOriginalCli = isOriginalWechatCli(selectedWechatCandidate);
  const isWindowsWechatSetupFlow = wechatSetupMode === "windows_original_cli";
  const selectedWechatNeedsResign = !isWindowsWechatSetupFlow && !selectedWechatUsesOriginalCli && selectedWechatCandidate?.needsResign !== false;
  const isEditingPersistedWechatProfile = Boolean(wechatSetupProfile && isPersistedProfile(wechatSetupProfile));
  const windowsWechatDeployment =
    selectedWechatUsesOriginalCli && wechatDeployment
      ? wechatDeployment
      : selectedWechatUsesOriginalCli && wechatSetupProfile && selectedWechatCandidate
      ? buildOriginalWechatDeployment(wechatSetupProfile, selectedWechatCandidate, wechatDbDir)
      : null;
  const windowsWechatVersionStatus: PlatformCliVersionStatus | null = windowsWechatDeployment
    ? wechatVersionStatus ?? {
        platform: "wechat",
        currentVersion: windowsWechatDeployment.currentVersion || "已配置版本",
        latestVersion: windowsWechatDeployment.currentVersion || "已配置版本",
        updateAvailable: false,
        source: windowsWechatDeployment.source,
        checkedAt: new Date().toISOString()
      }
    : null;
  // Windows 微信绑定入口必须稳定使用原版 CLI 模板，不能因热更新失败或候选账号尚未生成而回退到 macOS 本地签名流程。
  const wechatUsesOfficialCliTemplate = isWindowsWechatSetupFlow || selectedWechatUsesOriginalCli || Boolean(windowsWechatDeployment);
  const windowsWechatSaveDisabled =
    isInitializingWechat || !windowsWechatDeployment || (!isEditingPersistedWechatProfile && wechatCandidates.length === 0);
  const localWechatStepOffset = isEditingPersistedWechatProfile ? 0 : 1;

  async function copyWindowsWechatInitCommand() {
    await copyOfficialCliCommand("wechat", windowsWechatDeployment);
  }

  async function openWechatPrivacyPane(pane: "app_management" | "full_disk_access") {
    try {
      await openMacosPrivacySettings(pane, selectedWechatCandidate?.appPath ?? "", selectedWechatCandidate?.dataDir ?? "");
    } catch (error) {
      setFormMessage(userErrorMessage(error, "打开系统设置失败，请手动前往 系统设置 > 隐私与安全性。"));
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

  function nextDeploymentRequest() {
    deploymentRequestRef.current += 1;
    return deploymentRequestRef.current;
  }

  function cancelDeploymentRequest() {
    deploymentRequestRef.current += 1;
  }

  function isDeploymentRequestActive(requestId: number) {
    return deploymentRequestRef.current === requestId;
  }

  async function cleanupPlatformCliIfUnused(platform: Platform) {
    try {
      await cleanupUnusedPlatformCli(platform);
    } catch (error) {
      logRecoverableError(OFFICIAL_CLI_MESSAGES.cleanupFailed(platform), error);
    }
  }

  function runAfterModalPaint(task: () => void) {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => window.setTimeout(task, 80));
    });
  }

  async function watchDeploymentProgressFor(platform: Platform, profileId: string, requestId: number) {
    return watchPlatformCliDeploymentProgress((progress) => {
      if (!isDeploymentRequestActive(requestId)) return;
      if (progress.platform !== platform || progress.profileId !== profileId) return;
      setCliDeploymentProgress((current) => [...current, progress].slice(-5));
    });
  }

  function renderModalFormMessage() {
    if (!formMessage) return null;
    return (
      <FloatingNotice
        variant={modalFormMessageVariant}
        scope="modal"
        autoCloseMs={false}
        onClose={() => setFormMessage("")}
      >
        <PagedTextBlock
          text={formMessage}
          className="modal-form-message-content"
          controlsClassName="modal-form-message-pager"
          maxLines={4}
          compactCopy
        />
      </FloatingNotice>
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
        <div className="floating-notice-layer page stacked">
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
          {profileReadNotices.map((notice) => (
            <FloatingNotice
              key={notice.id}
              message={notice.message}
              variant={notice.variant}
              withinLayer
              autoCloseMs={notice.variant === "success" ? undefined : false}
              onClose={() => closeProfileReadNotice(notice.id)}
            />
          ))}
        </div>
      )}

      <section className="profile-layout">
        <ProfilePlatformEntryPanel orderedProfiles={orderedProfiles} onAddProfile={addProfile} onOpenAbout={() => setIsAboutOpen(true)} />

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
              bulkDeleteId={BULK_DELETE_ID}
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

      <ProfileBindFlowController
        shared={{
          updatingCliPlatform,
          cliDeploymentProgress,
          formMessage: renderModalFormMessage(),
          onUpdateCli: updateOfficialCli,
          onOpenAbout: () => setIsAboutOpen(true),
          onCompositionStart: handleModalCompositionStart,
          onCompositionEnd: handleModalCompositionEnd
        }}
        wechatOfficial={{
          isOpen: Boolean(wechatSetupProfile && wechatUsesOfficialCliTemplate),
          deployment: windowsWechatDeployment,
          versionStatus: windowsWechatVersionStatus,
          isCheckingVersion: isCheckingWechatVersion,
          isDeployingBridge: isDeployingWechatBridge,
          isCopied: copiedCommandPlatform === "wechat",
          remark: wechatRemark,
          saveDisabled: windowsWechatSaveDisabled,
          onCopy: copyWindowsWechatInitCommand,
          onRemarkChange: setWechatRemark,
          onClose: closeWechatSetup,
          onSave: isEditingPersistedWechatProfile ? saveExistingWechatProfile : submitWechatInitialization,
          onKeyDown: saveWechatSetupFromEnter
        }}
        wechatLocal={{
          isOpen: Boolean(wechatSetupProfile && !wechatUsesOfficialCliTemplate),
          candidates: wechatCandidates,
          selectedCandidate: selectedWechatCandidate,
          selectedWechatId,
          wechatDbDir,
          sudoPassword,
          remark: wechatRemark,
          isInitializingWechat,
          isEditingPersistedProfile: isEditingPersistedWechatProfile,
          selectedWechatNeedsResign,
          stepOffset: localWechatStepOffset,
          onClose: closeWechatSetup,
          onRefreshCandidates: refreshWechatCandidates,
          onSelectCandidate: (candidate) => {
            setSelectedWechatId(candidate.id);
            setWechatDbDir(latestWechatDbDir(candidate) || candidate.dbDir);
          },
          onWechatDbDirChange: setWechatDbDir,
          onRestoreAutoDbDir: () => {
            const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
            setWechatDbDir(latestWechatDbDir(candidate) || candidate?.dbDir || "");
          },
          onSudoPasswordChange: setSudoPassword,
          onRemarkChange: setWechatRemark,
          onOpenPrivacyPane: openWechatPrivacyPane,
          onSave: isEditingPersistedWechatProfile ? saveExistingWechatProfile : submitWechatInitialization,
          onKeyDown: saveWechatSetupFromEnter
        }}
        officialCli={{
          state: officialCliFlow,
          isCopied: Boolean(officialCliFlow.platform && copiedCommandPlatform === officialCliFlow.platform),
          onCopy: copyActiveOfficialCliCommand,
          onRemarkChange: (remark) => setOfficialCliFlow((current) => ({ ...current, remark })),
          onClose: closeOfficialCliSetup,
          onSave: saveOfficialCliProfile,
          onKeyDown: saveOfficialCliSetupFromEnter
        }}
      />

      {editingProfile && (
        <GenericProfileConfigModal
          profile={editingProfile}
          formMessage={renderModalFormMessage()}
          configValue={configValue}
          onLabelChange={(label) => setEditingProfile({ ...editingProfile, label })}
          onConfigChange={updateConfig}
          onClose={() => setEditingProfile(null)}
          onSave={saveProfile}
          onKeyDown={saveGenericProfileFromEnter}
          onCompositionStart={handleModalCompositionStart}
          onCompositionEnd={handleModalCompositionEnd}
        />
      )}

      {isAboutOpen && <AboutModal onClose={() => setIsAboutOpen(false)} />}
    </div>
  );
}
