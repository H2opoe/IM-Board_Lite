import { useState } from "react";
import { Check, ChevronDown, Copy, Download, KeyRound, Loader2, RotateCcw, Save, Trash2, X } from "lucide-react";
import { FloatingNotice, FloatingNoticeStack } from "../../components/shared/FloatingNotice";
import { APP_MESSAGES } from "../../constants/messages";
import {
  defaultAnalysisBatchSize,
  defaultAnalysisPrompt,
  defaultSummaryPrompt,
  localDeepseekDisplayName,
  providerDefaults,
  providerOptions
} from "./aiConfigDefaults";
import { formatBytes, useAiSettingsController } from "./useAiSettingsController";

export function SettingsPage() {
  const {
    config,
    localDeepseek,
    downloadProgress,
    notices,
    dismissNotice,
    isSavingConfig,
    isTestingConnection,
    isPathCopied,
    isClearModelArmed,
    isClearModelDone,
    isManagedLocalDeepseek,
    isDownloadingLocalModel,
    isCancellingLocalModelDownload,
    isConfiguringLocalModel,
    isClearingLocalModel,
    isLocalModelActionRunning,
    isLocalDeepseekEnabled,
    isEffectiveAiEnabled,
    localModelActionLabel,
    localModelSize,
    progressTotalBytes,
    progressDownloadedBytes,
    progressPercent,
    save,
    test,
    updateConfigDraft,
    handleLocalModelPrimaryAction,
    clearLocalDeepseek,
    copyLocalModelPath
  } = useAiSettingsController();
  const [isPromptPanelExpanded, setIsPromptPanelExpanded] = useState(false);
  const isAnalysisPromptDefault = config.analysisPrompt === defaultAnalysisPrompt;
  const isSummaryPromptDefault = config.summaryPrompt === defaultSummaryPrompt;
  const isLocalProvider = config.provider.includes("本地");

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
    if (notices.length === 0) return null;
    return (
      <FloatingNoticeStack>
        {[...notices].reverse().map((notice) => (
          <FloatingNotice
            key={notice.id}
            message={notice.message}
            variant={notice.variant}
            withinLayer
            autoCloseMs={notice.variant === "success" ? undefined : false}
            onClose={() => dismissNotice(notice.id)}
          />
        ))}
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
                  <span>{localDeepseek?.installed ? `已保存在本机·${formatBytes(localDeepseek.sizeBytes)}` : `首次使用需下载到本机，约${localModelSize || "4.4GB"}`}</span>
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
                    {isCancellingLocalModelDownload || isConfiguringLocalModel || isSavingConfig ? (
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
                <span>Model ID/接入点ID</span>
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
            <span>启用AI分析</span>
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
        <button className="secondary-button" onClick={test} disabled={isTestingConnection}>
          {isTestingConnection ? <Loader2 size={16} className="spin" /> : <Check size={16} />}
          {isTestingConnection ? "测试中" : "测试连接"}
        </button>
        <button className="primary-button" onClick={save} disabled={isSavingConfig}>
          {isSavingConfig ? <Loader2 size={16} className="spin" /> : <Save size={16} />}
          {APP_MESSAGES.saveConfig}
        </button>
      </footer>
    </div>
  );
}
