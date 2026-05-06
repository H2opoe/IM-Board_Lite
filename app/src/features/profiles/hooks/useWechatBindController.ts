import { useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import type { PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import type { ImProfile, Platform, WechatCandidate } from "../model/types";
import { useMacWechatBindController } from "./useMacWechatBindController";
import { useWechatBindDerivedState } from "./useWechatBindDerivedState";
import { useWindowsWechatBindController } from "./useWindowsWechatBindController";

interface UseWechatBindControllerParams {
  profiles: ImProfile[];
  orderedProfiles: ImProfile[];
  onProfilesChange: () => Promise<void>;
  cancelDeploymentRequest: () => void;
  cleanupPlatformCliIfUnused: (platform: Platform) => Promise<void>;
  isDeploymentRequestActive: (requestId: number) => boolean;
  nextDeploymentRequest: () => number;
  preservePersistedProfileState: (original: ImProfile, nextProfile: ImProfile) => ImProfile;
  resetCliDeploymentProgress: () => void;
  resetCopiedCommandStatus: () => void;
  runAfterModalPaint: (task: () => void) => void;
  setFormMessage: (message: string) => void;
  setUpdatingCliPlatform: (platform: Platform | null) => void;
  shouldHandleModalEnter: (event: ReactKeyboardEvent<HTMLElement>) => boolean;
  watchDeploymentProgressFor: (platform: Platform, profileId: string, requestId: number) => Promise<() => void>;
}

export function useWechatBindController({
  profiles,
  orderedProfiles,
  onProfilesChange,
  cancelDeploymentRequest,
  cleanupPlatformCliIfUnused,
  isDeploymentRequestActive,
  nextDeploymentRequest,
  preservePersistedProfileState,
  resetCliDeploymentProgress,
  resetCopiedCommandStatus,
  runAfterModalPaint,
  setFormMessage,
  setUpdatingCliPlatform,
  shouldHandleModalEnter,
  watchDeploymentProgressFor
}: UseWechatBindControllerParams) {
  const [wechatSetupProfile, setWechatSetupProfile] = useState<ImProfile | null>(null);
  const [wechatSetupMode, setWechatSetupMode] = useState<"local" | "windows_original_cli">("local");
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

  const derivedState = useWechatBindDerivedState({
    profiles,
    wechatCandidates,
    selectedWechatId,
    wechatSetupMode,
    wechatSetupProfile,
    wechatDeployment,
    wechatDbDir,
    wechatVersionStatus,
    isInitializingWechat
  });

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
    resetCliDeploymentProgress();
    resetCopiedCommandStatus();
    setFormMessage("");
    setIsInitializingWechat(false);
    if (shouldCleanupCli) void cleanupPlatformCliIfUnused("wechat");
  }

  const macWechat = useMacWechatBindController({
    closeWechatSetup,
    isDeploymentRequestActive,
    isEditingPersistedWechatProfile: derivedState.isEditingPersistedWechatProfile,
    isInitializingWechat,
    nextDeploymentRequest,
    onProfilesChange,
    orderedProfiles,
    preservePersistedProfileState,
    resetCopiedCommandStatus,
    runAfterModalPaint,
    selectedWechatCandidate: derivedState.selectedWechatCandidate,
    selectedWechatId,
    setFormMessage,
    setIsInitializingWechat,
    setSelectedWechatId,
    setSudoPassword,
    setWechatCandidates,
    setWechatDbDir,
    setWechatRemark,
    setWechatSetupMode,
    setWechatSetupProfile,
    shouldHandleModalEnter,
    sudoPassword,
    wechatCandidates,
    wechatDbDir,
    wechatRemark,
    wechatSetupProfile
  });

  const windowsWechat = useWindowsWechatBindController({
    closeWechatSetup,
    isDeploymentRequestActive,
    isEditingPersistedWechatProfile: derivedState.isEditingPersistedWechatProfile,
    isInitializingWechat,
    nextDeploymentRequest,
    onProfilesChange,
    orderedProfiles,
    preservePersistedProfileState,
    resetCliDeploymentProgress,
    resetCopiedCommandStatus,
    runAfterModalPaint,
    selectedWechatId,
    setFormMessage,
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
    setWechatVersionStatus,
    shouldHandleModalEnter,
    watchDeploymentProgressFor,
    wechatCandidates,
    wechatDbDir,
    wechatRemark,
    wechatSetupProfile
  });

  return {
    ...derivedState,
    ...macWechat,
    ...windowsWechat,
    wechatSetupProfile,
    wechatDeployment,
    wechatRemark,
    isCheckingWechatVersion,
    wechatVersionStatus,
    wechatCandidates,
    selectedWechatId,
    wechatDbDir,
    sudoPassword,
    isInitializingWechat,
    isDeployingWechatBridge,
    closeWechatSetup,
    setSelectedWechatId,
    setWechatCandidates,
    setWechatDbDir,
    setWechatDeployment,
    setWechatRemark,
    setWechatVersionStatus,
    setSudoPassword
  };
}
