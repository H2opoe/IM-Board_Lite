import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { Save, X } from "lucide-react";
import { APP_MESSAGES } from "../../../constants/messages";
import type { ImProfile } from "../model/types";

interface GenericProfileConfigModalProps {
  profile: ImProfile;
  formMessage: ReactNode;
  configValue: (key: string) => string;
  onLabelChange: (value: string) => void;
  onConfigChange: (key: string, value: string | number) => void;
  onClose: () => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

export function GenericProfileConfigModal({
  profile,
  formMessage,
  configValue,
  onLabelChange,
  onConfigChange,
  onClose,
  onSave,
  onKeyDown,
  onCompositionStart,
  onCompositionEnd
}: GenericProfileConfigModalProps) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article
        className="profile-modal"
        onKeyDown={onKeyDown}
        onCompositionStart={onCompositionStart}
        onCompositionEnd={onCompositionEnd}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <div>
            <strong>账号配置</strong>
            <span>{profile.id}</span>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭配置">
            <X size={17} />
          </button>
        </header>
        {formMessage}

        <div className="modal-grid">
          <label className="field-row compact">
            <span>账号配置名称</span>
            <input value={profile.label} onChange={(event) => onLabelChange(event.target.value)} />
          </label>
          <label className="field-row compact">
            <span>备注</span>
            <input value={configValue("remark")} onChange={(event) => onConfigChange("remark", event.target.value)} placeholder="例如：工作号 / 私人号" />
          </label>
          <label className="field-row compact">
            <span>configPath</span>
            <input value={configValue("configPath")} onChange={(event) => onConfigChange("configPath", event.target.value)} />
          </label>
          <label className="field-row compact">
            <span>cacheDir</span>
            <input value={configValue("cacheDir")} onChange={(event) => onConfigChange("cacheDir", event.target.value)} />
          </label>
        </div>

        <footer className="modal-actions">
          <button className="secondary-button" onClick={onClose}>
            <X size={16} />
            {APP_MESSAGES.cancel}
          </button>
          <button className="primary-button" onClick={onSave}>
            <Save size={16} />
            保存账号配置
          </button>
        </footer>
      </article>
    </div>
  );
}
