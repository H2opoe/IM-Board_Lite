import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { CheckCircle2, Clipboard, Info, RefreshCw, Save, Settings2, Terminal, X } from "lucide-react";
import type { PlatformCliDeploymentProgress, PlatformCliVersionStatus, PlatformDeployment } from "../../../api/bridgeApi";
import { APP_MESSAGES, OFFICIAL_CLI_MESSAGES } from "../../../constants/messages";
import type { Platform } from "../model/types";
import { commandLineToolName, formatCliVersion } from "../model/profilePathUtils";

interface OfficialCliSharedProps {
  platform: Platform;
  deployment: PlatformDeployment | null;
  versionStatus: PlatformCliVersionStatus | null;
  isCheckingVersion: boolean;
  isDeployingBridge: boolean;
  updatingCliPlatform: Platform | null;
  cliDeploymentProgress: PlatformCliDeploymentProgress[];
  onUpdateCli: (platform: Platform) => void;
}

interface OfficialCliCommandSectionProps extends OfficialCliSharedProps {
  placeholder: string;
  isCopied: boolean;
  onCopy: () => void;
  title?: string;
  description?: string;
  detailLabel?: string;
  showStatus?: boolean;
}

interface OfficialCliBindModalProps extends OfficialCliSharedProps {
  title: string;
  closeAriaLabel: string;
  placeholder: string;
  detailLabel?: string;
  isCopied: boolean;
  onCopy: () => void;
  remark: string;
  onRemarkChange: (value: string) => void;
  openSourceText: string;
  onOpenAbout: () => void;
  onClose: () => void;
  onSave: () => void;
  saveDisabled: boolean;
  formMessage: ReactNode;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

export function OfficialCliBindModal({
  title,
  closeAriaLabel,
  placeholder,
  detailLabel,
  isCopied,
  onCopy,
  remark,
  onRemarkChange,
  openSourceText,
  onOpenAbout,
  onClose,
  onSave,
  saveDisabled,
  formMessage,
  onKeyDown,
  onCompositionStart,
  onCompositionEnd,
  ...sharedProps
}: OfficialCliBindModalProps) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article
        className="profile-modal wechat-setup-modal official-cli-modal"
        onKeyDown={onKeyDown}
        onCompositionStart={onCompositionStart}
        onCompositionEnd={onCompositionEnd}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <div>
            <strong>{title}</strong>
          </div>
          <button className="icon-button" onClick={onClose} aria-label={closeAriaLabel}>
            <X size={17} />
          </button>
        </header>
        {formMessage}

        <OfficialCliCommandSection
          {...sharedProps}
          placeholder={placeholder}
          isCopied={isCopied}
          onCopy={onCopy}
          detailLabel={detailLabel}
        />
        <OfficialCliRemarkSection value={remark} onChange={onRemarkChange} />
        <OfficialCliOpenSourceSection text={openSourceText} onOpenAbout={onOpenAbout} />

        <footer className="modal-actions">
          <button className="secondary-button" onClick={onClose}>
            <X size={16} />
            {APP_MESSAGES.cancel}
          </button>
          <button className="primary-button" onClick={onSave} disabled={saveDisabled}>
            <Save size={16} />
            {APP_MESSAGES.saveConfig}
          </button>
        </footer>
      </article>
    </div>
  );
}

export function OfficialCliCommandSection({
  platform,
  deployment,
  versionStatus,
  isCheckingVersion,
  isDeployingBridge,
  updatingCliPlatform,
  cliDeploymentProgress,
  onUpdateCli,
  placeholder,
  isCopied,
  onCopy,
  title = "账号授权命令",
  description,
  detailLabel = "命令详情",
  showStatus = true
}: OfficialCliCommandSectionProps) {
  return (
    <section className="wechat-setup-section">
      <div className="wechat-step-title">
        <Terminal size={18} />
        <div>
          <strong>{title}</strong>
          <span>{description ?? `在${commandLineToolName(deployment)}运行以下命令`}</span>
        </div>
        <button className="secondary-button slim-button" onClick={onCopy} disabled={!deployment || isDeployingBridge}>
          {isDeployingBridge ? <RefreshCw size={15} className="spin" /> : isCopied ? <CheckCircle2 size={15} /> : <Clipboard size={15} />}
          {isDeployingBridge ? "准备中" : isCopied ? "已复制" : "复制命令"}
        </button>
      </div>
      <div className="command-detail">
        <strong className="command-detail-label">{detailLabel}</strong>
        <div className={platform === "wecom" ? "command-box" : "command-box multiline-command"}>
          <code>{deployment?.command ?? placeholder}</code>
        </div>
      </div>
      {showStatus && (
        <CliStatusBox
          platform={platform}
          deployment={deployment}
          versionStatus={versionStatus}
          isCheckingVersion={isCheckingVersion}
          isDeployingBridge={isDeployingBridge}
          updatingCliPlatform={updatingCliPlatform}
          cliDeploymentProgress={cliDeploymentProgress}
          onUpdateCli={onUpdateCli}
        />
      )}
    </section>
  );
}

export function OfficialCliRemarkSection({
  value,
  onChange
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <section className="wechat-setup-section">
      <div className="wechat-step-title">
        <Settings2 size={18} />
        <div>
          <strong>备注信息</strong>
          <span>用于区分多个账号用途</span>
        </div>
      </div>
      <div className="modal-grid compact-grid">
        <label className="field-row compact">
          <span>备注</span>
          <input value={value} onChange={(event) => onChange(event.target.value)} />
        </label>
      </div>
    </section>
  );
}

export function OfficialCliOpenSourceSection({
  text,
  onOpenAbout
}: {
  text: string;
  onOpenAbout: () => void;
}) {
  return (
    <section className="wechat-setup-section">
      <div className="wechat-step-title">
        <Info size={18} />
        <div>
          <strong>开源信息</strong>
        </div>
      </div>
      <p className="license-note">
        {text}完整许可见：
        <button type="button" onClick={onOpenAbout}>开源许可</button>
      </p>
    </section>
  );
}

function CliStatusBox({
  platform,
  deployment,
  versionStatus,
  isCheckingVersion,
  isDeployingBridge,
  updatingCliPlatform,
  cliDeploymentProgress,
  onUpdateCli
}: OfficialCliSharedProps) {
  const latestProgress = cliDeploymentProgress[cliDeploymentProgress.length - 1];
  const isCurrentPlatformProgress = isDeployingBridge && latestProgress?.platform === platform;
  const isDownloadProgress = isCurrentPlatformProgress && ["installing", "installed", "locating"].includes(latestProgress.phase);
  const progressPercent = latestProgress ? Math.max(6, Math.min(100, Math.round((latestProgress.current / Math.max(latestProgress.total, 1)) * 100))) : 0;
  const currentVersion = versionStatus?.currentVersion || deployment?.currentVersion || latestProgress?.version || "未知";

  if (isCurrentPlatformProgress && !isDownloadProgress) {
    return (
      <div className="cli-version-box cli-checking-box">
        <div>
          <strong>检查更新中</strong>
          <span>{latestProgress?.message || OFFICIAL_CLI_MESSAGES.checkingVersion}</span>
        </div>
        <span className="cli-version-action busy">
          <RefreshCw size={15} className="spin" />
        </span>
      </div>
    );
  }

  if (isDownloadProgress) {
    return (
      <div className="cli-version-box cli-progress-box">
        <div className="cli-progress-content">
          <div className="cli-progress-header">
            <strong>{OFFICIAL_CLI_MESSAGES.preparingStatusTitle}</strong>
            <span>当前CLI版本号 {formatCliVersion(currentVersion)}</span>
          </div>
          <div className="cli-progress-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progressPercent}>
            <span style={{ width: `${progressPercent}%` }} />
          </div>
          <div className="cli-progress-log">
            {cliDeploymentProgress.map((progress, index) => (
              <small key={`${progress.phase}-${index}`}>
                {progress.message}
                {progress.registry ? `·${progress.registry.replace("https://", "")}` : ""}
              </small>
            ))}
          </div>
        </div>
        <span className="cli-version-action busy">
          <RefreshCw size={15} className="spin" />
          准备中
        </span>
      </div>
    );
  }

  const isLatest = Boolean(versionStatus && !versionStatus.updateAvailable);
  return (
    <div className={versionStatus?.updateAvailable ? "cli-version-box update" : "cli-version-box"}>
      <div>
        <strong>{isCheckingVersion ? "检查更新中" : OFFICIAL_CLI_MESSAGES.readyStatusTitle}</strong>
        <span>
          {isCheckingVersion
            ? OFFICIAL_CLI_MESSAGES.checkingVersion
            : versionStatus
              ? versionStatus.updateAvailable
                ? `当前${formatCliVersion(versionStatus.currentVersion)}·最新 ${formatCliVersion(versionStatus.latestVersion)}`
                : `当前${formatCliVersion(versionStatus.currentVersion)}·已是最新版本`
              : deployment
                ? `当前${formatCliVersion(deployment.currentVersion || "未知")}`
                : OFFICIAL_CLI_MESSAGES.pendingReady}
        </span>
      </div>
      {isCheckingVersion ? (
        <RefreshCw size={15} className="spin" />
      ) : isLatest ? (
        <span className="cli-version-action muted">无需更新</span>
      ) : (
        <button
          className="secondary-button slim-button"
          onClick={() => onUpdateCli(platform)}
          disabled={!deployment || updatingCliPlatform !== null}
        >
          {updatingCliPlatform === platform ? <RefreshCw size={15} className="spin" /> : <RefreshCw size={15} />}
          更新
        </button>
      )}
    </div>
  );
}
