import { useCallback, useEffect, useRef, useState } from "react";
import type { AiConfig } from "../../features/ai/model/types";

let cachedConfigDraft: AiConfig | null = null;
let cachedHasUnsavedConfig = false;

export function useAiConfigDraft(initialConfig: AiConfig) {
  const [config, setConfig] = useState<AiConfig>(() => cachedConfigDraft ?? initialConfig);
  const configRef = useRef<AiConfig>(cachedConfigDraft ?? initialConfig);
  const hasUnsavedConfigRef = useRef(cachedHasUnsavedConfig);

  const applySavedConfig = useCallback((nextConfig: AiConfig) => {
    const safeConfig = { ...nextConfig, apiKey: "", clearApiKey: false };
    hasUnsavedConfigRef.current = false;
    configRef.current = safeConfig;
    cachedHasUnsavedConfig = false;
    cachedConfigDraft = safeConfig;
    setConfig(safeConfig);
  }, []);

  const updateConfigDraft = useCallback((nextConfig: AiConfig) => {
    hasUnsavedConfigRef.current = true;
    configRef.current = nextConfig;
    cachedHasUnsavedConfig = true;
    cachedConfigDraft = nextConfig;
    setConfig(nextConfig);
  }, []);

  useEffect(() => {
    configRef.current = config;
  }, [config]);

  return {
    config,
    configRef,
    hasUnsavedConfigRef,
    applySavedConfig,
    updateConfigDraft
  };
}
