import { useRef, useState } from "react";
import { cleanupUnusedPlatformCli, watchPlatformCliDeploymentProgress } from "../../../api/bridgeApi";
import type { PlatformCliDeploymentProgress } from "../../../api/bridgeApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import { logRecoverableError } from "../../../utils/logging";
import type { Platform } from "../model/types";

export function usePlatformCliDeployment() {
  const [cliDeploymentProgress, setCliDeploymentProgress] = useState<PlatformCliDeploymentProgress[]>([]);
  const deploymentRequestRef = useRef(0);

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

  function resetCliDeploymentProgress() {
    setCliDeploymentProgress([]);
  }

  async function cleanupPlatformCliIfUnused(platform: Platform) {
    try {
      await cleanupUnusedPlatformCli(platform);
    } catch (error) {
      logRecoverableError(OFFICIAL_CLI_MESSAGES.cleanupFailed(platform), error);
    }
  }

  async function watchDeploymentProgressFor(platform: Platform, profileId: string, requestId: number) {
    return watchPlatformCliDeploymentProgress((progress) => {
      if (!isDeploymentRequestActive(requestId)) return;
      if (progress.platform !== platform || progress.profileId !== profileId) return;
      setCliDeploymentProgress((current) => [...current, progress].slice(-5));
    });
  }

  return {
    cancelDeploymentRequest,
    cleanupPlatformCliIfUnused,
    cliDeploymentProgress,
    isDeploymentRequestActive,
    nextDeploymentRequest,
    resetCliDeploymentProgress,
    watchDeploymentProgressFor
  };
}
