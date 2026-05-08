import { useCallback, useEffect, useRef, useState } from "react";
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
import { AI_SETTINGS_MESSAGES } from "../../constants/messages";
import type { FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { upsertNoticeToStackTop } from "../../components/shared/noticeStack";
import type { AiConfig, LocalModelDownloadProgress, LocalModelStatus } from "../../features/ai/model/types";
import { userErrorMessage } from "../../utils/errors";
import {
  emptyConfig,
  localDeepseekDefaultBatchSize,
  localDeepseekEnablePendingKey,
  normalizeConfig,
  providerDefaults
} from "./aiConfigDefaults";

const CLEAR_MODEL_CONFIRM_TIMEOUT_MS = 3_000;
const CLEAR_MODEL_DONE_TIMEOUT_MS = 1_800;
const TEST_CONNECTION_NOTICE_ID = "ai-settings-test-connection";
const LOCAL_MODEL_DOWNLOAD_CANCELLED_NOTICE_ID = "ai-settings-local-model-download-cancelled";

let cachedConfigDraft: AiConfig | null = null;
let cachedHasUnsavedConfig = false;

export interface SettingsNotice {
  id: string;
  message: string;
  variant: FloatingNoticeVariant;
}

export function useAiSettingsController() {
  const [config, setConfig] = useState<AiConfig>(() => cachedConfigDraft ?? emptyConfig);
  const configRef = useRef<AiConfig>(cachedConfigDraft ?? emptyConfig);
  const hasUnsavedConfigRef = useRef(cachedHasUnsavedConfig);
  const noticeIdRef = useRef(0);
  const clearModelConfirmNoticeId = useRef<string | null>(null);
  const copyPathTimer = useRef<number | null>(null);
  const clearModelConfirmTimer = useRef<number | null>(null);
  const clearModelDoneTimer = useRef<number | null>(null);
  const [localDeepseek, setLocalDeepseek] = useState<LocalModelStatus | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<LocalModelDownloadProgress | null>(null);
  const [status, setStatus] = useState("idle");
  const [notices, setNotices] = useState<SettingsNotice[]>([]);
  const [isSavingConfig, setIsSavingConfig] = useState(false);
  const [isTestingConnection, setIsTestingConnection] = useState(false);
  const [isPathCopied, setIsPathCopied] = useState(false);
  const [isClearModelArmed, setIsClearModelArmed] = useState(false);
  const [isClearModelDone, setIsClearModelDone] = useState(false);

  const isDownloadProgressActive = downloadProgress ? isLocalModelDownloadActive(downloadProgress) : false;
  const isDownloadingLocalModel = status === "installingLocalModel" || isDownloadProgressActive;
  const isCancellingLocalModelDownload = status === "cancellingLocalModelDownload";
  const isConfiguringLocalModel = status === "configuringLocalModel";
  const isClearingLocalModel = status === "clearingLocalModel";
  const isLocalModelPrimaryRunning = isDownloadingLocalModel || isCancellingLocalModelDownload || isConfiguringLocalModel || isSavingConfig;
  const isLocalModelActionRunning = isLocalModelPrimaryRunning || isClearingLocalModel;
  const isManagedLocalDeepseek = config.provider === "本地DeepSeek";
  const isLocalDeepseekConfigMatched = Boolean(
    isManagedLocalDeepseek &&
      localDeepseek?.installed &&
      config.model === localDeepseek.model &&
      config.baseUrl.trim().replace(/\/$/, "") === providerDefaults["本地DeepSeek"].baseUrl.replace(/\/$/, "")
  );
  const isEffectiveAiEnabled = config.enabled && (!isManagedLocalDeepseek || isLocalDeepseekConfigMatched);
  const isLocalDeepseekEnabled = Boolean(isLocalDeepseekConfigMatched && config.enabled);
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

  const pushNotice = useCallback((message: string, variant: FloatingNoticeVariant = "info") => {
    const id = `ai-settings-notice-${Date.now()}-${noticeIdRef.current++}`;
    setNotices((current) => [...current, { id, message, variant }]);
    return id;
  }, []);

  const setNotice = useCallback((id: string, message: string, variant: FloatingNoticeVariant = "info") => {
    setNotices((current) => {
      return upsertNoticeToStackTop(current, { id, message, variant });
    });
  }, []);

  const dismissNotice = useCallback((id: string) => {
    setNotices((current) => current.filter((notice) => notice.id !== id));
    if (clearModelConfirmNoticeId.current === id) {
      clearModelConfirmNoticeId.current = null;
    }
  }, []);

  const notifyLocalModelDownloadCancelled = useCallback(() => {
    // 取消下载会同时收到命令返回和后台进度事件，统一复用同一个提示，避免重复弹出相同文案。
    setNotice(LOCAL_MODEL_DOWNLOAD_CANCELLED_NOTICE_ID, AI_SETTINGS_MESSAGES.downloadCancelled, "success");
  }, [setNotice]);

  const updateConfigDraft = useCallback((nextConfig: AiConfig) => {
    hasUnsavedConfigRef.current = true;
    configRef.current = nextConfig;
    cachedHasUnsavedConfig = true;
    cachedConfigDraft = nextConfig;
    setConfig(nextConfig);
  }, []);

  const completeLocalDeepseekEnable = useCallback(async () => {
    const installed = await getLocalDeepseekStatus().catch(() => null);
    if (!installed) return;
    setLocalDeepseek(installed);
    if (!installed.installed) return;
    // 下载进度事件可能在用户离开页面后完成；只有带有待启用标记时才自动切换配置，避免覆盖用户后续手动选择的云端模型。
    if (window.localStorage.getItem(localDeepseekEnablePendingKey) !== "1") {
      setStatus("saved");
      pushNotice(AI_SETTINGS_MESSAGES.modelDownloaded, "success");
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
    pushNotice(AI_SETTINGS_MESSAGES.downloadedAndEnabled, "success");
    window.localStorage.removeItem(localDeepseekEnablePendingKey);
  }, [applySavedConfig, pushNotice]);

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
  }, [applySavedConfig, completeLocalDeepseekEnable]);

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
        void completeLocalDeepseekEnable();
      } else if (progress.status === "failed") {
        setStatus("error");
        pushNotice(AI_SETTINGS_MESSAGES.downloadOrRuntimeFailed, "error");
        window.localStorage.removeItem(localDeepseekEnablePendingKey);
      } else if (progress.status === "cancelled") {
        setStatus("idle");
        notifyLocalModelDownloadCancelled();
        window.localStorage.removeItem(localDeepseekEnablePendingKey);
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => unlisten?.();
  }, [completeLocalDeepseekEnable, notifyLocalModelDownloadCancelled, pushNotice]);

  useEffect(() => {
    return () => {
      if (copyPathTimer.current !== null) window.clearTimeout(copyPathTimer.current);
      if (clearModelConfirmTimer.current !== null) window.clearTimeout(clearModelConfirmTimer.current);
      if (clearModelDoneTimer.current !== null) window.clearTimeout(clearModelDoneTimer.current);
    };
  }, []);

  async function save() {
    if (isSavingConfig) return;
    setIsSavingConfig(true);
    setStatus("saving");
    try {
      const saved = await saveAiConfig(config);
      applySavedConfig(saved);
      setStatus("saved");
      pushNotice(AI_SETTINGS_MESSAGES.configSaved, "success");
    } catch (error) {
      setStatus("error");
      pushNotice(userErrorMessage(error, AI_SETTINGS_MESSAGES.saveFailed), "error");
    } finally {
      setIsSavingConfig(false);
    }
  }

  async function test() {
    if (isTestingConnection) return;
    setIsTestingConnection(true);
    setStatus("testing");
    setNotice(TEST_CONNECTION_NOTICE_ID, AI_SETTINGS_MESSAGES.testingConnection);
    try {
      const result = await testAiConnection(config);
      if (result === "downloading") {
        window.localStorage.setItem(localDeepseekEnablePendingKey, "1");
        setStatus("installingLocalModel");
        setNotice(TEST_CONNECTION_NOTICE_ID, AI_SETTINGS_MESSAGES.downloadStarted, "success");
        return;
      }
      updateConfigDraft({ ...config, testStatus: result });
      setStatus("tested");
      setNotice(
        TEST_CONNECTION_NOTICE_ID,
        result === "disabled" ? AI_SETTINGS_MESSAGES.aiDisabled : AI_SETTINGS_MESSAGES.connectionPassed,
        result === "disabled" ? "info" : "success"
      );
    } catch (error) {
      setStatus("error");
      setNotice(TEST_CONNECTION_NOTICE_ID, userErrorMessage(error, AI_SETTINGS_MESSAGES.testFailed), "error");
    } finally {
      setIsTestingConnection(false);
    }
  }

  async function enableLocalDeepseek() {
    const needsDownload = !localDeepseek?.installed;
    setStatus(needsDownload ? "installingLocalModel" : "configuringLocalModel");
    if (needsDownload) {
      window.localStorage.setItem(localDeepseekEnablePendingKey, "1");
    } else {
      pushNotice(AI_SETTINGS_MESSAGES.configuringRuntime);
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
      if (!installed) throw new Error(AI_SETTINGS_MESSAGES.localModelStatusFailed);
      setLocalDeepseek(installed);
      if (needsDownload && !installed.installed) return;
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
      pushNotice(AI_SETTINGS_MESSAGES.localDeepseekEnabled, "success");
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
    } catch (error) {
      setStatus("error");
      setDownloadProgress((current) => (current ? { ...current, status: "failed" } : current));
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
      pushNotice(userErrorMessage(error, AI_SETTINGS_MESSAGES.downloadFailed), "error");
    }
  }

  async function cancelLocalModelDownload() {
    setStatus("cancellingLocalModelDownload");
    try {
      const current = await cancelLocalDeepseekDownload();
      setLocalDeepseek(current);
      setDownloadProgress((progress) => (progress ? { ...progress, status: "cancelled" } : null));
      window.localStorage.removeItem(localDeepseekEnablePendingKey);
      setStatus("idle");
      notifyLocalModelDownloadCancelled();
    } catch (error) {
      setStatus("error");
      pushNotice(userErrorMessage(error, AI_SETTINGS_MESSAGES.cancelDownloadFailed), "error");
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
      if (clearModelConfirmTimer.current !== null) window.clearTimeout(clearModelConfirmTimer.current);
      if (clearModelDoneTimer.current !== null) {
        window.clearTimeout(clearModelDoneTimer.current);
        clearModelDoneTimer.current = null;
      }
      setIsClearModelDone(false);
      setIsClearModelArmed(true);
      setStatus("idle");
      clearModelConfirmNoticeId.current = pushNotice(AI_SETTINGS_MESSAGES.clearModelConfirm);
      clearModelConfirmTimer.current = window.setTimeout(() => {
        setIsClearModelArmed(false);
        const noticeId = clearModelConfirmNoticeId.current;
        if (noticeId) {
          dismissNotice(noticeId);
        }
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
      pushNotice(AI_SETTINGS_MESSAGES.modelCleared, "success");
    } catch (error) {
      setStatus("error");
      pushNotice(userErrorMessage(error, AI_SETTINGS_MESSAGES.clearModelFailed), "error");
    }
  }

  async function copyLocalModelPath() {
    if (!localDeepseek?.filePath) return;
    await navigator.clipboard?.writeText(localDeepseek.filePath);
    if (copyPathTimer.current !== null) window.clearTimeout(copyPathTimer.current);
    setIsPathCopied(true);
    copyPathTimer.current = window.setTimeout(() => {
      setIsPathCopied(false);
      copyPathTimer.current = null;
    }, 1800);
    pushNotice(AI_SETTINGS_MESSAGES.modelPathCopied, "success");
  }

  return {
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
  };
}

export function formatBytes(value: number) {
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
