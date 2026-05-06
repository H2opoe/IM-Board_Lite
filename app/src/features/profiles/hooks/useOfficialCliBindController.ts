import { useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  checkPlatformCliUpdate,
  deployPlatformBridge
} from "../../../api/bridgeApi";
import { verifyOfficialCliAuthorization } from "../../../api/profileReadApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import { profileRemark } from "../../../utils/profiles";
import { upsertProfile } from "../api/profilesApi";
import {
  buildOfficialCliProfileForFlow,
  initialOfficialCliBindState,
  officialCliBindFlowConfig,
  type OfficialCliBindPlatform,
  type OfficialCliBindState
} from "../bind-flows/officialCli";
import {
  findDuplicateAccountIdentity,
  readBoundAccountIdentity,
  withAccountIdentity
} from "../model/accountIdentity";
import { getConfigString } from "../model/profilePathUtils";
import type { ImProfile, Platform } from "../model/types";

interface UseOfficialCliBindControllerParams {
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

export function useOfficialCliBindController({
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
}: UseOfficialCliBindControllerParams) {
  const [officialCliFlow, setOfficialCliFlow] = useState<OfficialCliBindState>(initialOfficialCliBindState);

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
    resetCliDeploymentProgress();
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

  function closeOfficialCliSetup() {
    const platform = officialCliFlow.platform;
    const shouldCleanupCli = Boolean(officialCliFlow.deployment);
    cancelDeploymentRequest();
    setOfficialCliFlow(initialOfficialCliBindState);
    setUpdatingCliPlatform(null);
    resetCliDeploymentProgress();
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
      await upsertProfile(preservePersistedProfileState(setupProfile, identity ? withAccountIdentity(profile, identity) : profile));
      closeOfficialCliSetup();
      await onProfilesChange();
    } catch (error) {
      setFormMessage(userErrorMessage(error, config.saveFailedMessage));
    }
  }

  function saveOfficialCliSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (!officialCliFlow.deployment) return;
    void saveOfficialCliProfile();
  }

  return {
    closeOfficialCliSetup,
    officialCliFlow,
    openOfficialCliSetup,
    saveOfficialCliProfile,
    saveOfficialCliSetupFromEnter,
    setOfficialCliFlow,
    updateOfficialCliFlow
  };
}
