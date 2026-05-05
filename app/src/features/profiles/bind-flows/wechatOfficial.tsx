import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import type { PlatformCliDeploymentProgress, PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import type { Platform } from "../../../types";
import { OfficialCliBindModal } from "../../../pages/Profiles/OfficialCliBindModal";

interface WechatOfficialBindModalProps {
  deployment: PlatformDeployment | null;
  versionStatus: PlatformCliVersionStatus | null;
  isCheckingVersion: boolean;
  isDeployingBridge: boolean;
  updatingCliPlatform: Platform | null;
  cliDeploymentProgress: PlatformCliDeploymentProgress[];
  isCopied: boolean;
  remark: string;
  saveDisabled: boolean;
  formMessage: ReactNode;
  onUpdateCli: (platform: Platform) => void;
  onCopy: () => void;
  onRemarkChange: (value: string) => void;
  onOpenAbout: () => void;
  onClose: () => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

export function WechatOfficialBindModal({
  deployment,
  versionStatus,
  isCheckingVersion,
  isDeployingBridge,
  updatingCliPlatform,
  cliDeploymentProgress,
  isCopied,
  remark,
  saveDisabled,
  formMessage,
  onUpdateCli,
  onCopy,
  onRemarkChange,
  onOpenAbout,
  onClose,
  onSave,
  onKeyDown,
  onCompositionStart,
  onCompositionEnd
}: WechatOfficialBindModalProps) {
  return (
    <OfficialCliBindModal
      platform="wechat"
      title="微信账号绑定"
      closeAriaLabel="关闭微信绑定"
      deployment={deployment}
      versionStatus={versionStatus}
      isCheckingVersion={isCheckingVersion}
      isDeployingBridge={isDeployingBridge}
      updatingCliPlatform={updatingCliPlatform}
      cliDeploymentProgress={cliDeploymentProgress}
      onUpdateCli={onUpdateCli}
      placeholder={OFFICIAL_CLI_MESSAGES.placeholder("wechat")}
      isCopied={isCopied}
      onCopy={onCopy}
      remark={remark}
      onRemarkChange={onRemarkChange}
      openSourceText="本功能使用应用内准备的原版wechat-cli（Apache-2.0）。"
      onOpenAbout={onOpenAbout}
      onClose={onClose}
      onSave={onSave}
      saveDisabled={saveDisabled}
      formMessage={formMessage}
      onKeyDown={onKeyDown}
      onCompositionStart={onCompositionStart}
      onCompositionEnd={onCompositionEnd}
    />
  );
}
