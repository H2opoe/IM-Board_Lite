import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import type { PlatformCliDeploymentProgress, PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import type { Platform, WechatCandidate } from "../../../types";
import type { OfficialCliBindState } from "./officialCli";
import { OfficialCliBindFlowModal } from "./officialCli";
import { WechatLocalBindModal } from "./wechatLocal";
import { WechatOfficialBindModal } from "./wechatOfficial";

interface SharedBindModalProps {
  updatingCliPlatform: Platform | null;
  cliDeploymentProgress: PlatformCliDeploymentProgress[];
  formMessage: ReactNode;
  onUpdateCli: (platform: Platform) => void;
  onOpenAbout: () => void;
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

interface WechatOfficialControllerProps {
  isOpen: boolean;
  deployment: PlatformDeployment | null;
  versionStatus: PlatformCliVersionStatus | null;
  isCheckingVersion: boolean;
  isDeployingBridge: boolean;
  isCopied: boolean;
  remark: string;
  saveDisabled: boolean;
  onCopy: () => void;
  onRemarkChange: (value: string) => void;
  onClose: () => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
}

interface WechatLocalControllerProps {
  isOpen: boolean;
  candidates: WechatCandidate[];
  selectedCandidate?: WechatCandidate;
  selectedWechatId: string;
  wechatDbDir: string;
  sudoPassword: string;
  remark: string;
  isInitializingWechat: boolean;
  isEditingPersistedProfile: boolean;
  selectedWechatNeedsResign: boolean;
  stepOffset: number;
  onClose: () => void;
  onRefreshCandidates: () => void;
  onSelectCandidate: (candidate: WechatCandidate) => void;
  onWechatDbDirChange: (value: string) => void;
  onRestoreAutoDbDir: () => void;
  onSudoPasswordChange: (value: string) => void;
  onRemarkChange: (value: string) => void;
  onOpenPrivacyPane: (pane: "app_management" | "full_disk_access") => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
}

interface OfficialCliControllerProps {
  state: OfficialCliBindState;
  isCopied: boolean;
  onCopy: () => void;
  onRemarkChange: (value: string) => void;
  onClose: () => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
}

interface ProfileBindFlowControllerProps {
  shared: SharedBindModalProps;
  wechatOfficial: WechatOfficialControllerProps;
  wechatLocal: WechatLocalControllerProps;
  officialCli: OfficialCliControllerProps;
}

export function ProfileBindFlowController({
  shared,
  wechatOfficial,
  wechatLocal,
  officialCli
}: ProfileBindFlowControllerProps) {
  return (
    <>
      {wechatOfficial.isOpen && (
        <WechatOfficialBindModal
          deployment={wechatOfficial.deployment}
          versionStatus={wechatOfficial.versionStatus}
          isCheckingVersion={wechatOfficial.isCheckingVersion}
          isDeployingBridge={wechatOfficial.isDeployingBridge}
          updatingCliPlatform={shared.updatingCliPlatform}
          cliDeploymentProgress={shared.cliDeploymentProgress}
          onUpdateCli={shared.onUpdateCli}
          isCopied={wechatOfficial.isCopied}
          onCopy={wechatOfficial.onCopy}
          remark={wechatOfficial.remark}
          onRemarkChange={wechatOfficial.onRemarkChange}
          onOpenAbout={shared.onOpenAbout}
          onClose={wechatOfficial.onClose}
          onSave={wechatOfficial.onSave}
          saveDisabled={wechatOfficial.saveDisabled}
          formMessage={shared.formMessage}
          onKeyDown={wechatOfficial.onKeyDown}
          onCompositionStart={shared.onCompositionStart}
          onCompositionEnd={shared.onCompositionEnd}
        />
      )}

      {wechatLocal.isOpen && (
        <WechatLocalBindModal
          candidates={wechatLocal.candidates}
          selectedCandidate={wechatLocal.selectedCandidate}
          selectedWechatId={wechatLocal.selectedWechatId}
          wechatDbDir={wechatLocal.wechatDbDir}
          sudoPassword={wechatLocal.sudoPassword}
          remark={wechatLocal.remark}
          isInitializingWechat={wechatLocal.isInitializingWechat}
          isEditingPersistedProfile={wechatLocal.isEditingPersistedProfile}
          selectedWechatNeedsResign={wechatLocal.selectedWechatNeedsResign}
          stepOffset={wechatLocal.stepOffset}
          formMessage={shared.formMessage}
          onClose={wechatLocal.onClose}
          onRefreshCandidates={wechatLocal.onRefreshCandidates}
          onSelectCandidate={wechatLocal.onSelectCandidate}
          onWechatDbDirChange={wechatLocal.onWechatDbDirChange}
          onRestoreAutoDbDir={wechatLocal.onRestoreAutoDbDir}
          onSudoPasswordChange={wechatLocal.onSudoPasswordChange}
          onRemarkChange={wechatLocal.onRemarkChange}
          onOpenPrivacyPane={wechatLocal.onOpenPrivacyPane}
          onSave={wechatLocal.onSave}
          onKeyDown={wechatLocal.onKeyDown}
          onCompositionStart={shared.onCompositionStart}
          onCompositionEnd={shared.onCompositionEnd}
        />
      )}

      <OfficialCliBindFlowModal
        state={officialCli.state}
        updatingCliPlatform={shared.updatingCliPlatform}
        cliDeploymentProgress={shared.cliDeploymentProgress}
        isCopied={officialCli.isCopied}
        formMessage={shared.formMessage}
        onUpdateCli={shared.onUpdateCli}
        onCopy={officialCli.onCopy}
        onRemarkChange={officialCli.onRemarkChange}
        onOpenAbout={shared.onOpenAbout}
        onClose={officialCli.onClose}
        onSave={officialCli.onSave}
        onKeyDown={officialCli.onKeyDown}
        onCompositionStart={shared.onCompositionStart}
        onCompositionEnd={shared.onCompositionEnd}
      />
    </>
  );
}
