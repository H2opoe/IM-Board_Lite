import type { PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import { buildOriginalWechatDeployment } from "../model/profileBuilders";
import { isOriginalWechatCli } from "../model/profilePathUtils";
import type { ImProfile, WechatCandidate } from "../model/types";

interface UseWechatBindDerivedStateParams {
  profiles: ImProfile[];
  wechatCandidates: WechatCandidate[];
  selectedWechatId: string;
  wechatSetupMode: "local" | "windows_original_cli";
  wechatSetupProfile: ImProfile | null;
  wechatDeployment: PlatformDeployment | null;
  wechatDbDir: string;
  wechatVersionStatus: PlatformCliVersionStatus | null;
  isInitializingWechat: boolean;
}

export function useWechatBindDerivedState({
  profiles,
  wechatCandidates,
  selectedWechatId,
  wechatSetupMode,
  wechatSetupProfile,
  wechatDeployment,
  wechatDbDir,
  wechatVersionStatus,
  isInitializingWechat
}: UseWechatBindDerivedStateParams) {
  const selectedWechatCandidate = wechatCandidates.find((item) => item.id === selectedWechatId);
  const selectedWechatUsesOriginalCli = isOriginalWechatCli(selectedWechatCandidate);
  const isWindowsWechatSetupFlow = wechatSetupMode === "windows_original_cli";
  const selectedWechatNeedsResign = !isWindowsWechatSetupFlow && !selectedWechatUsesOriginalCli && selectedWechatCandidate?.needsResign !== false;
  const isEditingPersistedWechatProfile = Boolean(wechatSetupProfile && profiles.some((item) => item.id === wechatSetupProfile.id));
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

  // Windows微信绑定入口必须稳定使用原版CLI模板，不能因热更新失败或候选账号尚未生成而回退到macOS本地签名流程。
  const wechatUsesOfficialCliTemplate = isWindowsWechatSetupFlow || selectedWechatUsesOriginalCli || Boolean(windowsWechatDeployment);
  const windowsWechatSaveDisabled =
    isInitializingWechat || !windowsWechatDeployment || (!isEditingPersistedWechatProfile && wechatCandidates.length === 0);
  const localWechatStepOffset = isEditingPersistedWechatProfile ? 0 : 1;

  return {
    isEditingPersistedWechatProfile,
    localWechatStepOffset,
    selectedWechatCandidate,
    selectedWechatNeedsResign,
    selectedWechatUsesOriginalCli,
    wechatUsesOfficialCliTemplate,
    windowsWechatDeployment,
    windowsWechatSaveDisabled,
    windowsWechatVersionStatus
  };
}
