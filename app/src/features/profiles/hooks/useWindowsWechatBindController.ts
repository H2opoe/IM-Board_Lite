import type { Dispatch, SetStateAction, KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  checkPlatformCliUpdate,
  deployPlatformBridge,
  initializeWechatFiles
} from "../../../api/bridgeApi";
import type { PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import { profileRemark } from "../../../utils/profiles";
import { upsertProfile } from "../api/profilesApi";
import {
  buildOriginalWechatCandidate,
  buildWechatProfile
} from "../model/profileBuilders";
import { getConfigString } from "../model/profilePathUtils";
import type { ImProfile, WechatCandidate } from "../model/types";

interface UseWindowsWechatBindControllerParams {
  orderedProfiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
  closeWechatSetup: () => void;
  isDeploymentRequestActive: (requestId: number) => boolean;
  nextDeploymentRequest: () => number;
  preservePersistedProfileState: (original: ImProfile, nextProfile: ImProfile) => ImProfile;
  resetCliDeploymentProgress: () => void;
  resetCopiedCommandStatus: () => void;
  runAfterModalPaint: (task: () => void) => void;
  setFormMessage: (message: string) => void;
  shouldHandleModalEnter: (event: ReactKeyboardEvent<HTMLElement>) => boolean;
  watchDeploymentProgressFor: (platform: "wechat", profileId: string, requestId: number) => Promise<() => void>;
  selectedWechatId: string;
  wechatCandidates: WechatCandidate[];
  wechatDbDir: string;
  wechatRemark: string;
  wechatSetupProfile: ImProfile | null;
  isEditingPersistedWechatProfile: boolean;
  isInitializingWechat: boolean;
  setIsCheckingWechatVersion: Dispatch<SetStateAction<boolean>>;
  setIsDeployingWechatBridge: Dispatch<SetStateAction<boolean>>;
  setIsInitializingWechat: Dispatch<SetStateAction<boolean>>;
  setSelectedWechatId: Dispatch<SetStateAction<string>>;
  setSudoPassword: Dispatch<SetStateAction<string>>;
  setWechatCandidates: Dispatch<SetStateAction<WechatCandidate[]>>;
  setWechatDbDir: Dispatch<SetStateAction<string>>;
  setWechatDeployment: Dispatch<SetStateAction<PlatformDeployment | null>>;
  setWechatRemark: Dispatch<SetStateAction<string>>;
  setWechatSetupMode: Dispatch<SetStateAction<"local" | "windows_original_cli">>;
  setWechatSetupProfile: Dispatch<SetStateAction<ImProfile | null>>;
  setWechatVersionStatus: Dispatch<SetStateAction<PlatformCliVersionStatus | null>>;
}

export function useWindowsWechatBindController({
  orderedProfiles,
  onProfilesChange,
  closeWechatSetup,
  isDeploymentRequestActive,
  nextDeploymentRequest,
  preservePersistedProfileState,
  resetCliDeploymentProgress,
  resetCopiedCommandStatus,
  runAfterModalPaint,
  setFormMessage,
  shouldHandleModalEnter,
  watchDeploymentProgressFor,
  selectedWechatId,
  wechatCandidates,
  wechatDbDir,
  wechatRemark,
  wechatSetupProfile,
  isEditingPersistedWechatProfile,
  isInitializingWechat,
  setIsCheckingWechatVersion,
  setIsDeployingWechatBridge,
  setIsInitializingWechat,
  setSelectedWechatId,
  setSudoPassword,
  setWechatCandidates,
  setWechatDbDir,
  setWechatDeployment,
  setWechatRemark,
  setWechatSetupMode,
  setWechatSetupProfile,
  setWechatVersionStatus
}: UseWindowsWechatBindControllerParams) {
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
    resetCliDeploymentProgress();
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

  async function submitWindowsWechatInitialization() {
    if (isInitializingWechat) return;
    if (!wechatSetupProfile) return;
    const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
    if (!candidate) {
      setFormMessage("未选择微信运行实例。");
      return;
    }

    setIsInitializingWechat(true);

    const selectedCandidate = { ...candidate, dbDir: wechatDbDir.trim() || candidate.dbDir };
    const profile = buildWechatProfile(wechatSetupProfile, selectedCandidate, orderedProfiles.length, wechatRemark.trim());
    try {
      const initializedProfile = await initializeWechatFiles(profile, selectedCandidate, "");
      await upsertProfile(initializedProfile);
      closeWechatSetup();
      await onProfilesChange();
    } catch (error) {
      setIsInitializingWechat(false);
      setFormMessage(userErrorMessage(error, "微信初始化失败。"));
    }
  }

  async function saveExistingWindowsWechatProfile() {
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

  function saveWindowsWechatSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (isInitializingWechat || (!isEditingPersistedWechatProfile && wechatCandidates.length === 0)) return;
    void (isEditingPersistedWechatProfile ? saveExistingWindowsWechatProfile() : submitWindowsWechatInitialization());
  }

  return {
    openWindowsWechatSetup,
    saveExistingWindowsWechatProfile,
    saveWindowsWechatSetupFromEnter,
    submitWindowsWechatInitialization
  };
}
