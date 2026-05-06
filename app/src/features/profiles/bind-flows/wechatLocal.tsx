import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { AlertTriangle, ExternalLink, FolderSearch, KeyRound, RefreshCw, Save, Search, Settings2, ShieldCheck, X } from "lucide-react";
import { APP_MESSAGES } from "../../../constants/messages";
import type { WechatCandidate } from "../model/types";
import { formatWechatDbTime, wechatDbFolderName } from "../model/profilePathUtils";

interface WechatLocalBindModalProps {
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
  formMessage: ReactNode;
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
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

export function WechatLocalBindModal({
  candidates,
  selectedCandidate,
  selectedWechatId,
  wechatDbDir,
  sudoPassword,
  remark,
  isInitializingWechat,
  isEditingPersistedProfile,
  selectedWechatNeedsResign,
  stepOffset,
  formMessage,
  onClose,
  onRefreshCandidates,
  onSelectCandidate,
  onWechatDbDirChange,
  onRestoreAutoDbDir,
  onSudoPasswordChange,
  onRemarkChange,
  onOpenPrivacyPane,
  onSave,
  onKeyDown,
  onCompositionStart,
  onCompositionEnd
}: WechatLocalBindModalProps) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article
        className="profile-modal wechat-setup-modal local-wechat-modal"
        onKeyDown={onKeyDown}
        onCompositionStart={onCompositionStart}
        onCompositionEnd={onCompositionEnd}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <div>
            <strong>微信账号绑定</strong>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭微信初始化">
            <X size={17} />
          </button>
        </header>
        {formMessage}

        {!isEditingPersistedProfile && <WechatSigningPrepSection candidate={selectedCandidate} onOpenPrivacyPane={onOpenPrivacyPane} />}

        <section className="wechat-setup-section">
          <div className="wechat-step-title">
            <Search size={18} />
            <div>
              <strong>{wechatStepTitle(stepOffset + 1, "选择运行微信实例")}</strong>
              <span>{candidates.length}个候选实例</span>
            </div>
            <button className="secondary-button slim-button" onClick={onRefreshCandidates} disabled={isInitializingWechat}>
              <RefreshCw size={15} />
              重新检测
            </button>
          </div>
          <div className="wechat-candidate-list">
            {candidates.map((candidate) => (
              <button
                key={candidate.id}
                className={selectedWechatId === candidate.id ? "wechat-candidate selected" : "wechat-candidate"}
                onClick={() => onSelectCandidate(candidate)}
              >
                <strong>{candidate.label || "微信"}</strong>
                <span>PID {candidate.pid || "未知"}·{candidate.bundleId || "未知Bundle ID"}</span>
                <small>{candidate.appPath}</small>
              </button>
            ))}
            {candidates.length === 0 && <div className="empty-state">请确认微信正在运行，然后重新检测。</div>}
          </div>
        </section>

        <section className="wechat-setup-section">
          <div className="wechat-step-title">
            <FolderSearch size={18} />
            <div>
              <strong>{wechatStepTitle(stepOffset + 2, "选择账号")}</strong>
              <span>默认选中修改日期最新的聊天数据文件夹，可按实际登录账号切换。</span>
            </div>
          </div>
          <div className="wechat-db-candidate-list">
            {(selectedCandidate?.candidateDbDirs ?? []).map((dbCandidate) => (
              <button
                key={dbCandidate.path}
                className={wechatDbDir === dbCandidate.path ? "wechat-db-candidate selected" : "wechat-db-candidate"}
                onClick={() => onWechatDbDirChange(dbCandidate.path)}
              >
                <strong>{wechatDbFolderName(dbCandidate.path)}</strong>
                <span>
                  修改时间{formatWechatDbTime(dbCandidate.lastModified)}·数据库{dbCandidate.dbCount}个
                  {dbCandidate.valid ? "·结构完整" : "·需确认"}
                </span>
                <small>{dbCandidate.path}</small>
              </button>
            ))}
            {selectedCandidate && (selectedCandidate.candidateDbDirs ?? []).length === 0 && (
              <div className="empty-state">未识别到账号聊天数据文件夹，请确认微信已登录后重新检测。</div>
            )}
          </div>
          <div className="wechat-path-editor">
            <label className="field-row compact">
              <span>db_storage</span>
              <input value={wechatDbDir} onChange={(event) => onWechatDbDirChange(event.target.value)} placeholder="选择或粘贴微信db_storage目录" />
            </label>
            <button className="secondary-button" onClick={onRestoreAutoDbDir} disabled={!selectedWechatId || !selectedCandidate?.dbDir}>
              <ShieldCheck size={16} />
              恢复自动识别
            </button>
          </div>
        </section>

        <section className="wechat-setup-section">
          <div className="wechat-step-title">
            <KeyRound size={18} />
            <div>
              <strong>{wechatStepTitle(stepOffset + 3, "电脑用户密码")}</strong>
              <span>即开机解锁密码；密码只用于本次提取密钥，不会被保存或用于其他用途。</span>
            </div>
          </div>
          <label className="field-row compact sudo-row">
            <span>电脑用户密码</span>
            <input type="password" value={sudoPassword} onChange={(event) => onSudoPasswordChange(event.target.value)} placeholder="输入电脑用户密码" />
          </label>
        </section>

        <section className="wechat-setup-section">
          <div className="wechat-step-title">
            <Settings2 size={18} />
            <div>
              <strong>{wechatStepTitle(stepOffset + 4, "备注信息")}</strong>
              <span>用于区分多个账号用途</span>
            </div>
          </div>
          <label className="field-row compact">
            <span>备注</span>
            <input value={remark} onChange={(event) => onRemarkChange(event.target.value)} />
          </label>
        </section>

        <footer className="modal-actions">
          <button className="secondary-button" onClick={onClose}>
            <X size={16} />
            {APP_MESSAGES.cancel}
          </button>
          <button className="primary-button" onClick={onSave} disabled={isInitializingWechat || (!isEditingPersistedProfile && candidates.length === 0)}>
            {isInitializingWechat ? <RefreshCw size={16} className="spin" /> : <Save size={16} />}
            {isInitializingWechat
              ? selectedWechatNeedsResign
                ? "正在签名"
                : "正在保存"
              : isEditingPersistedProfile
                ? APP_MESSAGES.saveConfig
                : selectedWechatNeedsResign
                  ? "开始签名"
                  : APP_MESSAGES.saveConfig}
          </button>
        </footer>
      </article>
    </div>
  );
}

function WechatSigningPrepSection({
  candidate,
  onOpenPrivacyPane
}: {
  candidate?: WechatCandidate;
  onOpenPrivacyPane: (pane: "app_management" | "full_disk_access") => void;
}) {
  const needsResign = candidate?.needsResign !== false;
  let signingStatusText = "微信已完成签名。请确认微信已重新登录，选择账号后即可保存配置。";
  if (needsResign) {
    signingStatusText = "授权后点击右下角“开始签名”按钮。签名完成后会自动重启微信并重新检测，请在微信窗口重新登录。";
  }
  return (
    <section className="wechat-setup-section">
      <div className="wechat-step-title">
        <ShieldCheck size={18} />
        <div>
          <strong>第一步：授权访问微信文件</strong>
        </div>
      </div>
      <div className={`setup-checklist wechat-signing-guide ${needsResign ? "attention" : "ready"}`}>
        <div className="setup-checkline">
          <ShieldCheck size={15} />
          <span>完全磁盘访问权限用于读取微信本地数据目录；App管理权限用于macOS允许IM-Board修改微信签名。</span>
        </div>
        <div className="wechat-permission-actions">
          <button className="secondary-button slim-button" onClick={() => onOpenPrivacyPane("full_disk_access")}>
            <ExternalLink size={15} />
            完全磁盘访问权限
          </button>
          <button className="secondary-button slim-button" onClick={() => onOpenPrivacyPane("app_management")}>
            <ExternalLink size={15} />
           App管理权限
          </button>
        </div>
        <div className="setup-checkline">
          <RefreshCw size={15} />
          <span>{signingStatusText}</span>
        </div>
        {needsResign && (
          <div className="setup-checkline wechat-restart-warning">
            <AlertTriangle size={15} />
            <span>绑定签名过程中会强制重启微信，请先保存正在编辑或尚未发送的内容。</span>
          </div>
        )}
      </div>
    </section>
  );
}

function wechatStepTitle(step: number, title: string) {
  const stepNames = ["", "第一步", "第二步", "第三步", "第四步", "第五步"];
  return `${stepNames[step] ?? `第${step}步`}：${title}`;
}
