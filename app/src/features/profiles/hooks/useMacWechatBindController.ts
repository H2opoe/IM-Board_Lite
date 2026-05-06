import type { Dispatch, SetStateAction, KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  discoverWechatInstances,
  initializeWechatFiles
} from "../../../api/bridgeApi";
import { openMacosPrivacySettings } from "../../../api/appSettingsApi";
import { userErrorMessage } from "../../../utils/errors";
import { profileRemark } from "../../../utils/profiles";
import { upsertProfile } from "../api/profilesApi";
import { buildWechatProfile } from "../model/profileBuilders";
import {
  findMatchingWechatInstance,
  getConfigString,
  latestWechatDbDir
} from "../model/profilePathUtils";
import type { ImProfile, WechatCandidate } from "../model/types";

interface OpenMacWechatSetupOptions {
  preserveInputs?: boolean;
  preferredDbDir?: string;
  preferredInstance?: WechatCandidate | null;
  notice?: string;
  selectLatestDbDir?: boolean;
}

interface UseMacWechatBindControllerParams {
  orderedProfiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
  closeWechatSetup: () => void;
  isDeploymentRequestActive: (requestId: number) => boolean;
  nextDeploymentRequest: () => number;
  preservePersistedProfileState: (original: ImProfile, nextProfile: ImProfile) => ImProfile;
  resetCopiedCommandStatus: () => void;
  runAfterModalPaint: (task: () => void) => void;
  setFormMessage: (message: string) => void;
  shouldHandleModalEnter: (event: ReactKeyboardEvent<HTMLElement>) => boolean;
  isEditingPersistedWechatProfile: boolean;
  selectedWechatCandidate: WechatCandidate | undefined;
  selectedWechatId: string;
  sudoPassword: string;
  wechatCandidates: WechatCandidate[];
  wechatDbDir: string;
  wechatRemark: string;
  wechatSetupProfile: ImProfile | null;
  isInitializingWechat: boolean;
  setIsInitializingWechat: Dispatch<SetStateAction<boolean>>;
  setSelectedWechatId: Dispatch<SetStateAction<string>>;
  setSudoPassword: Dispatch<SetStateAction<string>>;
  setWechatCandidates: Dispatch<SetStateAction<WechatCandidate[]>>;
  setWechatDbDir: Dispatch<SetStateAction<string>>;
  setWechatRemark: Dispatch<SetStateAction<string>>;
  setWechatSetupMode: Dispatch<SetStateAction<"local" | "windows_original_cli">>;
  setWechatSetupProfile: Dispatch<SetStateAction<ImProfile | null>>;
}

export function useMacWechatBindController({
  orderedProfiles,
  onProfilesChange,
  closeWechatSetup,
  isDeploymentRequestActive,
  nextDeploymentRequest,
  preservePersistedProfileState,
  resetCopiedCommandStatus,
  runAfterModalPaint,
  setFormMessage,
  shouldHandleModalEnter,
  isEditingPersistedWechatProfile,
  selectedWechatCandidate,
  selectedWechatId,
  sudoPassword,
  wechatCandidates,
  wechatDbDir,
  wechatRemark,
  wechatSetupProfile,
  isInitializingWechat,
  setIsInitializingWechat,
  setSelectedWechatId,
  setSudoPassword,
  setWechatCandidates,
  setWechatDbDir,
  setWechatRemark,
  setWechatSetupMode,
  setWechatSetupProfile
}: UseMacWechatBindControllerParams) {
  function openWechatSetup(profile: ImProfile, options: OpenMacWechatSetupOptions = {}) {
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

  async function submitMacWechatInitialization() {
    if (isInitializingWechat) return;
    if (!wechatSetupProfile) return;
    const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
    if (!candidate) {
      setFormMessage("未选择微信运行实例。");
      return;
    }
    if (!wechatDbDir.trim()) {
      setFormMessage("请确认聊天数据存储路径。");
      return;
    }
    if (!sudoPassword.trim()) {
      setFormMessage("请输入电脑用户密码。");
      return;
    }

    setIsInitializingWechat(true);

    const selectedCandidate = { ...candidate, dbDir: wechatDbDir.trim() || candidate.dbDir };
    const profile = buildWechatProfile(wechatSetupProfile, selectedCandidate, orderedProfiles.length, wechatRemark.trim());
    try {
      const initializedProfile = await initializeWechatFiles(profile, selectedCandidate, sudoPassword);
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

  async function saveExistingMacWechatProfile() {
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

  function saveMacWechatSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (isInitializingWechat || (!isEditingPersistedWechatProfile && wechatCandidates.length === 0)) return;
    void (isEditingPersistedWechatProfile ? saveExistingMacWechatProfile() : submitMacWechatInitialization());
  }

  function selectWechatCandidate(candidate: WechatCandidate) {
    setSelectedWechatId(candidate.id);
    setWechatDbDir(latestWechatDbDir(candidate) || candidate.dbDir);
  }

  function restoreWechatDbDir() {
    const candidate = wechatCandidates.find((item) => item.id === selectedWechatId);
    setWechatDbDir(latestWechatDbDir(candidate) || candidate?.dbDir || "");
  }

  async function openWechatPrivacyPane(pane: "app_management" | "full_disk_access") {
    try {
      await openMacosPrivacySettings(pane, selectedWechatCandidate?.appPath ?? "", selectedWechatCandidate?.dataDir ?? "");
    } catch (error) {
      setFormMessage(userErrorMessage(error, "打开系统设置失败，请手动前往 系统设置>隐私与安全性。"));
    }
  }

  return {
    openWechatPrivacyPane,
    openWechatSetup,
    refreshWechatCandidates,
    restoreWechatDbDir,
    saveExistingMacWechatProfile,
    saveMacWechatSetupFromEnter,
    selectWechatCandidate,
    submitMacWechatInitialization
  };
}
