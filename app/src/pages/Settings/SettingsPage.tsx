import { useCallback, useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Copy, Download, KeyRound, Loader2, RotateCcw, Save, Trash2, X } from "lucide-react";
import {
  cancelLocalDeepseekDownload,
  clearLocalDeepseekModel,
  getAiConfig,
  getLocalDeepseekDownloadProgress,
  getLocalDeepseekStatus,
  installLocalDeepseekModel,
  saveAiConfig,
  testAiConnection,
  watchLocalModelDownloadProgress
} from "../../api/aiApi";
import { FloatingNotice, FloatingNoticeStack, type FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { AI_SETTINGS_MESSAGES, APP_MESSAGES, SETTINGS_SUCCESS_MESSAGES } from "../../constants/messages";
import type { AiConfig, LocalModelDownloadProgress, LocalModelStatus } from "../../features/ai/model/types";
import { userErrorMessage } from "../../utils/errors";
import {
  defaultAnalysisBatchSize,
  defaultAnalysisPrompt,
  defaultSummaryPrompt,
  emptyConfig,
  localDeepseekDefaultBatchSize,
  localDeepseekDisplayName,
  localDeepseekEnablePendingKey,
  normalizeConfig,
  providerDefaults,
  providerOptions
} from "./aiConfigDefaults";

const CLEAR_MODEL_CONFIRM_TIMEOUT_MS = 3_000;
const CLEAR_MODEL_DONE_TIMEOUT_MS = 1_800;

let cachedConfigDraft: AiConfig | null = null;
let cachedHasUnsavedConfig = false;

export function SettingsPage() {
  const [config, setConfig] = useState<AiConfig>(() => cachedConfigDraft ?? emptyConfig);
  const configRef = useRef<AiConfig>(cachedConfigDraft ?? emptyConfig);
  const hasUnsavedConfigRef = useRef(cachedHasUnsavedConfig);
  const copyPathTimer = useRef<number | null>(null);
  const clearModelConfirmTimer = useRef<number | null>(null);
  const clearModelDoneTimer = useRef<number | null>(null);
  const [localDeepseek, setLocalDeepseek] = useState<LocalModelStatus | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<LocalModelDownloadProgress | null>(null);
  const [status, setStatus] = useState("idle");
  const [message, setMessage] = useState("");
  const [isPathCopied, setIsPathCopied] = useState(false);
  const [isClearModelArmed, setIsClearModelArmed] = useState(false);
  const [isClearModelDone, setIsClearModelDone] = useState(false);
  const [isPromptPanelExpanded, setIsPromptPanelExpanded] = useState(false);
  const isAnalysisPromptDefault = config.analysisPrompt === defaultAnalysisPrompt;
  const isSummaryPromptDefault = config.summaryPrompt === defaultSummaryPrompt;
  const isLocalProvider = config.provider.includes("本地");
  const isManagedLocalDeepseek = config.provider === "本地DeepSeek";
  const isDownloadProgressActive = downloadProgress ? isLocalModelDownloadActive(downloadProgress) : false;
  const isDownloadingLocalModel = status === "installingLocalModel" || isDownloadProgressActive;
  const isCancellingLocalModelDownload = status === "cancellingLocalModelDownload";
  const isConfiguringLocalModel = status === "configuringLocalModel";
  const isClearingLocalModel = status === "clearingLocalModel";
  const isLocalModelPrimaryRunning = isDownloadingLocalModel || isCancellingLocalModelDownload || isConfiguringLocalModel || status === "saving";
  const isLocalModelActionRunning = isLocalModelPrimaryRunning || isClearingLocalModel;
  const isLocalDeepseekConfigMatched = Boolean(
    isManagedLocalDeepseek &&
      localDeepseek?.installed &&
      config.model === localDeepseek.model &&
      config.baseUrl.trim().replace(/\/$/, "") === providerDefaults["本地DeepSeek"].baseUrl.replace(/\/$/, "")
  );
  const isEffectiveAiEnabled = config.enabled && (!isManagedLocalDeepseek || isLocalDeepseekConfigMatched);
  const isLocalDeepseekEnabled = Boolean(
    isLocalDeepseekConfigMatched && config.enabled
  );
  const localModelActionLabel = isCancellingLocalModelDownload
    ? "取消中"
    : isDownloadingLocalModel
      ? "取消下载"
      : isConfiguringLocalModel
      ? "配置中"
      : isLocalDeepseekEnabled
        ? "已启用"
        : localDeepseek?.installed
          ? "启用"
          : "下载并启用";
  const localModelSize = localDeepseek ? formatBytes(localDeepseek.sizeBytes || localDeepseek.expectedSizeBytes) : "";
  const progressTotalBytes = downloadProgress?.totalBytes || localDeepseek?.expectedSizeBytes || 0;
  const progressDownloadedBytes = downloadProgress?.downloadedBytes || 0;
  const progressPercent = Math.max(0, Math.min(100, Math.round(downloadProgress?.percent ?? 0)));

  const applySavedConfig = useCallback((nextConfig: AiConfig) => {
    hasUnsavedConfigRef.current = false;
    configRef.current = nextConfig;
    cachedHasUnsavedConfig = false;
    cachedConfigDraft = nextConfig;
    setConfig(nextConfig);
  }, []);

  const updateConfigDraft = useCallback((nextConfig: AiConfig) => {
    hasUnsavedConfigRef.current = true;
    configRef.current = nextConfig;
    cachedHasUnsavedConfig = true;
    cachedConfigDraft = nextConfig;
    setConfig(nextConfig);
  }, []);

  const refreshLocalDeepseekState = useCallback(async () => {
    const [savedConfig, installed, progress] = await Promise.all([
      getAiConfig(),
      getLocalDeepseekStatus().catch(() => null),
      getLocalDeepseekDownloadProgress().catch(() => null)
    ]);
    const normalizedConfig = normalizeConfig(savedConfig);
    setLocalDeepseek(installed);
    if (progress) {
      setDownloadProgress(progress);
      if (isLocalModelDownloadActive(progress)) {
        setStatus("installingLocalModel");
      } else if (progress.status === "done") {
        await completeLocalDeepseekEnable();
        return;
      }
    }
    if (!hasUnsavedConfigRef.current) {
      applySavedConfig(normalizedConfig);
    }
  }, []);

  useEffect(() => {
    void refreshLocalDeepseekState();
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") {
        void refreshLocalDeepseekState();
      }
    };
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [refreshLocalDeepseekState]);

  useEffect(() => {
    configRef.current = config;
  }, [config]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    watchLocalModelDownloadProgress((progress) => {
      setDownloadProgress(progress);
      if (isLocalModelDownloadActive(progress)) {
        setStatus("installingLocalModel");
      } else if (progress.status === "done") {
        completeLocalDeepseekEnable();
      } else if (progress.status === "failed") {
        setStatus("error");
        setMessage(AI_SETTINGS_MESSAGES.downloadOrRuntimeFailed);
        window.localStorage.removeItem(localDeepseekEnablePendingKey);
      } else if (progress.status === "cancelled") {
        setStatus("idle");
        setMessage(AI_SETTINGS_MESSAGES.downloadCancelled);
        window.localStorage.removeItem(localDeepseekEnablePendingKey);
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    return () => {
      if (copyPathTimer.current !== null) {
        window.clearTimeout(copyPathTimer.current);
      }
      if (clearModelConfirmTimer.current !== null) {
        window.clearTimeout(clearModelConfirmTimer.current);
      }
      if (clearModelDoneTimer.current !== null) {
        window.clearTimeout(clearModelDoneTimer.current);
      }
    };
  }, []);

  async function save() {
    setStatus("saving");
    const saved = await saveAiConfig(config);
    applySavedConfig(saved);
    setStatus("saved");
    setMessage(AI_SETTINGS_MESSAGES.configSaved);
  }

  async function test() {
    setStatus("testing");
    setMessage(AI_SETTINGS_MESSAGES.testingConnection);
    try {
      const result = await testAiConnection(config);
      if (result === "downloading") {
        window.localStorage.setItem(localDeepseekEnablePendingKey, "1");
        setStatus("installingLocalModel");
        setMessage(AI_SETTINGS_MESSAGES.downloadStarted);
        return;
      }
      const next = { ...config, testStatus: result };
      updateConfigDraft(next);
      setStatus("tested");
      setMessage(result === "disabled" ? AI_SETTINGS_MESSAGES.aiDisabled : AI_SETTINGS_MESSAGES.connectionPassed);
    } catch (error) {
      setStatus("error");
      setMessage(userErrorMessage(error, AI_SETTINGS_MESSAGES.testFailed));
    }
  }

  async function enableLocalDeepseek() {
    const needsDownload = !localDeepseek?.installed;
    setStatus(needsDownload ? "installingLocalModel" : "configuringLocalModel");
    if (needsDownload) {
      window.localStorage.setItem(localDeepseekEnablePendingKey, "1");
    } else {
      setMessage(AI_SETTINGS_MESSAGES.configuringRuntime);
    }
    setDownloadProgress(
      needsDownload
        ? {
            provider: "本地DeepSeek",
            model: providerDefaults["本地DeepSeek"].model,
            status: "starting",
            downloadedBytes: 0,
            totalBytes: localDeepseek?.expectedSizeBytes || 0,
            percent: 0
          }
        : null
    );
    try {
      const installed = await installLocalDeepseekModel();
      if (!installed) {
        throw new Error(AI_SETTINGS_MESSAGES.localModelStatusFailed);
      }
      setLocalDeepseek(installed);
      if (needsDownload && !installed.installed) {
        return;
      }
      const next = {
        ...config,
        provider: installed.provider,
        apiKey: "",
        baseUrl: providerDefaults[installed.provider]?.baseUrl || "http://127.0.0.1:11434/v1",
        model: installed.model,
        enabled: true,
        testStatus: "untested"
      };
      const saved = await saveAiConfig(next);
      applySavedConfig(saved);
      setStatus("saved");
      setMessage(AI_SETTINGS_MESSAGES.localDeepseekEnabled);
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
    } catch (error) {
      setStatus("error");
      setDownloadProgress((current) =>
        current
          ? {
              ...current,
              status: "failed"
            }
          : current
      );
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
      setMessage(userErrorMessage(error, AI_SETTINGS_MESSAGES.downloadFailed));
    }
  }

  async function cancelLocalModelDownload() {
    setStatus("cancellingLocalModelDownload");
    try {
      const current = await cancelLocalDeepseekDownload();
      setLocalDeepseek(current);
      setDownloadProgress((progress) =>
        progress
          ? {
              ...progress,
              status: "cancelled"
            }
          : null
      );
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
      setStatus("idle");
      setMessage(AI_SETTINGS_MESSAGES.downloadCancelled);
    } catch (error) {
      setStatus("error");
      setMessage(userErrorMessage(error, AI_SETTINGS_MESSAGES.cancelDownloadFailed));
    }
  }

  function handleLocalModelPrimaryAction() {
    if (isDownloadingLocalModel) {
      void cancelLocalModelDownload();
      return;
    }
    void enableLocalDeepseek();
  }

  async function clearLocalDeepseek() {
    if (!isClearModelArmed) {
      if (clearModelConfirmTimer.current !== null) {
        window.clearTimeout(clearModelConfirmTimer.current);
      }
      if (clearModelDoneTimer.current !== null) {
        window.clearTimeout(clearModelDoneTimer.current);
        clearModelDoneTimer.current = null;
      }
      setIsClearModelDone(false);
      setIsClearModelArmed(true);
      setStatus("idle");
      setMessage(AI_SETTINGS_MESSAGES.clearModelConfirm);
      clearModelConfirmTimer.current = window.setTimeout(() => {
        setIsClearModelArmed(false);
        setMessage((current) => (current === AI_SETTINGS_MESSAGES.clearModelConfirm ? "" : current));
        clearModelConfirmTimer.current = null;
      }, CLEAR_MODEL_CONFIRM_TIMEOUT_MS);
      return;
    }

    if (clearModelConfirmTimer.current !== null) {
      window.clearTimeout(clearModelConfirmTimer.current);
      clearModelConfirmTimer.current = null;
    }
    setIsClearModelArmed(false);
    setStatus("clearingLocalModel");
    try {
      const cleared = await clearLocalDeepseekModel();
      setLocalDeepseek(cleared);
      setDownloadProgress(null);
      const saved = await getAiConfig();
      applySavedConfig(normalizeConfig(saved));
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
      setStatus("saved");
      setIsClearModelDone(true);
      clearModelDoneTimer.current = window.setTimeout(() => {
        setIsClearModelDone(false);
        clearModelDoneTimer.current = null;
      }, CLEAR_MODEL_DONE_TIMEOUT_MS);
      setMessage(AI_SETTINGS_MESSAGES.modelCleared);
    } catch (error) {
      setStatus("error");
      setMessage(userErrorMessage(error, AI_SETTINGS_MESSAGES.clearModelFailed));
    }
  }

  async function copyLocalModelPath() {
    if (!localDeepseek?.filePath) return;
    await navigator.clipboard?.writeText(localDeepseek.filePath);
    if (copyPathTimer.current !== null) {
      window.clearTimeout(copyPathTimer.current);
    }
    setIsPathCopied(true);
    copyPathTimer.current = window.setTimeout(() => {
      setIsPathCopied(false);
      copyPathTimer.current = null;
    }, 1800);
    setMessage(AI_SETTINGS_MESSAGES.modelPathCopied);
  }

  async function completeLocalDeepseekEnable() {
    const installed = await getLocalDeepseekStatus().catch(() => null);
    if (!installed) return;
    setLocalDeepseek(installed);
    if (!installed.installed) return;
    // 下载进度事件可能在用户离开页面后完成；只有带有待启用标记时才自动切换配置，避免覆盖用户后续手动选择的云端模型。
    if (window.localStorage.getItem(localDeepseekEnablePendingKey) !== "1") {
      setStatus("saved");
      setMessage(AI_SETTINGS_MESSAGES.modelDownloaded);
      return;
    }
    const currentConfig = normalizeConfig(await getAiConfig().catch(() => configRef.current));
    const next = {
      ...currentConfig,
      provider: installed.provider,
      apiKey: "",
      baseUrl: providerDefaults[installed.provider]?.baseUrl || "http://127.0.0.1:11434/v1",
      model: installed.model,
      analysisBatchSize: localDeepseekDefaultBatchSize,
      enabled: true,
      testStatus: "untested"
    };
    const saved = await saveAiConfig(next);
    applySavedConfig(saved);
    setStatus("saved");
    setMessage(AI_SETTINGS_MESSAGES.downloadedAndEnabled);
    window.localStorage.removeItem(localDeepseekEnablePendingKey);
  }

  function changeProvider(provider: string) {
    const defaults = providerDefaults[provider];
    const knownBaseUrls = Object.values(providerDefaults).map((item) => item.baseUrl);
    updateConfigDraft({
      ...config,
      provider,
      apiKey: provider.includes("本地") ? "" : config.apiKey,
      baseUrl: defaults && knownBaseUrls.includes(config.baseUrl) ? defaults.baseUrl : config.baseUrl,
      model: defaults && (!config.model || Object.values(providerDefaults).some((item) => item.model === config.model)) ? defaults.model : config.model,
      analysisBatchSize: defaultAnalysisBatchSize(provider)
    });
  }

  function renderSettingsMessage() {
    if (!message) return null;
    const variant: FloatingNoticeVariant = status === "error" ? "error" : SETTINGS_SUCCESS_MESSAGES.has(message) ? "success" : "info";
    return (
      <FloatingNoticeStack>
        <FloatingNotice
          message={message}
          variant={variant}
          withinLayer
          autoCloseMs={variant === "success" ? undefined : false}
          onClose={() => setMessage("")}
        />
      </FloatingNoticeStack>
    );
  }

  return (
    <div className="page-surface settings-page">
      <header className="topbar">
        <div>
          <h1>AI配置</h1>
          <span>待办、待回复和热门话题只在AI配置后显示真实分析结果</span>
        </div>
      </header>
      {renderSettingsMessage()}

      <section className="settings-layout">
        <article className="panel settings-panel">
          <header className="panel-header">
            <div>
              <strong>模型连接</strong>
              <span>{isEffectiveAiEnabled ? "已启用" : "未启用"}</span>
            </div>
            <KeyRound size={20} />
          </header>

          <label className="field-row">
            <span>服务商</span>
            <select value={config.provider} onChange={(event) => changeProvider(event.target.value)}>
              {providerOptions.map((provider) => (
                <option key={provider}>{provider}</option>
              ))}
            </select>
          </label>

          {isManagedLocalDeepseek && (
            <div className="field-row local-model-row">
              <span aria-hidden="true" />
              <div className={localDeepseek?.installed ? "local-model-box installed" : "local-model-box"}>
                <div className="local-model-main">
                  <strong>{localDeepseekDisplayName}</strong>
                  <span>{localDeepseek?.installed ? `已保存在本机·${formatBytes(localDeepseek.sizeBytes)}` : `首次使用需下载到本机，约 ${localModelSize || "4.4GB"}`}</span>
                  {(isDownloadingLocalModel || downloadProgress?.status === "done") && (
                    <div className="model-download-progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progressPercent}>
                      <div className="model-download-track">
                        <span style={{ width: `${progressPercent}%` }} />
                      </div>
                      <small>
                        {progressPercent}%·{formatBytes(progressDownloadedBytes)} / {formatBytes(progressTotalBytes)}
                      </small>
                    </div>
                  )}
                  {localDeepseek?.filePath && (
                    <div className="local-model-path">
                      <span>存储路径</span>
                      <code title={localDeepseek.filePath}>{localDeepseek.filePath}</code>
                      <button className={isPathCopied ? "local-model-copy-button copied" : "local-model-copy-button"} onClick={copyLocalModelPath} aria-label="复制本地DeepSeek模型存储路径" title={isPathCopied ? "已复制" : "复制路径"}>
                        {isPathCopied ? <Check size={14} /> : <Copy size={14} />}
                        {isPathCopied && <span>已复制</span>}
                      </button>
                    </div>
                  )}
                </div>
                <div className="local-model-actions">
                  <button className="secondary-button slim-button" onClick={handleLocalModelPrimaryAction} disabled={isCancellingLocalModelDownload || (isLocalModelActionRunning && !isDownloadingLocalModel) || isLocalDeepseekEnabled}>
                    {isCancellingLocalModelDownload || isConfiguringLocalModel || status === "saving" ? (
                      <Loader2 size={15} className="spin" />
                    ) : isDownloadingLocalModel ? (
                      <X size={15} />
                    ) : isLocalDeepseekEnabled ? (
                      <Check size={15} />
                    ) : (
                      <Download size={15} />
                    )}
                    {localModelActionLabel}
                  </button>
                  <button
                    className={isClearModelArmed || isClearingLocalModel ? "danger-button slim-button" : "secondary-button slim-button"}
                    onClick={clearLocalDeepseek}
                    disabled={isLocalModelActionRunning || isClearModelDone}
                  >
                    {isClearingLocalModel ? <Loader2 size={15} className="spin" /> : isClearModelDone ? <Check size={15} /> : <Trash2 size={15} />}
                    {isClearingLocalModel ? "清除中" : isClearModelDone ? "已清除" : isClearModelArmed ? "确认清除" : "清除模型"}
                  </button>
                </div>
              </div>
            </div>
          )}

          {!isManagedLocalDeepseek && (
            <>
              <label className="field-row">
                <span>接口地址</span>
                <input value={config.baseUrl} onChange={(event) => updateConfigDraft({ ...config, baseUrl: event.target.value })} />
              </label>

              <label className="field-row">
                <span>Model ID/接入点 ID</span>
                <input value={config.model} onChange={(event) => updateConfigDraft({ ...config, model: event.target.value })} />
              </label>

              {!isLocalProvider && (
                <label className="field-row">
                  <span>API Key</span>
                  <input
                    type="password"
                    value={config.apiKey}
                    onChange={(event) => updateConfigDraft({ ...config, apiKey: event.target.value })}
                    placeholder="sk-..."
                  />
                </label>
              )}
            </>
          )}

          <label className="field-row">
            <span>消息分批分析</span>
            <div className="batch-size-control">
              <div className="batch-size-input-row">
                <input
                  type="number"
                  step={1}
                  value={config.analysisBatchSize}
                  onChange={(event) =>
                    updateConfigDraft({
                      ...config,
                      analysisBatchSize: Number.parseInt(event.target.value, 10) || 0
                    })
                  }
                />
                <span>{config.analysisBatchSize}条消息/批</span>
              </div>
              <small>
                {isManagedLocalDeepseek
                  ? "本地DeepSeek会按完整上下文自动均衡，单批上限10-30条。"
                  : "系统会按完整上下文自动均衡，单批上限10-300条。"}
              </small>
            </div>
          </label>

          <label className="field-row user-prompt-field">
            <span>用户提示词</span>
            <textarea
              value={config.userPrompt}
              onChange={(event) => updateConfigDraft({ ...config, userPrompt: event.target.value })}
              placeholder="示例：我、我伴侣、我父母的真实姓名/昵称是……"
            />
          </label>

          <label className="toggle-row">
            <input
              type="checkbox"
              checked={config.enabled}
              onChange={(event) => updateConfigDraft({ ...config, enabled: event.target.checked })}
            />
            <span>启用AI分析队列</span>
          </label>

        </article>

        <article className="panel settings-panel settings-prompt-panel">
          <button
            type="button"
            className="panel-header prompt-panel-toggle"
            aria-expanded={isPromptPanelExpanded}
            onClick={() => setIsPromptPanelExpanded((expanded) => !expanded)}
          >
            <div>
              <strong>AI分析提示词</strong>
              <span>AI分析待我回复、待办事项、热门话题和关键词词云</span>
            </div>
            <span className="prompt-panel-icons">
              <ChevronDown size={17} className={isPromptPanelExpanded ? "prompt-panel-chevron expanded" : "prompt-panel-chevron"} />
            </span>
          </button>

          {isPromptPanelExpanded && (
            <div className="prompt-panel-body">
              <div className="prompt-editor">
                <div className="prompt-title">
                  <div>
                    <strong>待我回复&待办事项识别</strong>
                    <span>同步完成后会按此规则识别待我回复和待办事项</span>
                  </div>
                  <button
                    className="secondary-button"
                    onClick={() => updateConfigDraft({ ...config, analysisPrompt: defaultAnalysisPrompt, analysisPromptCustom: false })}
                    disabled={isAnalysisPromptDefault}
                  >
                    <RotateCcw size={16} />
                    恢复默认
                  </button>
                </div>
                <label className="field-row prompt-field">
                  <span>提示词</span>
                  <textarea
                    value={config.analysisPrompt}
                    onChange={(event) => updateConfigDraft({ ...config, analysisPrompt: event.target.value, analysisPromptCustom: true })}
                    placeholder={defaultAnalysisPrompt}
                  />
                </label>
              </div>

              <div className="prompt-editor">
                <div className="prompt-title">
                  <div>
                    <strong>热门话题&关键词识别</strong>
                    <span>同步完成后会按此规则识别热门话题和关键词词云</span>
                  </div>
                  <button
                    className="secondary-button"
                    onClick={() => updateConfigDraft({ ...config, summaryPrompt: defaultSummaryPrompt, summaryPromptCustom: false })}
                    disabled={isSummaryPromptDefault}
                  >
                    <RotateCcw size={16} />
                    恢复默认
                  </button>
                </div>
                <label className="field-row prompt-field">
                  <span>提示词</span>
                  <textarea
                    value={config.summaryPrompt}
                    onChange={(event) => updateConfigDraft({ ...config, summaryPrompt: event.target.value, summaryPromptCustom: true })}
                    placeholder={defaultSummaryPrompt}
                  />
                </label>
              </div>
            </div>
          )}
        </article>
      </section>

      <footer className="settings-sticky-footer">
        <button className="secondary-button" onClick={test} disabled={status === "testing"}>
          {status === "testing" ? <Loader2 size={16} className="spin" /> : <Check size={16} />}
          {status === "testing" ? "测试中" : "测试连接"}
        </button>
        <button className="primary-button" onClick={save} disabled={status === "saving"}>
          {status === "saving" ? <Loader2 size={16} className="spin" /> : <Save size={16} />}
          {APP_MESSAGES.saveConfig}
        </button>
      </footer>
    </div>
  );
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size >= 10 || unit === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unit]}`;
}

function isLocalModelDownloadActive(progress: LocalModelDownloadProgress) {
  return progress.status === "starting" || progress.status === "downloading";
}
