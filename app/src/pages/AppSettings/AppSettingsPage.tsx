import { useEffect, useState } from "react";
import { FileArchive, Info, Loader2, MessageCircle, Save, TimerReset } from "lucide-react";
import { save as showSaveDialog } from "@tauri-apps/plugin-dialog";
import { exportDiagnosticPackage, getAppSettings, saveAppSettings } from "../../api/appSettingsApi";
import { AboutModal } from "../../components/AboutModal";
import { DeveloperFeedbackModal } from "../../components/DeveloperFeedbackModal";
import { FloatingNotice, FloatingNoticeStack } from "../../components/shared/FloatingNotice";
import { APP_MESSAGES } from "../../constants/messages";
import type { AppSettings } from "../../features/app-settings/model/types";
import { userErrorMessage } from "../../utils/errors";

const defaultSettings: AppSettings = {
  cacheClearTime: "00:00"
};

function diagnosticPackageFileName() {
  const timestamp = new Date()
    .toLocaleString("sv-SE", { hour12: false })
    .replace(/[-:]/g, "")
    .replace(" ", "-");
  return `IM-Board-diagnostics-${timestamp}.zip`;
}

export function AppSettingsPage() {
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [status, setStatus] = useState<"idle" | "loading" | "saving" | "exportingDiagnostics" | "saved" | "error">("loading");
  const [message, setMessage] = useState("");
  const [isAboutOpen, setIsAboutOpen] = useState(false);
  const [isDeveloperFeedbackOpen, setIsDeveloperFeedbackOpen] = useState(false);
  const isExportingDiagnostics = status === "exportingDiagnostics";
  const canSave = status !== "loading" && status !== "saving" && !isExportingDiagnostics;

  useEffect(() => {
    getAppSettings()
      .then((next) => {
        setSettings(next);
        setStatus("idle");
      })
      .catch((error) => {
        setStatus("error");
        setMessage(userErrorMessage(error, "设置读取失败。"));
      });
  }, []);

  async function save() {
    if (!canSave) return;
    setStatus("saving");
    try {
      const saved = await saveAppSettings(settings);
      setSettings(saved);
      setStatus("saved");
      setMessage(APP_MESSAGES.settingsSaved);
    } catch (error) {
      setStatus("error");
      setMessage(userErrorMessage(error, "设置保存失败。"));
    }
  }

  async function exportDiagnostics() {
    try {
      const filePath = await showSaveDialog({
        title: "保存诊断包",
        defaultPath: diagnosticPackageFileName(),
        filters: [{ name: "ZIP 压缩包", extensions: ["zip"] }]
      });
      if (!filePath) return;

      setStatus("exportingDiagnostics");
      setMessage("正在导出诊断包，请稍候。");
      const result = await exportDiagnosticPackage(filePath);
      setStatus("saved");
      setMessage(`诊断包已导出到：${result.filePath}`);
    } catch (error) {
      setStatus("error");
      setMessage(userErrorMessage(error, "诊断包导出失败。"));
    }
  }

  return (
    <div className="page-surface settings-page app-settings-page">
      <header className="topbar">
        <div>
          <h1>设置</h1>
          <span>缓存清理与软件信息</span>
        </div>
      </header>
      {message && (
        <FloatingNoticeStack>
          <FloatingNotice
            message={message}
            variant={status === "error" ? "error" : status === "saved" ? "success" : "info"}
            withinLayer
            autoCloseMs={status === "saved" ? undefined : false}
            onClose={() => setMessage("")}
          />
        </FloatingNoticeStack>
      )}

      <section className="settings-layout app-settings-grid">
        <article className="panel settings-panel app-settings-card">
          <header className="panel-header">
            <div>
              <strong>每日清理时间</strong>
            </div>
            <TimerReset size={20} />
          </header>

          <p className="app-settings-copy">
            到达设置的清理时间会清空看板数据，但未完成的待我回复、待办事项会继续保留。
          </p>

          <div className="app-settings-inline">
            <label className="field-row">
              <span>清理时间</span>
              <input
                type="time"
                value={settings.cacheClearTime}
                onChange={(event) => setSettings({ ...settings, cacheClearTime: event.target.value })}
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  event.preventDefault();
                  void save();
                }}
                disabled={status === "loading"}
              />
            </label>
          </div>

        </article>

        <article className="panel settings-panel app-settings-card">
          <header className="panel-header">
            <div>
              <strong>关于软件</strong>
            </div>
            <Info size={20} />
          </header>

          <p className="app-settings-copy">查看产品作者信息、联系方式，以及内置或调用的第三方开源许可说明。</p>

          <div className="form-actions app-settings-actions-left">
            <button className="secondary-button" onClick={() => setIsAboutOpen(true)}>
              <Info size={16} />
              打开关于窗口
            </button>
          </div>
        </article>

        <article className="panel settings-panel app-settings-card">
          <header className="panel-header">
            <div>
              <strong>诊断工具</strong>
            </div>
            <FileArchive size={20} />
          </header>

          <p className="app-settings-copy">
            导出脱敏诊断包，便于排查同步失败、AI返回异常、账号授权和运行时问题。诊断包不会包含聊天内容、API Key等任何敏感数据。
          </p>

          <div className="form-actions app-settings-actions-left">
            <button className="secondary-button" onClick={exportDiagnostics} disabled={isExportingDiagnostics}>
              {isExportingDiagnostics ? <Loader2 size={16} className="spin" /> : <FileArchive size={16} />}
              {isExportingDiagnostics ? "正在导出" : "导出诊断包"}
            </button>
            <button className="secondary-button" onClick={() => setIsDeveloperFeedbackOpen(true)}>
              <MessageCircle size={16} />
              反馈给开发者
            </button>
          </div>
        </article>
      </section>

      <footer className="settings-sticky-footer">
        <button className="primary-button" onClick={save} disabled={!canSave}>
          {status === "saving" ? <Loader2 size={16} className="spin" /> : <Save size={16} />}
          {APP_MESSAGES.saveSettings}
        </button>
      </footer>

      {isAboutOpen && <AboutModal onClose={() => setIsAboutOpen(false)} />}
      {isDeveloperFeedbackOpen && <DeveloperFeedbackModal onClose={() => setIsDeveloperFeedbackOpen(false)} />}
    </div>
  );
}
