import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import type { PlatformCliDeploymentProgress, PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import { OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import type { AccountIdentity, ImProfile, Platform } from "../../../types";
import { profileDisplayName } from "../../../utils/profiles";
import { formatAccountIdentity } from "../../../pages/Profiles/accountIdentity";
import { OfficialCliBindModal } from "../../../pages/Profiles/OfficialCliBindModal";
import { buildDingtalkProfile, buildFeishuProfile, buildWecomProfile } from "../../../pages/Profiles/profileBuilders";

export type OfficialCliBindPlatform = Extract<Platform, "wecom" | "feishu" | "dingtalk">;
export type OfficialCliIdentityMode = "none" | "optional" | "required";

export interface OfficialCliBindState {
  platform: OfficialCliBindPlatform | null;
  profile: ImProfile | null;
  deployment: PlatformDeployment | null;
  remark: string;
  cliPath: string;
  isDeployingBridge: boolean;
  isCheckingVersion: boolean;
  versionStatus: PlatformCliVersionStatus | null;
}

export interface OfficialCliBindFlowConfig {
  platform: OfficialCliBindPlatform;
  title: string;
  closeAriaLabel: string;
  defaultCliPath: string;
  openSourceText: string;
  saveFailedMessage: string;
  detailLabel?: string;
  verifyAuthorizationBeforeSave: boolean;
  identityMode: OfficialCliIdentityMode;
  missingIdentityMessage?: string;
  duplicateMessage?: (duplicate: ImProfile, identity: AccountIdentity) => string;
  buildProfile: (profile: ImProfile, sortOrder: number, remark: string, cliPath: string, deployment: PlatformDeployment) => ImProfile;
}

export const initialOfficialCliBindState: OfficialCliBindState = {
  platform: null,
  profile: null,
  deployment: null,
  remark: "",
  cliPath: "",
  isDeployingBridge: false,
  isCheckingVersion: false,
  versionStatus: null
};

const OFFICIAL_CLI_BIND_FLOW_CONFIGS: Record<OfficialCliBindPlatform, OfficialCliBindFlowConfig> = {
  wecom: {
    platform: "wecom",
    title: "企业微信账号绑定",
    closeAriaLabel: "关闭企业微信绑定",
    defaultCliPath: "wecom-cli",
    openSourceText: "本功能使用绑定时热更新的@wecom/cli（MIT）。",
    saveFailedMessage: "企业微信账号配置保存失败。",
    verifyAuthorizationBeforeSave: true,
    identityMode: "none",
    buildProfile: buildWecomProfile
  },
  feishu: {
    platform: "feishu",
    title: "飞书账号绑定",
    closeAriaLabel: "关闭飞书绑定",
    defaultCliPath: "lark-cli",
    openSourceText: "本功能使用绑定时热更新的@larksuite/cli。",
    saveFailedMessage: "飞书账号配置保存失败。",
    detailLabel: "绑定命令",
    verifyAuthorizationBeforeSave: true,
    identityMode: "optional",
    duplicateMessage: (duplicate, identity) =>
      `这个飞书授权已绑定为「${profileDisplayName(duplicate)}」：${formatAccountIdentity(identity)}。请先在绑定命令中重新登录另一个飞书账号。`,
    buildProfile: buildFeishuProfile
  },
  dingtalk: {
    platform: "dingtalk",
    title: "钉钉账号绑定",
    closeAriaLabel: "关闭钉钉绑定",
    defaultCliPath: "dws",
    openSourceText: "本功能使用绑定时热更新的@DingTalk-Real-AI/dingtalk-workspace-cli（Apache-2.0）。",
    saveFailedMessage: "钉钉账号配置保存失败。",
    verifyAuthorizationBeforeSave: false,
    identityMode: "required",
    missingIdentityMessage: "未能读取钉钉当前授权身份，请确认绑定命令最后的get-self返回了用户信息。",
    duplicateMessage: (duplicate, identity) =>
      `这个钉钉授权已绑定为「${profileDisplayName(duplicate)}」：${formatAccountIdentity(identity)}。请先在绑定命令中重新扫码另一个钉钉账号。`,
    buildProfile: buildDingtalkProfile
  }
};

interface OfficialCliBindFlowModalProps {
  state: OfficialCliBindState;
  updatingCliPlatform: Platform | null;
  cliDeploymentProgress: PlatformCliDeploymentProgress[];
  isCopied: boolean;
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

export function isOfficialCliBindPlatform(platform: Platform): platform is OfficialCliBindPlatform {
  return platform === "wecom" || platform === "feishu" || platform === "dingtalk";
}

export function officialCliBindFlowConfig(platform: OfficialCliBindPlatform): OfficialCliBindFlowConfig {
  return OFFICIAL_CLI_BIND_FLOW_CONFIGS[platform];
}

export function buildOfficialCliProfileForFlow(
  platform: OfficialCliBindPlatform,
  profile: ImProfile,
  sortOrder: number,
  remark: string,
  cliPath: string,
  deployment: PlatformDeployment
): ImProfile {
  return officialCliBindFlowConfig(platform).buildProfile(profile, sortOrder, remark, cliPath, deployment);
}

export function OfficialCliBindFlowModal({
  state,
  updatingCliPlatform,
  cliDeploymentProgress,
  isCopied,
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
}: OfficialCliBindFlowModalProps) {
  if (!state.platform || !state.profile) return null;
  const config = officialCliBindFlowConfig(state.platform);
  return (
    <OfficialCliBindModal
      platform={state.platform}
      title={config.title}
      closeAriaLabel={config.closeAriaLabel}
      deployment={state.deployment}
      versionStatus={state.versionStatus}
      isCheckingVersion={state.isCheckingVersion}
      isDeployingBridge={state.isDeployingBridge}
      updatingCliPlatform={updatingCliPlatform}
      cliDeploymentProgress={cliDeploymentProgress}
      onUpdateCli={onUpdateCli}
      placeholder={OFFICIAL_CLI_MESSAGES.placeholder(state.platform)}
      detailLabel={config.detailLabel}
      isCopied={isCopied}
      onCopy={onCopy}
      remark={state.remark}
      onRemarkChange={onRemarkChange}
      openSourceText={config.openSourceText}
      onOpenAbout={onOpenAbout}
      onClose={onClose}
      onSave={onSave}
      saveDisabled={!state.deployment}
      formMessage={formMessage}
      onKeyDown={onKeyDown}
      onCompositionStart={onCompositionStart}
      onCompositionEnd={onCompositionEnd}
    />
  );
}
