import { useBindingDraft } from "./useBindingDraft";
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  checkPlatformCliUpdate,
  deployPlatformBridge,
  updatePlatformCli
} from "../../../api/bridgeApi";
import { verifyOfficialCliAuthorization } from "../../../api/profileReadApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import { userErrorMessage } from "../../../utils/errors";
import { profileRemark } from "../../../utils/profiles";
import { upsertProfile } from "../api/profilesApi";
import {
  buildOfficialCliProfileForFlow,
  isOfficialCliBindPlatform,
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
  onDuplicateProfileReplaced: (profileId: string) => void;
  cancelDeploymentRequest: () => void;
  isDeploymentRequestActive: (requestId: number) => boolean;
  nextDeploymentRequest: () => number;
  preservePersistedProfileState: (original: ImProfile, nextProfile: ImProfile) => ImProfile;
  resetCliDeploymentProgress: () => void;
  resetCopiedCommandStatus: () => void;
  markCopiedCommand: (platform: Platform) => void;
  runAfterModalPaint: (task: () => void) => void;
  setFormMessage: (message: string) => void;
  setUpdatingCliPlatform: (platform: Platform | null) => void;
  shouldHandleModalEnter: (event: ReactKeyboardEvent<HTMLElement>) => boolean;
  watchDeploymentProgressFor: (platform: Platform, profileId: string, requestId: number) => Promise<() => void>;
}

export function useOfficialCliBindController({
  orderedProfiles,
  onDuplicateProfileReplaced,
  onProfilesChange,
  cancelDeploymentRequest,
  isDeploymentRequestActive,
  nextDeploymentRequest,
  preservePersistedProfileState,
  resetCliDeploymentProgress,
  resetCopiedCommandStatus,
  markCopiedCommand,
  runAfterModalPaint,
  setFormMessage,
  setUpdatingCliPlatform,
  shouldHandleModalEnter,
  watchDeploymentProgressFor
}: UseOfficialCliBindControllerParams) {
  const [officialCliFlow, setOfficialCliFlow] = useState<OfficialCliBindState>(initialOfficialCliBindState);
  const [isSavingOfficialCliProfile, setIsSavingOfficialCliProfile] = useState(false);
  const bindingDraft = useBindingDraft();
  const sessionRef = useRef(0);
  const savingSessionRef = useRef<number | null>(null);
  const updatingSessionRef = useRef<number | null>(null);
  const copyRequestRef = useRef(0);
  useEffect(() => () => { sessionRef.current += 1; }, []);

  function updateOfficialCliFlow(platform: OfficialCliBindPlatform, patch: Partial<OfficialCliBindState>) {
    setOfficialCliFlow((current) => (current.platform === platform ? { ...current, ...patch } : current));
  }

  function openOfficialCliSetup(platform: OfficialCliBindPlatform, profile: ImProfile) {
    bindingDraft.track(profile.id);
    const session = ++sessionRef.current;
    savingSessionRef.current = null;
    updatingSessionRef.current = null;
    setUpdatingCliPlatform(null);
    setIsSavingOfficialCliProfile(false);
    const requestId = nextDeploymentRequest();
    const isCurrent = () => sessionRef.current === session && isDeploymentRequestActive(requestId);
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
      if (!isCurrent()) return;
      let unwatch = () => {};
      try {
        unwatch = await watchDeploymentProgressFor(platform, profile.id, requestId);
        if (!isCurrent()) return;
        const deployment = await deployPlatformBridge(platform, profile.id);
        if (!isCurrent()) return;
        updateOfficialCliFlow(platform, {
          deployment,
          cliPath: deployment.cliPath || getConfigString(profile, "cliPath") || config.defaultCliPath
        });
        void refreshOfficialCliVersion(platform, requestId);
      } catch (error) {
        if (!isCurrent()) return;
        setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.prepareFailed(platform)));
      } finally {
        unwatch();
        if (isCurrent()) updateOfficialCliFlow(platform, { isDeployingBridge: false });
      }
    });
  }

  async function refreshOfficialCliVersion(platform: OfficialCliBindPlatform, requestId?: number) {
    const session = sessionRef.current;
    const isCurrent = () => sessionRef.current === session && (!requestId || isDeploymentRequestActive(requestId));
    updateOfficialCliFlow(platform, { isCheckingVersion: true });
    try {
      const status = await checkPlatformCliUpdate(platform);
      if (!isCurrent()) return;
      updateOfficialCliFlow(platform, { versionStatus: status });
    } catch (error) {
      if (!isCurrent()) return;
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.versionCheckFailed(platform)));
    } finally {
      if (isCurrent()) updateOfficialCliFlow(platform, { isCheckingVersion: false });
    }
  }

  async function copyOfficialCliCommand() {
    const session = sessionRef.current;
    const { platform, deployment } = officialCliFlow;
    if (!platform || !deployment) return;
    const request = ++copyRequestRef.current;
    const isCurrent = () => sessionRef.current === session && copyRequestRef.current === request;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error(OFFICIAL_CLI_MESSAGES.clipboardUnsupported);
      }
      await navigator.clipboard.writeText(deployment.command);
      if (!isCurrent()) return;
      markCopiedCommand(platform);
      setFormMessage("");
    } catch (error) {
      if (!isCurrent()) return;
      resetCopiedCommandStatus();
      setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.copyFailed));
    }
  }

  async function updateOfficialCli(platform: Platform) {
    const session = sessionRef.current;
    const setupProfile = officialCliFlow.profile;
    if (!isOfficialCliBindPlatform(platform) || officialCliFlow.platform !== platform || !setupProfile) return;
    if (updatingSessionRef.current === session) return;
    const isCurrent = () => sessionRef.current === session;
    updatingSessionRef.current = session;
    setUpdatingCliPlatform(platform);
    setFormMessage("");
    try {
      const status = await updatePlatformCli(platform);
      if (!isCurrent()) return;
      setOfficialCliFlow((current) => {
        if (!isCurrent() || current.platform !== platform || current.profile?.id !== setupProfile.id) return current;
        return { ...current, versionStatus: status };
      });
      const deployment = await deployPlatformBridge(platform, setupProfile.id);
      if (!isCurrent()) return;
      setOfficialCliFlow((current) => {
        if (!isCurrent() || current.platform !== platform || current.profile?.id !== setupProfile.id) return current;
        return {
          ...current,
          versionStatus: status,
          deployment,
          cliPath: deployment.cliPath ?? current.cliPath
        };
      });
    } catch (error) {
      if (isCurrent()) setFormMessage(userErrorMessage(error, OFFICIAL_CLI_MESSAGES.updateFailed(platform)));
    } finally {
      if (isCurrent()) {
        updatingSessionRef.current = null;
        setUpdatingCliPlatform(null);
      }
    }
  }

  function closeOfficialCliSetup() {
    bindingDraft.release();
    sessionRef.current += 1;
    savingSessionRef.current = null;
    updatingSessionRef.current = null;
    cancelDeploymentRequest();
    setOfficialCliFlow(initialOfficialCliBindState);
    setIsSavingOfficialCliProfile(false);
    setUpdatingCliPlatform(null);
    resetCliDeploymentProgress();
    resetCopiedCommandStatus();
    setFormMessage("");
  }

  async function saveOfficialCliProfile() {
    const session = sessionRef.current;
    if (savingSessionRef.current === session) return;
    const isCurrent = () => sessionRef.current === session;
    const { platform, profile: setupProfile, deployment } = officialCliFlow;
    if (!platform || !setupProfile) return;
    const config = officialCliBindFlowConfig(platform);
    if (!deployment) {
      setFormMessage(OFFICIAL_CLI_MESSAGES.waitReady(platform));
      return;
    }
    savingSessionRef.current = session;
    setIsSavingOfficialCliProfile(true);
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
        if (!isCurrent()) return;
      }
      let identity = null;
      if (config.identityMode !== "none") {
        identity = await readBoundAccountIdentity(profile);
        if (!isCurrent()) return;
        if (!identity && config.identityMode === "required") {
          throw new Error(config.missingIdentityMessage ?? "未能读取当前授权身份。");
        }
      }
      let profileToSave = identity ? withAccountIdentity(profile, identity) : profile;
      let persistedStateSource = setupProfile;
      let replacedProfileId = "";
      if (identity) {
        const duplicate = await findDuplicateAccountIdentity(platform, identity, profile.id, orderedProfiles);
        if (!isCurrent()) return;
        if (duplicate) {
          persistedStateSource = duplicate;
          replacedProfileId = duplicate.id;
          profileToSave = {
            ...profileToSave,
            id: duplicate.id,
            enabled: duplicate.enabled,
            status: duplicate.status,
            sortOrder: duplicate.sortOrder,
            createdAt: duplicate.createdAt
          };
        }
      }
      if (!isCurrent()) return;
      await upsertProfile(preservePersistedProfileState(persistedStateSource, profileToSave));
      if (!isCurrent()) {
        // A write already sent to the backend cannot be cancelled by closing the modal.
        await onProfilesChange();
        return;
      }
      if (replacedProfileId) onDuplicateProfileReplaced(replacedProfileId);
      closeOfficialCliSetup();
      await onProfilesChange();
    } catch (error) {
      if (isCurrent()) setFormMessage(userErrorMessage(error, config.saveFailedMessage));
    } finally {
      if (isCurrent()) {
        savingSessionRef.current = null;
        setIsSavingOfficialCliProfile(false);
      }
    }
  }

  function saveOfficialCliSetupFromEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (!shouldHandleModalEnter(event)) return;
    event.preventDefault();
    if (!officialCliFlow.deployment || isSavingOfficialCliProfile) return;
    void saveOfficialCliProfile();
  }

  return {
    closeOfficialCliSetup,
    copyOfficialCliCommand,
    isSavingOfficialCliProfile,
    officialCliFlow,
    openOfficialCliSetup,
    saveOfficialCliProfile,
    saveOfficialCliSetupFromEnter,
    setOfficialCliFlow,
    updateOfficialCli
  };
}
